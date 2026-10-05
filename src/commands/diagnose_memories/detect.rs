use std::collections::{HashMap, HashSet};

use super::types::{ConflictCandidate, Corpus, MemInfo};
use crate::infrastructure::memory::embedding;

const SNIPPET_CHARS: usize = 80;

pub(super) fn find_conflicts(corpus: &Corpus, threshold: f32) -> Vec<ConflictCandidate> {
    let infos = &corpus.infos;
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut candidates = Vec::new();

    for (i, j) in file_sharing_pairs(infos) {
        if !seen.insert((i, j)) {
            continue;
        }
        let (a, b) = (&infos[i], &infos[j]);
        let shared_files = a
            .files
            .iter()
            .filter(|f| b.files.contains(f))
            .cloned()
            .collect();
        candidates.extend(candidate(corpus, a, b, threshold, shared_files, vec![]));
    }

    for i in 0..infos.len() {
        for j in (i + 1)..infos.len() {
            let (a, b) = (&infos[i], &infos[j]);
            if seen.contains(&(i, j)) || !same_tag_set(a, b) {
                continue;
            }
            seen.insert((i, j));
            candidates.extend(candidate(corpus, a, b, threshold, vec![], a.tags.clone()));
        }
    }

    sort_by_similarity(&mut candidates);
    candidates
}

fn file_sharing_pairs(infos: &[MemInfo]) -> Vec<(usize, usize)> {
    let mut by_file: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, info) in infos.iter().enumerate() {
        for f in &info.files {
            by_file.entry(f.as_str()).or_default().push(i);
        }
    }
    let mut pairs = Vec::new();
    for indices in by_file.values() {
        for &i in indices {
            for &j in indices {
                if i < j {
                    pairs.push((i, j));
                }
            }
        }
    }
    pairs
}

fn same_tag_set(a: &MemInfo, b: &MemInfo) -> bool {
    if a.tags.is_empty() || b.tags.is_empty() {
        return false;
    }
    let a_set: HashSet<&String> = a.tags.iter().collect();
    let b_set: HashSet<&String> = b.tags.iter().collect();
    a_set == b_set
}

fn candidate(
    corpus: &Corpus,
    a: &MemInfo,
    b: &MemInfo,
    threshold: f32,
    shared_files: Vec<String>,
    shared_tags: Vec<String>,
) -> Option<ConflictCandidate> {
    if corpus.linked.contains(&(a.uuid.clone(), b.uuid.clone())) {
        return None;
    }
    let cosine = match (corpus.vectors.get(&a.uuid), corpus.vectors.get(&b.uuid)) {
        (Some(va), Some(vb)) => Some(embedding::cosine(va, vb)),
        _ => None,
    };
    if cosine.is_some_and(|c| c < threshold) {
        return None;
    }
    Some(ConflictCandidate {
        label_a: a.label.clone(),
        label_b: b.label.clone(),
        snippet_a: a.body.chars().take(SNIPPET_CHARS).collect(),
        snippet_b: b.body.chars().take(SNIPPET_CHARS).collect(),
        shared_files,
        shared_tags,
        cosine,
    })
}

fn sort_by_similarity(candidates: &mut [ConflictCandidate]) {
    candidates.sort_by(|a, b| {
        let ka = a.cosine.unwrap_or(f32::NEG_INFINITY);
        let kb = b.cosine.unwrap_or(f32::NEG_INFINITY);
        kb.partial_cmp(&ka)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.label_a.cmp(&b.label_a))
            .then_with(|| a.label_b.cmp(&b.label_b))
    });
}
