use std::fmt;

/// A domain rule was broken. Callers may add context (flags, config hints) on top.
#[derive(Debug, Clone, PartialEq)]
pub enum DomainError {
    MemoryBodyTooLarge { len: usize, limit: usize },
    MemoryBodyContainsSecret { reason: &'static str },
    UnknownMemoryStatus(String),
    UnknownRelation(String),
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DomainError::MemoryBodyTooLarge { len, limit } => {
                write!(f, "Memory body is {len} characters ({limit} char limit)")
            }
            DomainError::MemoryBodyContainsSecret { reason } => {
                write!(f, "Memory body may contain a secret ({reason})")
            }
            DomainError::UnknownMemoryStatus(s) => write!(f, "Unknown memory status '{s}'"),
            DomainError::UnknownRelation(s) => write!(f, "Unknown relation '{s}'"),
        }
    }
}

impl std::error::Error for DomainError {}
