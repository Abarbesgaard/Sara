use std::collections::HashSet;

use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use crate::infrastructure::db;

pub(in crate::commands::learn) fn file_overlaps(
    conn: &Connection,
    files: &[String],
    exclude: &HashSet<Uuid>,
) -> Result<Vec<(String, Uuid)>> {
    let mut out: Vec<(String, Uuid)> = Vec::new();
    for path in files {
        for item in db::find_items_by_file(conn, path, false)? {
            if item.kind == "memory"
                && !out.iter().any(|(_, u)| *u == item.uuid)
                && !exclude.contains(&item.uuid)
            {
                out.push((path.clone(), item.uuid));
            }
        }
    }
    Ok(out)
}
