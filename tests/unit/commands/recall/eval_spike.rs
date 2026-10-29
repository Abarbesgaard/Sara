//! Recall-quality eval harness for the vector-index spike (task 442af1d1).
//!
//! Ignored by default: it needs a COPY of a real sara DB (recall writes usage
//! rows) and an eval file produced by `spikes/vector-index/mine_eval.py`.
//!
//! ```sh
//! cp "$HOME/Library/Application Support/sara/tasks.db" /tmp/eval.db
//! SARA_EVAL_DB=/tmp/eval.db SARA_EVAL_FILE=eval.jsonl \
//!   cargo test --release eval_spike -- --ignored --nocapture
//! ```
//!
//! Each eval line is `{source, query, relevant: [uuid], cutoff, exclude}`. A
//! candidate counts only if it was created before `cutoff` and is not in
//! `exclude`, so memories learned *from* the query's own task cannot leak in.

use super::*;
use crate::commands::recall::search::fts;
use crate::infrastructure::memory::embedding::{self, Embedder};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

const RRF_K: f64 = 60.0;
const POOL: usize = 50;
/// recall passes `limit.max(50)` to FTS over tasks, notes AND memories.
const FTS_LIMIT: i64 = 50;

#[derive(serde::Deserialize)]
struct Case {
    source: String,
    query: String,
    relevant: Vec<String>,
    cutoff: Option<String>,
    exclude: Vec<String>,
}

#[derive(Default)]
struct Score {
    n: usize,
    r5: f64,
    r10: f64,
    mrr: f64,
    hit1: f64,
    time: Duration,
    per_query: Vec<f64>,
}

impl Score {
    fn add(&mut self, ranked: &[String], relevant: &HashSet<String>, t: Duration) {
        let found = |k: usize| {
            ranked
                .iter()
                .take(k)
                .filter(|u| relevant.contains(*u))
                .count() as f64
                / relevant.len() as f64
        };
        self.n += 1;
        self.r5 += found(5);
        self.r10 += found(10);
        let rr = ranked
            .iter()
            .take(10)
            .position(|u| relevant.contains(u))
            .map_or(0.0, |pos| 1.0 / (pos + 1) as f64);
        self.per_query.push(rr);
        if let Some(pos) = ranked.iter().take(10).position(|u| relevant.contains(u)) {
            self.mrr += 1.0 / (pos + 1) as f64;
            if pos == 0 {
                self.hit1 += 1.0;
            }
        }
        self.time += t;
    }
}

struct Corpus {
    created: HashMap<String, String>,
    strength: HashMap<String, f64>,
    vectors: Vec<(String, Vec<f32>)>,
}

impl Corpus {
    fn allowed(&self, uuid: &str, case: &Case) -> bool {
        let Some(created) = self.created.get(uuid) else {
            return false;
        };
        !case.exclude.iter().any(|e| e == uuid) && case.cutoff.as_ref().is_none_or(|c| created < c)
    }
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphabetic())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 3 && !fts::STOP_WORDS.contains(&w.as_str()))
        .take(fts::MAX_AND_TOKENS)
        .collect()
}

/// The product's phrase → AND → OR cascade (recall::search::fts), but each
/// stage only "succeeds" if it finds an allowed memory, as it would have on
/// the corpus at the time the query was made.
fn fts_order(conn: &Connection, corpus: &Corpus, case: &Case) -> Vec<String> {
    let keep = |hits: Vec<db::SearchHit>| -> Vec<String> {
        let mut seen = HashSet::new();
        hits.into_iter()
            .filter(|h| h.ref_kind.starts_with("item_"))
            .map(|h| h.task_uuid)
            .filter(|u| corpus.allowed(u, case) && seen.insert(u.clone()))
            .take(POOL)
            .collect()
    };
    let limit = FTS_LIMIT;
    let phrase = keep(db::search_fts(conn, &case.query, limit).unwrap());
    if !phrase.is_empty() {
        return phrase;
    }
    let toks = tokens(&case.query);
    if toks.is_empty() {
        return vec![];
    }
    let and = keep(db::search_fts_tokens(conn, &toks, limit).unwrap());
    if !and.is_empty() || toks.len() < 2 {
        return and;
    }
    keep(db::search_fts_tokens_or(conn, &toks, limit).unwrap())
}

/// Emulates collect_hits + scoring::compare on the time-travelled corpus: FTS
/// hits, then up to `top_k` semantic hits >= threshold not already found,
/// ordered strength-first, then FTS rank, then cosine. (No cluster collapse.)
fn current_emulated(
    conn: &Connection,
    corpus: &Corpus,
    case: &Case,
    semantic: bool,
) -> Vec<String> {
    let f = fts_order(conn, corpus, case);
    let mut rows: Vec<(String, Option<usize>, Option<f32>)> = f
        .iter()
        .enumerate()
        .map(|(i, u)| (u.clone(), Some(i), None))
        .collect();
    if semantic {
        let have: HashSet<String> = f.into_iter().collect();
        rows.extend(
            vector_order(corpus, case, 0.30)
                .into_iter()
                .filter(|(u, _)| !have.contains(u))
                .take(5)
                .map(|(u, c)| (u, None, Some(c))),
        );
    }
    rows.sort_by(|a, b| {
        corpus.strength[&b.0]
            .total_cmp(&corpus.strength[&a.0])
            .then(a.1.unwrap_or(usize::MAX).cmp(&b.1.unwrap_or(usize::MAX)))
            .then(b.2.unwrap_or(f32::MIN).total_cmp(&a.2.unwrap_or(f32::MIN)))
    });
    rows.into_iter().map(|r| r.0).collect()
}

fn vector_order(corpus: &Corpus, case: &Case, threshold: f32) -> Vec<(String, f32)> {
    let qv = embedding::bundled().embed(&case.query);
    let mut scored: Vec<(String, f32)> = corpus
        .vectors
        .iter()
        .filter(|(u, _)| corpus.allowed(u, case))
        .map(|(u, v)| (u.clone(), embedding::cosine(&qv, v)))
        .filter(|(_, c)| *c >= threshold)
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(POOL);
    scored
}

fn rrf(lists: &[(&[String], f64)]) -> Vec<String> {
    let mut score: HashMap<&str, f64> = HashMap::new();
    for (list, weight) in lists {
        for (rank, u) in list.iter().enumerate() {
            *score.entry(u).or_default() += weight / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut fused: Vec<(&str, f64)> = score.into_iter().collect();
    fused.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
    fused.into_iter().map(|(u, _)| u.to_string()).collect()
}

fn current(
    conn: &Connection,
    corpus: &Corpus,
    case: &Case,
    semantic: &SemanticOpts,
) -> Vec<String> {
    collect_hits(conn, &case.query, &[], &[], &[], 200, semantic)
        .unwrap()
        .into_iter()
        .filter_map(|h| h.item_uuid.map(|u| u.to_string()))
        .filter(|u| corpus.allowed(u, case))
        .collect()
}

fn load_corpus(conn: &Connection) -> Corpus {
    let items = db::list_memories(conn).unwrap();
    let strength: HashMap<String, f64> = db::item_strengths(conn, &items)
        .into_iter()
        .map(|(u, s)| (u.to_string(), s))
        .collect();
    let created: HashMap<String, String> = items
        .iter()
        .map(|i| (i.uuid.to_string(), i.created.to_rfc3339()))
        .collect();
    let vectors = db::active_embeddings(conn)
        .unwrap()
        .into_iter()
        .filter(|(u, _)| created.contains_key(u))
        .collect();
    Corpus {
        created,
        strength,
        vectors,
    }
}

#[test]
#[ignore = "spike eval: needs SARA_EVAL_DB (a copy) and SARA_EVAL_FILE"]
fn eval_spike() {
    let db_path = std::env::var("SARA_EVAL_DB").expect("SARA_EVAL_DB");
    let eval_path = std::env::var("SARA_EVAL_FILE").expect("SARA_EVAL_FILE");
    let conn = Connection::open(&db_path).unwrap();
    let corpus = load_corpus(&conn);
    let cases: Vec<Case> = std::fs::read_to_string(&eval_path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let on = SemanticOpts {
        enabled: true,
        threshold: 0.30,
        top_k: 5,
    };

    let mut table: BTreeMap<(String, &str), Score> = BTreeMap::new();
    for case in &cases {
        let relevant: HashSet<String> = case.relevant.iter().cloned().collect();
        let mut run = |name: &'static str, f: &mut dyn FnMut() -> Vec<String>| {
            let t = Instant::now();
            let ranked = f();
            let dt = t.elapsed();
            for src in [case.source.clone(), "all".to_string()] {
                table
                    .entry((src, name))
                    .or_default()
                    .add(&ranked, &relevant, dt);
            }
        };
        run(
            "0 product path (hand set only: no time travel)",
            &mut || {
                if case.source == "hand" {
                    current(&conn, &corpus, case, &on)
                } else {
                    vec![]
                }
            },
        );
        run("1 current (fts+sem, strength-first)", &mut || {
            current_emulated(&conn, &corpus, case, true)
        });
        run("2 current, semantic off", &mut || {
            current_emulated(&conn, &corpus, case, false)
        });
        run("3 fts rank only", &mut || fts_order(&conn, &corpus, case));
        run("4 vector only", &mut || {
            vector_order(&corpus, case, f32::MIN)
                .into_iter()
                .map(|(u, _)| u)
                .collect()
        });
        run("5 rrf(fts, vector)", &mut || {
            let f = fts_order(&conn, &corpus, case);
            let v: Vec<String> = vector_order(&corpus, case, f32::MIN)
                .into_iter()
                .map(|(u, _)| u)
                .collect();
            rrf(&[(&f, 1.0), (&v, 1.0)])
        });
        run("6 rrf(fts, vector>=0.30)", &mut || {
            let f = fts_order(&conn, &corpus, case);
            let v: Vec<String> = vector_order(&corpus, case, 0.30)
                .into_iter()
                .map(|(u, _)| u)
                .collect();
            rrf(&[(&f, 1.0), (&v, 1.0)])
        });
        for (name, w) in [
            ("7a rrf(fts, vector, strength*0.25)", 0.25),
            ("7b rrf(fts, vector, strength*0.5)", 0.5),
            ("7c rrf(fts, vector, strength*1.0)", 1.0),
        ] {
            run(name, &mut || {
                let f = fts_order(&conn, &corpus, case);
                let v: Vec<String> = vector_order(&corpus, case, f32::MIN)
                    .into_iter()
                    .map(|(u, _)| u)
                    .collect();
                let mut pool: Vec<String> = f.iter().chain(v.iter()).cloned().collect();
                pool.sort();
                pool.dedup();
                pool.sort_by(|a, b| corpus.strength[b].total_cmp(&corpus.strength[a]));
                rrf(&[(&f, 1.0), (&v, 1.0), (&pool, w)])
            });
        }
    }

    println!(
        "\n{} cases, {} vectors in corpus\n",
        cases.len(),
        corpus.vectors.len()
    );
    println!("| source | variant | n | recall@5 | recall@10 | MRR@10 | hit@1 | ms/query |");
    println!("|---|---|---:|---:|---:|---:|---:|---:|");
    for ((src, name), s) in &table {
        if name.starts_with('0') && src != "hand" {
            continue;
        }
        let n = s.n as f64;
        println!(
            "| {src} | {name} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.1} |",
            s.n,
            s.r5 / n,
            s.r10 / n,
            s.mrr / n,
            s.hit1 / n,
            s.time.as_secs_f64() * 1000.0 / n
        );
    }

    println!("\nPaired bootstrap (10k resamples) of MRR@10 delta vs variant 1, per source:\n");
    println!("| source | variant | ΔMRR | 95% CI |");
    println!("|---|---|---:|---|");
    for ((src, name), s) in &table {
        if name.starts_with('0') || name.starts_with('1') {
            continue;
        }
        let base = &table[&(src.clone(), "1 current (fts+sem, strength-first)")].per_query;
        let d: Vec<f64> = s.per_query.iter().zip(base).map(|(a, b)| a - b).collect();
        let (mean, lo, hi) = bootstrap(&d);
        println!("| {src} | {name} | {mean:+.3} | [{lo:+.3}, {hi:+.3}] |");
    }
}

fn bootstrap(d: &[f64]) -> (f64, f64, f64) {
    let n = d.len();
    let mean = d.iter().sum::<f64>() / n as f64;
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    let mut means: Vec<f64> = (0..10_000)
        .map(|_| (0..n).map(|_| d[next()]).sum::<f64>() / n as f64)
        .collect();
    means.sort_by(f64::total_cmp);
    (mean, means[250], means[9_750])
}
