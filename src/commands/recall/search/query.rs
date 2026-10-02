use rusqlite::Connection;

use crate::commands::shared::{normalize_list, resolve_files};
use crate::infrastructure::db;
use crate::infrastructure::model::Item;

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
            tags: normalize_list(tags),
            projects: normalize_list(projects),
            files: resolve_files(&normalize_list(files)),
        }
    }

    pub fn is_recent(&self) -> bool {
        self.query.is_empty()
            && self.tags.is_empty()
            && self.projects.is_empty()
            && self.files.is_empty()
    }
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
