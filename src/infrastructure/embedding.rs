//! Semantic-recall embedder.
//!
//! Sara's recall is lexical (FTS5) by default, so paraphrased or conceptually
//! similar memories with different wording are missed. This module adds a small,
//! **bundled, local** embedding model so recall can also rank by meaning — no
//! daemon, no network, no runtime download.
//!
//! The model is a [model2vec](https://github.com/MinishLab/model2vec) *static*
//! embedding model (`minishlab/potion-base-8M`): a plain token→vector lookup
//! table. Inference is therefore trivially cheap and pure-Rust — no neural
//! runtime:
//!
//! ```text
//! tokenize (BERT WordPiece) → gather a matrix row per token → mean-pool → L2-normalize
//! ```
//!
//! The matrix (int8-quantized from the original f32 `[29528, 256]`) and the
//! tokenizer are `include_bytes!`d into the binary, so a `sara` build is fully
//! self-contained. See `assets/embedding-model/PROVENANCE.md`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Anything that can turn text into a dense, L2-normalized vector. A trait so
/// tests can substitute a deterministic fake and the backend can evolve.
pub trait Embedder: Send + Sync {
    /// Embed `text` into a unit-length vector of length [`Embedder::dim`].
    fn embed(&self, text: &str) -> Vec<f32>;
    /// Dimensionality of the produced vectors.
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

/// The bundled model2vec static-embedding backend.
pub struct StaticEmbedder {
    vocab: HashMap<String, u32>,
    unk_id: u32,
    /// Row-major int8 matrix, `vocab_n * dim`. Dequantize with `* scale`.
    matrix: Vec<i8>,
    scale: f32,
    dim: usize,
    vocab_n: usize,
}

impl StaticEmbedder {
    /// Parse the `include_bytes!`d model + tokenizer. Fails only if the vendored
    /// bytes are corrupt (a build/packaging error, never a runtime condition).
    pub fn load_bundled() -> Result<Self, String> {
        // ── matrix blob ── magic "SEMB", u32 version, u32 vocab, u32 dim, f32 scale, i8 data
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

        // ── tokenizer vocab ── parse only `model.vocab` (token → id) from tokenizer.json.
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
        })
    }

    /// Dequantized matrix row (the embedding for token `id`).
    #[inline]
    fn row(&self, id: u32) -> &[i8] {
        let o = id as usize * self.dim;
        &self.matrix[o..o + self.dim]
    }

    /// BERT-style WordPiece tokenization → token ids.
    ///
    /// A self-contained implementation (no heavy `tokenizers` C-dependency):
    /// because index-time and query-time use *this same* tokenizer, cosine
    /// ranking is self-consistent regardless of tiny deviations from the
    /// reference tokenizer. Special tokens ([CLS]/[SEP]) are intentionally
    /// omitted — model2vec pools content tokens only.
    fn tokenize(&self, text: &str) -> Vec<u32> {
        let mut ids = Vec::new();
        for word in pre_tokenize(text) {
            self.wordpiece(&word, &mut ids);
        }
        ids
    }

    /// Greedy longest-match WordPiece for a single pre-token. Continuation
    /// pieces carry the `##` prefix; an unmatchable word maps to `[UNK]`.
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
                let cand = if start == 0 {
                    sub
                } else {
                    format!("##{sub}")
                };
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
                    // Any piece unmatchable → whole word is [UNK].
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
            return acc; // all-zero for empty/OOV input
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

/// The process-wide bundled embedder, loaded once on first use. The model is
/// compiled into the binary, so this is always available (it only fails on a
/// corrupt build, which `load_bundled` would surface at first call).
pub fn bundled() -> &'static StaticEmbedder {
    static E: OnceLock<StaticEmbedder> = OnceLock::new();
    E.get_or_init(|| StaticEmbedder::load_bundled().expect("bundled embedding model is valid"))
}

/// The text a memory is embedded from: its title plus its body/summary, so both
/// the headline and the detail contribute to the semantic vector.
fn memory_embed_text(item: &crate::infrastructure::model::Item) -> String {
    let detail = item.summary.clone().unwrap_or_else(|| item.body.clone());
    if item.title.trim().is_empty() {
        detail
    } else {
        format!("{}\n{}", item.title, detail)
    }
}

/// Embed a single memory and store its vector (best-effort). Called after a
/// memory is learned so the semantic index stays current. Silently no-ops on
/// failure — a missing embedding only means that memory won't match semantically.
pub fn index_memory(conn: &rusqlite::Connection, item: &crate::infrastructure::model::Item) {
    let text = memory_embed_text(item);
    if text.trim().is_empty() {
        return;
    }
    let v = bundled().embed(&text);
    let _ = crate::infrastructure::db::upsert_embedding(conn, &item.uuid.to_string(), &v);
}

/// (Re)build the semantic index over every active memory. Returns the number of
/// memories embedded. Backs the `sara reindex-embeddings` command.
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

/// L2-normalize in place. A zero vector is left untouched.
fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Cosine similarity of two vectors. For L2-normalized inputs this equals the
/// dot product; the explicit normalization keeps it correct regardless.
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

/// BERT pre-tokenization: normalize (clean control chars, lowercase) then split
/// on whitespace, isolating punctuation into standalone pieces.
fn pre_tokenize(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, words: &mut Vec<String>| {
        if !cur.is_empty() {
            words.push(std::mem::take(cur));
        }
    };
    for ch in text.chars() {
        // clean_text: drop control chars; treat all whitespace as a separator.
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

/// BERT treats ASCII punctuation and Unicode punctuation as standalone tokens.
fn is_punct(ch: char) -> bool {
    ch.is_ascii_punctuation() || ch.is_ascii_graphic() && !ch.is_alphanumeric() || {
        // General Unicode punctuation blocks (best-effort; English dev text is
        // dominated by ASCII, so this only needs to be reasonable).
        matches!(ch, '\u{2000}'..='\u{206F}' | '\u{3000}'..='\u{303F}')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_related_closer_than_unrelated() {
        // The whole point of semantic recall: a paraphrase must be closer than
        // an unrelated sentence, even with different surface wording.
        let e = bundled();
        let anchor = e.embed("dependabot bump broke the restore step; pin the lockfile version");
        let related = e.embed("dependency update caused a CI build failure");
        let unrelated = e.embed("how to bake sourdough bread at home");
        let rel = cosine(&anchor, &related);
        let unrel = cosine(&anchor, &unrelated);
        assert!(
            rel > unrel,
            "related ({rel}) should exceed unrelated ({unrel})"
        );
    }

    #[test]
    fn embedding_is_deterministic_normalized_and_sized() {
        let e = bundled();
        assert_eq!(e.dim(), 256);
        let a = e.embed("stacked PR auto-closed when its base branch was deleted");
        let b = e.embed("stacked PR auto-closed when its base branch was deleted");
        assert_eq!(a, b, "embedding must be deterministic");
        assert_eq!(a.len(), 256);
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "expected unit norm, got {norm}");
    }

    #[test]
    fn embedding_empty_text_is_zero_vector() {
        let e = bundled();
        let v = e.embed("   ");
        assert_eq!(v.len(), 256);
        assert!(v.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn cosine_of_identical_is_one() {
        let e = bundled();
        let v = e.embed("reciprocal rank fusion merges lexical and semantic hits");
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-4);
    }
}
