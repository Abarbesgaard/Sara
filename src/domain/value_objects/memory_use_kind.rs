/// How a memory took part in a task. Outcome credit is derived from these at
/// read time, joined against the task's status and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryUseKind {
    Surfaced,
    Recalled,
    Cited,
}

impl MemoryUseKind {
    pub const ALL: [MemoryUseKind; 3] = [Self::Surfaced, Self::Recalled, Self::Cited];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Surfaced => "surfaced",
            Self::Recalled => "recalled",
            Self::Cited => "cited",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}
