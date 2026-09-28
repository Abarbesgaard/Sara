//! Pure recall ranking — the scoring *blend* with no DB or stdout dependency.
//!
//! `collect_hits` fetches candidates (FTS, exact-filter, semantic) and stamps
//! each [`Hit`] with the signals the ranking cares about (`exact_match`,
//! `strength`, `fts_rank`, `cosine`, `modified`). This module then orders them
//! by a fixed precedence, taking only an already-hydrated slice — so the
//! ranking math can be unit-tested against fixed inputs instead of only
//! end-to-end through a live database.

use std::cmp::Ordering;

use super::Hit;

/// The ranking precedence for two already-hydrated hits:
///
/// 1. **exact_match** — tag/project/file filter hits lead plain FTS hits.
/// 2. **strength** — linkage-derived confidence (higher wins).
/// 3. **fts_rank** — bm25 relevance for lexical hits (lower = better match;
///    `None`, i.e. exact/semantic hits, sorts last so it falls through).
/// 4. **cosine** — semantic similarity tie-break (higher wins; `None` last).
/// 5. **modified** — most-recently-modified wins.
///
/// Pure: reads only precomputed [`Hit`] signals, never touching a connection
/// or performing I/O.
pub(super) fn compare(a: &Hit, b: &Hit) -> Ordering {
    b.exact_match
        .cmp(&a.exact_match)
        .then(
            b.strength
                .partial_cmp(&a.strength)
                .unwrap_or(Ordering::Equal),
        )
        .then(
            a.fts_rank
                .unwrap_or(usize::MAX)
                .cmp(&b.fts_rank.unwrap_or(usize::MAX)),
        )
        .then(
            b.cosine
                .unwrap_or(f32::MIN)
                .partial_cmp(&a.cosine.unwrap_or(f32::MIN))
                .unwrap_or(Ordering::Equal),
        )
        .then(b.modified.cmp(&a.modified))
}

/// Rank a set of already-fetched candidates by the recall blend, returning them
/// best-first. Pure — no DB, no stdout — so callers feed fixed inputs in tests.
pub(super) fn rank(mut hits: Vec<Hit>) -> Vec<Hit> {
    hits.sort_by(compare);
    hits
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/recall/scoring.rs"]
mod tests;
