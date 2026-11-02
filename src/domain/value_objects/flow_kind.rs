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
