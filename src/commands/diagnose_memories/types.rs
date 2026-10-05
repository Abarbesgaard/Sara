use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub(super) struct ConflictCandidate {
    pub(super) label_a: String,
    pub(super) label_b: String,
    pub(super) snippet_a: String,
    pub(super) snippet_b: String,
    pub(super) shared_files: Vec<String>,
    pub(super) shared_tags: Vec<String>,
    pub(super) cosine: Option<f32>,
}

pub(super) struct MemInfo {
    pub(super) uuid: String,
    pub(super) label: String,
    pub(super) body: String,
    pub(super) files: Vec<String>,
    pub(super) tags: Vec<String>,
}

pub(super) struct Corpus {
    pub(super) infos: Vec<MemInfo>,
    pub(super) linked: HashSet<(String, String)>,
    pub(super) vectors: HashMap<String, Vec<f32>>,
}
