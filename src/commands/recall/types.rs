use chrono::{DateTime, Utc};

use crate::infrastructure::memory::fingerprint::AnchorState;
use crate::infrastructure::model::{Item, Task};

pub(super) struct Hit {
    pub(super) ref_kind: String,
    pub(super) label: String,
    pub(super) description: String,
    pub(super) snippet: String,
    pub(super) body: String,
    pub(super) strength: f64,
    pub(super) exact_match: bool,
    pub(super) loose: bool,
    pub(super) fts_rank: Option<usize>,
    pub(super) modified: Option<DateTime<Utc>>,
    pub(super) files: Vec<String>,
    pub(super) linked_tasks: Vec<(Task, String)>,
    pub(super) superseded_by: Vec<String>,
    pub(super) provisional: bool,
    pub(super) item_uuid: Option<uuid::Uuid>,
    pub(super) derived_from_labels: Vec<String>,
    pub(super) derived_children: Vec<String>,
    pub(super) semantic: bool,
    pub(super) cosine: Option<f32>,
    pub(super) cluster: Option<ClusterInfo>,
    pub(super) stale: Vec<(String, AnchorState)>,
}

#[derive(Clone, Debug)]
pub(super) struct ClusterInfo {
    pub(super) canonical_label: String,
    pub(super) size: usize,
    pub(super) collapsed_here: usize,
    pub(super) nearest: Option<String>,
}

pub(super) struct Related {
    pub(super) item: Item,
    pub(super) activation: f64,
    pub(super) path: Vec<String>,
}
