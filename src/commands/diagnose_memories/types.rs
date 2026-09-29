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
