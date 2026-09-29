use crate::infrastructure::model::Item;

/// What a dendrite points at.
#[derive(Clone)]
pub(super) enum NodeKind {
    /// Another memory (enterable): label + relation.
    Memory { label: String, relation: String },
    /// A linked task: display id + description + link source (auto/explicit).
    Task {
        id: String,
        desc: String,
        source: String,
    },
    /// An associated file path.
    File { name: String },
}

#[derive(Clone)]
pub(super) struct Neighbor {
    pub(super) kind: NodeKind,
}

pub(super) struct DreamData {
    pub(super) item: Item,
    pub(super) label: String,
    pub(super) strength: f64,
    pub(super) provisional: bool,
    pub(super) files: Vec<String>,
    pub(super) neighbors: Vec<Neighbor>,
    pub(super) sparkline: Vec<u64>,
    pub(super) recall_total_30d: u64,
}

pub(super) struct Star {
    pub(super) label: String,
    pub(super) title: String,
    pub(super) strength: f64,
    pub(super) provisional: bool,
    pub(super) tags: Vec<String>,
    /// Lowercased searchable text: label + tags + title + body.
    pub(super) haystack: String,
    /// Canvas position, produced by the force layout.
    pub(super) x: f64,
    pub(super) y: f64,
    /// Recalled within the last 7 days — pulses.
    pub(super) recently_recalled: bool,
}

impl Star {
    pub(super) fn matches(&self, query: &str) -> bool {
        !query.is_empty() && self.haystack.contains(query)
    }
}

pub(super) struct Bond {
    pub(super) a: usize,
    pub(super) b: usize,
    pub(super) relation: String,
}

/// A calibrated shared-anchor association between two stars, drawn as a faint
/// thread whose brightness tracks `weight` (0..=1). Distinct from a [`Bond`],
/// which is an explicit, authored `memory_link`.
pub(super) struct Assoc {
    pub(super) a: usize,
    pub(super) b: usize,
    pub(super) weight: f64,
}

pub(super) struct WebData {
    pub(super) stars: Vec<Star>,
    pub(super) bonds: Vec<Bond>,
    pub(super) links: Vec<Assoc>,
}

/// A screen direction for spatial navigation across the constellation.
#[derive(Clone, Copy)]
pub(super) enum Dir {
    Left,
    Right,
    Up,
    Down,
}
