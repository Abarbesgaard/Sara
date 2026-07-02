use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

/// Attach a file / code anchor (or a URL, which becomes a link) to a task,
/// returning a structured record. Print-free core shared by the CLI `attach`
/// command and the MCP `attach` tool.
pub fn attach_value(
    conn: &Connection,
    id_or_uuid: &str,
    path: &str,
    reason: Option<&str>,
    symbol: Option<&str>,
    lines: Option<&str>,
    source: Option<&str>,
) -> Result<Value> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    // URLs become navigable/openable links; everything else is a file. Tag the
    // link result with `kind` so every `attach` shape carries one (file/anchor/link).
    if db::is_url(path) {
        let mut v = super::link_value(conn, id_or_uuid, path, None)?;
        if let Some(obj) = v.as_object_mut() {
            obj.insert("kind".to_string(), json!("link"));
        }
        return Ok(v);
    }

    // A plain attach with no anchor metadata keeps the simple file-list behavior.
    let is_anchor = reason.is_some() || symbol.is_some() || lines.is_some() || source.is_some();
    if !is_anchor {
        let mut files = db::get_task_files(conn, &task.uuid)?;
        if !files.contains(&path.to_string()) {
            files.push(path.to_string());
        }
        db::set_task_files(conn, &task.uuid, &files)?;
        return Ok(json!({
            "task": task.id,
            "uuid": task.uuid.to_string(),
            "attached": path,
            "kind": "file",
        }));
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
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "attached": path,
        "kind": "anchor",
        "reason": reason,
        "symbol": symbol,
        "line_start": line_start,
        "line_end": line_end,
    }))
}

pub fn attach(
    conn: &Connection,
    id_or_uuid: &str,
    path: &str,
    reason: Option<&str>,
    symbol: Option<&str>,
    lines: Option<&str>,
    source: Option<&str>,
) -> Result<()> {
    let v = attach_value(conn, id_or_uuid, path, reason, symbol, lines, source)?;
    let id = v["task"].as_i64().unwrap_or(0);
    if let Some(url) = v.get("url").and_then(|u| u.as_str()) {
        // Delegated to link_value (URL path).
        println!("Linked task {id}: {url}");
    } else if v["kind"] == "anchor" {
        println!(
            "Attached anchor to task {id}: {}",
            v["attached"].as_str().unwrap_or_default()
        );
    } else {
        println!(
            "Attached to task {id}: {}",
            v["attached"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}
