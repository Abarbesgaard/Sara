use std::cmp::Ordering;

use crate::commands::recall::types::Hit;

pub(in crate::commands::recall) fn compare(a: &Hit, b: &Hit) -> Ordering {
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

pub(in crate::commands::recall) fn rank(mut hits: Vec<Hit>) -> Vec<Hit> {
    hits.sort_by(compare);
    hits
}

#[cfg(test)]
#[path = "../../../../tests/unit/commands/recall/scoring.rs"]
mod tests;
