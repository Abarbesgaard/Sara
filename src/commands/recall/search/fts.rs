use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

pub(in crate::commands::recall) const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "in", "it", "of", "to", "for", "on", "at", "by", "up", "as", "or",
    "do", "if", "be", "we", "he", "she", "they", "but", "and", "not", "with", "from", "this",
    "that", "are", "was", "has", "have", "how", "what", "does", "did",
];
pub(in crate::commands::recall) const MAX_AND_TOKENS: usize = 6;

fn meaningful_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphabetic())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(&w.as_str()))
        .take(MAX_AND_TOKENS)
        .collect()
}

pub(in crate::commands::recall) fn search(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> Result<(Vec<db::SearchHit>, bool)> {
    Ok(if query.is_empty() {
        (vec![], false)
    } else {
        let phrase = db::search_fts(conn, query, limit.max(50))?;
        if !phrase.is_empty() {
            (phrase, false)
        } else {
            let tokens = meaningful_tokens(query);
            if tokens.is_empty() {
                (vec![], false)
            } else {
                let and_hits = db::search_fts_tokens(conn, &tokens, limit.max(50))?;
                if !and_hits.is_empty() || tokens.len() < 2 {
                    (and_hits, false)
                } else {
                    (
                        db::search_fts_tokens_or(conn, &tokens, limit.max(50))?,
                        true,
                    )
                }
            }
        }
    })
}
