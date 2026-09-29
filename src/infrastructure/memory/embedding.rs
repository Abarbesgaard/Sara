use std::collections::HashMap;
use std::sync::OnceLock;

pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Vec<f32>;
    fn dim(&self) -> usize;
}

const MODEL_BIN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/embedding-model/potion-8m-int8.bin"
));
const TOKENIZER_JSON: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/embedding-model/tokenizer.json"
));

pub struct StaticEmbedder {
    vocab: HashMap<String, u32>,
    unk_id: u32,
    matrix: Vec<i8>,
    scale: f32,
    dim: usize,
    vocab_n: usize,
    model_version: u32,
}

impl StaticEmbedder {
    pub fn load_bundled() -> Result<Self, String> {
        let b = MODEL_BIN;
        if b.len() < 20 || &b[0..4] != b"SEMB" {
            return Err("embedding matrix: bad magic".into());
        }
        let rd_u32 = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let version = rd_u32(4);
        if version != 1 {
            return Err(format!("embedding matrix: unsupported version {version}"));
        }
        let vocab_n = rd_u32(8) as usize;
        let dim = rd_u32(12) as usize;
        let scale = f32::from_le_bytes([b[16], b[17], b[18], b[19]]);
        let data = &b[20..];
        if data.len() != vocab_n * dim {
            return Err(format!(
                "embedding matrix: size mismatch (have {}, expected {})",
                data.len(),
                vocab_n * dim
            ));
        }
        let matrix: Vec<i8> = data.iter().map(|&x| x as i8).collect();

        let v: serde_json::Value =
            serde_json::from_slice(TOKENIZER_JSON).map_err(|e| format!("tokenizer.json: {e}"))?;
        let raw_vocab = v
            .get("model")
            .and_then(|m| m.get("vocab"))
            .and_then(|x| x.as_object())
            .ok_or("tokenizer.json: missing model.vocab")?;
        let mut vocab = HashMap::with_capacity(raw_vocab.len());
        for (tok, id) in raw_vocab {
            if let Some(id) = id.as_u64() {
                vocab.insert(tok.clone(), id as u32);
            }
        }
        let lookup = |t: &str| vocab.get(t).copied();
        let unk_id = lookup("[UNK]").ok_or("tokenizer.json: missing [UNK]")?;

        Ok(StaticEmbedder {
            unk_id,
            vocab,
            matrix,
            scale,
            dim,
            vocab_n,
            model_version: version,
        })
    }

    #[inline]
    fn row(&self, id: u32) -> &[i8] {
        let o = id as usize * self.dim;
        &self.matrix[o..o + self.dim]
    }

    fn tokenize(&self, text: &str) -> Vec<u32> {
        let mut ids = Vec::new();
        for word in pre_tokenize(text) {
            self.wordpiece(&word, &mut ids);
        }
        ids
    }

    fn wordpiece(&self, word: &str, out: &mut Vec<u32>) {
        let chars: Vec<char> = word.chars().collect();
        if chars.is_empty() {
            return;
        }
        if chars.len() > 100 {
            out.push(self.unk_id);
            return;
        }
        let mut start = 0;
        let mut pieces = Vec::new();
        while start < chars.len() {
            let mut end = chars.len();
            let mut found: Option<u32> = None;
            while end > start {
                let sub: String = chars[start..end].iter().collect();
                let cand = if start == 0 { sub } else { format!("##{sub}") };
                if let Some(&id) = self.vocab.get(&cand) {
                    found = Some(id);
                    break;
                }
                end -= 1;
            }
            match found {
                Some(id) => {
                    pieces.push(id);
                    start = end;
                }
                None => {
                    out.push(self.unk_id);
                    return;
                }
            }
        }
        out.extend(pieces);
    }
}

impl Embedder for StaticEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        let ids = self.tokenize(text);
        let mut acc = vec![0.0f32; self.dim];
        let mut n = 0usize;
        for id in ids {
            if (id as usize) >= self.vocab_n {
                continue;
            }
            let row = self.row(id);
            for (a, &q) in acc.iter_mut().zip(row) {
                *a += q as f32 * self.scale;
            }
            n += 1;
        }
        if n == 0 {
            return acc;
        }
        let inv = 1.0 / n as f32;
        for a in acc.iter_mut() {
            *a *= inv;
        }
        l2_normalize(&mut acc);
        acc
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

impl StaticEmbedder {
    pub fn fingerprint(&self) -> String {
        format!(
            "m2v/v{}/n{}/d{}/s{:08x}",
            self.model_version,
            self.vocab_n,
            self.dim,
            self.scale.to_bits()
        )
    }
}

const MEMORY_EMBED_TEXT_VERSION: u32 = 1;

const SCHEME_VERSION_KEY: &str = "embedding_scheme_version";

fn compose_scheme_version(model_fingerprint: &str, text_version: u32) -> String {
    format!("{model_fingerprint}|text{text_version}")
}

pub fn scheme_version() -> String {
    compose_scheme_version(&bundled().fingerprint(), MEMORY_EMBED_TEXT_VERSION)
}

fn needs_reindex(stored: Option<&str>, current: &str) -> bool {
    stored != Some(current)
}

pub fn ensure_index_current(conn: &rusqlite::Connection) -> anyhow::Result<bool> {
    let current = scheme_version();
    let stored = crate::infrastructure::db::meta_get(conn, SCHEME_VERSION_KEY)?;
    if !needs_reindex(stored.as_deref(), &current) {
        return Ok(false);
    }
    reindex_all(conn)?;
    crate::infrastructure::db::meta_set(conn, SCHEME_VERSION_KEY, &current)?;
    Ok(true)
}

pub fn bundled() -> &'static StaticEmbedder {
    static E: OnceLock<StaticEmbedder> = OnceLock::new();
    E.get_or_init(|| StaticEmbedder::load_bundled().expect("bundled embedding model is valid"))
}

fn memory_embed_text(item: &crate::infrastructure::model::Item) -> String {
    let detail = item.summary.clone().unwrap_or_else(|| item.body.clone());
    if item.title.trim().is_empty() {
        detail
    } else {
        format!("{}\n{}", item.title, detail)
    }
}

pub fn index_memory(conn: &rusqlite::Connection, item: &crate::infrastructure::model::Item) {
    let text = memory_embed_text(item);
    if text.trim().is_empty() {
        return;
    }
    let v = bundled().embed(&text);
    let _ = crate::infrastructure::db::upsert_embedding(conn, &item.uuid.to_string(), &v);
}

pub fn reindex_all(conn: &rusqlite::Connection) -> anyhow::Result<usize> {
    let emb = bundled();
    let memories = crate::infrastructure::db::list_memories(conn)?;
    let mut n = 0;
    for m in &memories {
        let text = memory_embed_text(m);
        if text.trim().is_empty() {
            continue;
        }
        let v = emb.embed(&text);
        crate::infrastructure::db::upsert_embedding(conn, &m.uuid.to_string(), &v)?;
        n += 1;
    }
    Ok(n)
}

fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (&x, &y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

fn pre_tokenize(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, words: &mut Vec<String>| {
        if !cur.is_empty() {
            words.push(std::mem::take(cur));
        }
    };
    for ch in text.chars() {
        if ch.is_control() || ch.is_whitespace() {
            flush(&mut cur, &mut words);
            continue;
        }
        if is_punct(ch) {
            flush(&mut cur, &mut words);
            words.push(ch.to_lowercase().collect());
            continue;
        }
        for lc in ch.to_lowercase() {
            cur.push(lc);
        }
    }
    flush(&mut cur, &mut words);
    words
}

fn is_punct(ch: char) -> bool {
    ch.is_ascii_punctuation() || ch.is_ascii_graphic() && !ch.is_alphanumeric() || {
        matches!(ch, '\u{2000}'..='\u{206F}' | '\u{3000}'..='\u{303F}')
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/infrastructure/embedding.rs"]
mod tests;
