use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowKind {
    Doing,
    StepDone,
    StepAdded,
    Note(String),
    Memory(String),
    Change(String),
    AiRun,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlowEvent {
    pub at: DateTime<Utc>,
    pub kind: FlowKind,
    pub text: String,
    pub detail: Option<String>,
}
