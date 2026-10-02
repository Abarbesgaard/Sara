//! Graph rules for forming clusters: which edges count, splitting oversized
//! components, detecting existing consolidation, shared tags.

pub(super) mod consolidated;
pub(super) mod eligibility;
pub(super) mod split;
pub(super) mod tags;
