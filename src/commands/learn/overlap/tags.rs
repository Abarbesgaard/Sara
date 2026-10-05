use std::collections::HashSet;

use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use super::hints::canonical_derived_count;
use crate::commands::shared::normalize_list;
use crate::infrastructure::db;

const PARTIAL_SHARE: f64 = 0.5;

pub(in crate::commands::learn) struct TagOverlap {
    pub(super) tags: Vec<String>,
    pub(super) near_dupes: HashSet<Uuid>,
    pub(super) partial: Vec<Uuid>,
}

impl TagOverlap {
    pub(in crate::commands::learn) fn compute(conn: &Connection, tags: &[String]) -> Result<Self> {
        let tags: Vec<String> = normalize_list(tags)
            .iter()
            .map(|t| t.to_lowercase())
            .collect();

        let mut any_union: HashSet<Uuid> = HashSet::new();
        let mut all_intersection: Option<HashSet<Uuid>> = None;
        let mut per_tag_sets: Vec<HashSet<Uuid>> = Vec::new();
        for tag in &tags {
            let uuids: HashSet<Uuid> = db::find_items_by_tag(conn, tag)?
                .into_iter()
                .filter(|i| i.kind == "memory")
                .map(|i| i.uuid)
                .collect();
            any_union.extend(&uuids);
            all_intersection = Some(match all_intersection {
                Some(existing) => existing.intersection(&uuids).copied().collect(),
                None => uuids.clone(),
            });
            per_tag_sets.push(uuids);
        }

        let near_dupes = all_intersection.unwrap_or_default();
        let threshold = ((tags.len() as f64) * PARTIAL_SHARE).ceil() as usize;
        let partial: Vec<Uuid> = if tags.len() > 1 {
            any_union
                .difference(&near_dupes)
                .copied()
                .filter(|u| per_tag_sets.iter().filter(|set| set.contains(u)).count() >= threshold)
                .collect()
        } else {
            vec![]
        };

        Ok(Self {
            tags,
            near_dupes,
            partial,
        })
    }

    pub(in crate::commands::learn) fn canonical_candidates(&self, conn: &Connection) -> Vec<Uuid> {
        let mut out: Vec<Uuid> = Vec::new();
        for u in self.near_dupes.iter().chain(&self.partial) {
            if !out.contains(u) && canonical_derived_count(conn, u) > 0 {
                out.push(*u);
            }
        }
        out
    }
}
