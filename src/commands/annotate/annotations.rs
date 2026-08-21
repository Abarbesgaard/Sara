use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

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

/// Add an annotation and return a structured record. Shared by the CLI
/// `annotate`/`comment` command and the MCP `annotate` tool (which cannot print).
#[allow(clippy::too_many_arguments)]
pub fn annotate_value(
    conn: &Connection,
    id_or_uuid: &str,
    words: &[String],
    kind: Option<&str>,
    author: Option<&str>,
    on: Option<&str>,
    reconsider: bool,
) -> Result<serde_json::Value> {
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

    let note_kind = kind.unwrap_or(db::NOTE_KIND_COMMENT);
    let ann_id = db::add_annotation_full(
        conn,
        &task.uuid,
        text.trim(),
        note_kind,
        author.unwrap_or("human"),
        target_kind.as_deref(),
        target_id.as_deref(),
        reconsider,
    )?;

    // When recording a finding, resurface the task's own prior findings that are
    // semantically close — the reconsider guard against silently contradicting
    // an earlier conclusion.
    let related = if note_kind == "finding" {
        crate::commands::insight::related_findings(conn, &task.uuid, text.trim(), Some(ann_id))
    } else {
        Vec::new()
    };

    Ok(serde_json::json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": note_kind,
        "text": text.trim(),
        "related_findings": crate::commands::insight::related_findings_json(&related),
    }))
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
    let v = annotate_value(conn, id_or_uuid, words, kind, author, on, reconsider)?;
    println!(
        "Annotated task {}: {}",
        v.get("task").and_then(|t| t.as_i64()).unwrap_or(0),
        v.get("text").and_then(|t| t.as_str()).unwrap_or("")
    );
    if let Some(related) = v.get("related_findings").and_then(|r| r.as_array())
        && !related.is_empty()
    {
        eprintln!("⟳ reconsider — related prior finding(s) on this task:");
        for r in related {
            eprintln!(
                "    (~{:.2}) #{}: {}",
                r.get("cosine").and_then(|c| c.as_f64()).unwrap_or(0.0),
                r.get("annotation_id").and_then(|i| i.as_i64()).unwrap_or(0),
                r.get("text").and_then(|t| t.as_str()).unwrap_or("")
            );
        }
        eprintln!(
            "  If your new note revises or contradicts one, correct it (denotate / re-annotate)."
        );
    }
    Ok(())
}

pub fn denotate_value(conn: &Connection, annotation_id: i64) -> Result<Value> {
    if db::delete_annotation(conn, annotation_id)? {
        Ok(json!({ "annotation_id": annotation_id, "removed": true }))
    } else {
        anyhow::bail!("No annotation with id {annotation_id}")
    }
}

pub fn denotate(conn: &Connection, annotation_id: i64) -> Result<()> {
    denotate_value(conn, annotation_id)?;
    println!("Removed annotation {annotation_id}.");
    Ok(())
}
