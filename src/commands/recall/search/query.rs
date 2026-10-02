use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::model::Item;

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
