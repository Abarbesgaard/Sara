use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::model::Item;
use crate::infrastructure::project;

pub(in crate::commands::recall) struct RecallInput<'a> {
    pub query: &'a str,
    pub tags: Vec<String>,
    pub projects: Vec<String>,
    pub files: Vec<String>,
}

impl<'a> RecallInput<'a> {
    pub fn new(query: &'a str, tags: &[String], projects: &[String], files: &[String]) -> Self {
        Self {
            query: query.trim(),
            tags: normalize(tags),
            projects: normalize(projects),
            files: normalize(files)
                .iter()
                .map(|p| project::resolve_file_link_here(p))
                .collect(),
        }
    }

    pub fn is_recent(&self) -> bool {
        self.query.is_empty()
            && self.tags.is_empty()
            && self.projects.is_empty()
            && self.files.is_empty()
    }
}

pub(in crate::commands::recall) fn normalize(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

pub(in crate::commands::recall) fn resolve_label_query(
    conn: &Connection,
    query: &str,
) -> Option<Item> {
    let q = query.trim();
    let is_memory_handle = q.len() >= 2
        && (q.starts_with('m') || q.starts_with('M'))
        && q[1..].chars().all(|c| c.is_ascii_digit());
    if !is_memory_handle {
        return None;
    }
    db::get_item_by_handle(conn, q).ok()
}
