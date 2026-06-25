use anyhow::Result;
use rusqlite::Connection;

use crate::db;

/// Parse an `--on` reference (`step:N`, `acceptance:N`, `anchor:ID`, `note:ID`)
/// into a stable (target_kind, target_id) pair, resolving step/acceptance
/// indices to their database ids.
fn parse_on_ref(conn: &Connection, task_uuid: &uuid::Uuid, on: &str) -> Result<(String, String)> {
    let (kind, rest) = on.split_once(':').ok_or_else(|| {
        anyhow::anyhow!("--on must look like step:2, acceptance:1, anchor:ID, or note:ID")
    })?;
    match kind {
        "step" | "acceptance" => {
            let n: usize = rest.parse().context_invalid()?;
            let step_kind = if kind == "step" {
                db::STEP_KIND_STEP
            } else {
                db::STEP_KIND_ACCEPTANCE
            };
            let step_id = db::step_id_by_index(conn, task_uuid, step_kind, n)?;
            Ok((kind.to_string(), step_id.to_string()))
        }
        "anchor" | "note" => Ok((kind.to_string(), rest.to_string())),
        other => anyhow::bail!("unknown --on target kind: {other}"),
    }
}

trait ParseCtx<T> {
    fn context_invalid(self) -> Result<T>;
}
impl<T> ParseCtx<T> for std::result::Result<T, std::num::ParseIntError> {
    fn context_invalid(self) -> Result<T> {
        self.map_err(|_| anyhow::anyhow!("--on index must be a number"))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn annotate(
    conn: &Connection,
    id_or_uuid: &str,
    words: &[String],
    kind: Option<&str>,
    author: Option<&str>,
    on: Option<&str>,
    reconsider: bool,
) -> Result<()> {
    let text = words
        .iter()
        .filter(|w| !w.starts_with("--"))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    if text.trim().is_empty() {
        anyhow::bail!("Annotation text cannot be empty");
    }
    let task = db::resolve_task(conn, id_or_uuid)?;

    let (target_kind, target_id) = match on {
        Some(r) => {
            let (k, v) = parse_on_ref(conn, &task.uuid, r)?;
            (Some(k), Some(v))
        }
        None => (None, None),
    };

    db::add_annotation_full(
        conn,
        &task.uuid,
        text.trim(),
        kind.unwrap_or(db::NOTE_KIND_COMMENT),
        author.unwrap_or("human"),
        target_kind.as_deref(),
        target_id.as_deref(),
        reconsider,
    )?;
    println!("Annotated task {}: {}", task.id.unwrap_or(0), text.trim());
    Ok(())
}

pub fn denotate(conn: &Connection, annotation_id: i64) -> Result<()> {
    if db::delete_annotation(conn, annotation_id)? {
        println!("Removed annotation {annotation_id}.");
    } else {
        anyhow::bail!("No annotation with id {annotation_id}");
    }
    Ok(())
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
    let task = db::resolve_task(conn, id_or_uuid)?;
    // URLs become navigable/openable links; everything else is a file.
    if db::is_url(path) {
        return link(conn, id_or_uuid, path, None);
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

pub fn link(conn: &Connection, id_or_uuid: &str, url: &str, label: Option<&str>) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    db::add_link(conn, &task.uuid, url, label)?;
    println!("Linked task {}: {}", task.id.unwrap_or(0), url);
    Ok(())
}

pub fn unlink(conn: &Connection, link_id: i64) -> Result<()> {
    if db::delete_link(conn, link_id)? {
        println!("Removed link {link_id}.");
    } else {
        anyhow::bail!("No link with id {link_id}");
    }
    Ok(())
}
