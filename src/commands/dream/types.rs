use crate::infrastructure::model::Item;

#[derive(Clone)]
pub(super) enum NodeKind {
    Memory {
        label: String,
        relation: String,
    },
    Task {
        id: String,
        desc: String,
        source: String,
    },
    File {
        name: String,
    },
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
    pub(super) haystack: String,
    pub(super) x: f64,
    pub(super) y: f64,
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

#[derive(Clone, Copy)]
pub(super) enum Dir {
    Left,
    Right,
    Up,
    Down,
}
