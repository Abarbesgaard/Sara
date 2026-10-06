use super::super::Hit;
use super::{compare, rank};
use chrono::{DateTime, TimeZone, Utc};
use std::cmp::Ordering;

fn hit(
    label: &str,
    exact_match: bool,
    strength: f64,
    fts_rank: Option<usize>,
    cosine: Option<f32>,
    modified: Option<DateTime<Utc>>,
) -> Hit {
    Hit {
        ref_kind: "item_memory".into(),
        label: label.into(),
        description: String::new(),
        snippet: String::new(),
        body: String::new(),
        strength,
        exact_match,
        loose: false,
        fts_rank,
        modified,
        files: vec![],
        linked_tasks: vec![],
        superseded_by: vec![],
        provisional: false,
        item_uuid: None,
        derived_from_labels: vec![],
        derived_children: vec![],
        semantic: cosine.is_some(),
        cosine,
        cluster: None,
        stale: vec![],
        provenance: Default::default(),
    }
}

fn at(secs: i64) -> Option<DateTime<Utc>> {
    Some(Utc.timestamp_opt(secs, 0).unwrap())
}

fn order(hits: Vec<Hit>) -> Vec<String> {
    rank(hits).into_iter().map(|h| h.label).collect()
}

#[test]
fn exact_match_beats_stronger_fts_hit() {
    let exact = hit("exact", true, 1.0, None, None, at(100));
    let fts = hit("fts", false, 9.0, Some(0), None, at(100));
    assert_eq!(order(vec![fts, exact]), vec!["exact", "fts"]);
}

#[test]
fn file_prefix_match_leads_via_exact_flag() {
    let file = hit("file", true, 1.0, None, None, at(100));
    let lexical = hit("lexical", false, 5.0, Some(0), None, at(200));
    assert_eq!(order(vec![lexical, file]), vec!["file", "lexical"]);
}

#[test]
fn stronger_hit_wins_among_equal_exactness() {
    let weak = hit("weak", false, 1.0, Some(0), None, at(100));
    let strong = hit("strong", false, 4.0, Some(0), None, at(100));
    assert_eq!(order(vec![weak, strong]), vec!["strong", "weak"]);
}

#[test]
fn lexical_beats_semantic_on_equal_strength() {
    let lexical = hit("lexical", false, 1.0, Some(0), None, at(100));
    let semantic = hit("semantic", false, 1.0, None, Some(0.95), at(100));
    assert_eq!(order(vec![semantic, lexical]), vec!["lexical", "semantic"]);
}

#[test]
fn better_bm25_rank_wins_among_lexical_hits() {
    let worse = hit("worse", false, 1.0, Some(5), None, at(100));
    let better = hit("better", false, 1.0, Some(0), None, at(100));
    assert_eq!(order(vec![worse, better]), vec!["better", "worse"]);
}

#[test]
fn higher_cosine_wins_among_semantic_hits() {
    let low = hit("low", false, 1.0, None, Some(0.60), at(100));
    let high = hit("high", false, 1.0, None, Some(0.90), at(100));
    assert_eq!(order(vec![low, high]), vec!["high", "low"]);
}

#[test]
fn recency_is_the_final_tie_break() {
    let older = hit("older", false, 1.0, Some(0), None, at(100));
    let newer = hit("newer", false, 1.0, Some(0), None, at(200));
    assert_eq!(order(vec![older, newer]), vec!["newer", "older"]);
}

#[test]
fn compare_is_consistent_with_rank() {
    let a = hit("a", true, 1.0, None, None, at(100));
    let b = hit("b", false, 9.0, Some(0), None, at(100));
    assert_eq!(compare(&a, &b), Ordering::Less);
}
