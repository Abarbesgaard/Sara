use serde::{Deserialize, Serialize};

use crate::domain::error::DomainError;

/// How one memory relates to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Supersedes,
    SimilarTo,
    DerivedFrom,
    UsedIn,
    CoActivated,
}

impl Relation {
    pub const ALL: [Relation; 5] = [
        Self::Supersedes,
        Self::SimilarTo,
        Self::DerivedFrom,
        Self::UsedIn,
        Self::CoActivated,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supersedes => "supersedes",
            Self::SimilarTo => "similar_to",
            Self::DerivedFrom => "derived_from",
            Self::UsedIn => "used_in",
            Self::CoActivated => "co_activated",
        }
    }

    /// Hierarchical relations must not form cycles (A supersedes B supersedes A).
    pub fn forbids_cycles(self) -> bool {
        matches!(self, Self::Supersedes | Self::DerivedFrom)
    }
}

impl std::fmt::Display for Relation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Relation {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|r| r.as_str() == s)
            .ok_or_else(|| DomainError::UnknownRelation(s.to_string()))
    }
}
