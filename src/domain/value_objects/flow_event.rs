use chrono::{DateTime, Utc};

use crate::domain::value_objects::FlowKind;

#[derive(Debug, Clone, PartialEq)]
pub struct FlowEvent {
    pub at: DateTime<Utc>,
    pub kind: FlowKind,
    pub text: String,
    pub detail: Option<String>,
}
