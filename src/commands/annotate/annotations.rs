use anyhow::Result;
use rusqlite::Connection;

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
    db::add_annotation_full(
        conn,
        &task.uuid,
        text.trim(),
        note_kind,
        author.unwrap_or("human"),
        target_kind.as_deref(),
        target_id.as_deref(),
        reconsider,
    )?;
    Ok(serde_json::json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": note_kind,
        "text": text.trim(),
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
