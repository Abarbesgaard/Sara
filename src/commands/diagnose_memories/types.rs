/// A pair of memories that may be in conflict (no memory_links edge between them).
#[derive(Debug)]
pub(super) struct ConflictCandidate {
    pub(super) label_a: String,
    pub(super) label_b: String,
    pub(super) snippet_a: String,
    pub(super) snippet_b: String,
    pub(super) shared_files: Vec<String>,
    pub(super) shared_tags: Vec<String>,
    /// Embedding cosine between the two bodies. `None` when either memory has
    /// no embedding row yet — such a pair is kept (fail-open) so a lagging
    /// index never silently hides a candidate.
    pub(super) cosine: Option<f32>,
}
