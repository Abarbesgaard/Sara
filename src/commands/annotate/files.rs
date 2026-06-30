use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

pub fn attach(
    conn: &Connection,
    id_or_uuid: &str,
    path: &str,
    reason: Option<&str>,
    symbol: Option<&str>,
    lines: Option<&str>,
    source: Option<&str>,
) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    // URLs become navigable/openable links; everything else is a file.
    if db::is_url(path) {
        return super::link(conn, id_or_uuid, path, None);
    }

    // A plain attach with no anchor metadata keeps the simple file-list behavior.
    let is_anchor = reason.is_some() || symbol.is_some() || lines.is_some() || source.is_some();
    if !is_anchor {
        let mut files = db::get_task_files(conn, &task.uuid)?;
        if !files.contains(&path.to_string()) {
            files.push(path.to_string());
        }
        db::set_task_files(conn, &task.uuid, &files)?;
        println!("Attached to task {}: {}", task.id.unwrap_or(0), path);
        return Ok(());
    }

    let (line_start, line_end) = match lines {
        Some(spec) => {
            let mut parts = spec.split([':', '-']);
            let a = parts.next().and_then(|s| s.trim().parse::<i64>().ok());
            let b = parts.next().and_then(|s| s.trim().parse::<i64>().ok());
            (a, b)
        }
        None => (None, None),
    };
    let provenance = match source {
        Some("ai") => db::SOURCE_SUGGESTED,
        _ => db::SOURCE_MANUAL,
    };
    db::add_task_file(
        conn, &task.uuid, path, provenance, reason, symbol, line_start, line_end,
    )?;
    println!("Attached anchor to task {}: {}", task.id.unwrap_or(0), path);
    Ok(())
}
