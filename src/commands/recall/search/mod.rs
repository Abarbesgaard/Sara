//! Finding hits: filters, full-text and semantic search, ranking.

pub(super) mod collect;
pub(super) mod filters;
pub(super) mod fts;
pub(super) mod query;
pub(super) mod scoring;
pub(super) mod semantic;

#[cfg(test)]
use super::types::Hit;
