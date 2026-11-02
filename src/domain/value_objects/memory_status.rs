use serde::{Deserialize, Serialize};

use crate::domain::error::DomainError;

/// Lifecycle of a memory. New memories start `Active`; reviews may demote one to
/// `Provisional`, and pruning or forgetting moves it to `Archived`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryStatus {
    Active,
    Provisional,
    Archived,
}

impl MemoryStatus {
    pub const ALL: [MemoryStatus; 3] = [Self::Active, Self::Provisional, Self::Archived];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Provisional => "provisional",
            Self::Archived => "archived",
        }
    }

    /// Live memories take part in recall; archived ones do not.
    pub fn is_live(self) -> bool {
        self != Self::Archived
    }
}

impl std::fmt::Display for MemoryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for MemoryStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| DomainError::UnknownMemoryStatus(s.to_string()))
    }
}
