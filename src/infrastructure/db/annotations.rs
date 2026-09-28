//! Task file associations and annotations/feedback.
//!
//! Split out of the db monolith (issue #168); re-exported by `super`.

use super::*;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use uuid::Uuid;

// ── task files ───────────────────────────────────────────────────────────────

/// Source of a task file: manually attached by the user, or suggested by the LLM.
pub const SOURCE_MANUAL: &str = "manual";
pub const SOURCE_SUGGESTED: &str = "suggested";

/// Replace all files for a task. Every path is stored with `source`.
pub fn set_task_files(conn: &Connection, task_uuid: &Uuid, paths: &[String]) -> Result<()> {
    let sourced: Vec<(String, String)> = paths
        .iter()
        .map(|p| (p.clone(), SOURCE_MANUAL.to_string()))
        .collect();
    set_task_files_sourced(conn, task_uuid, &sourced)
}

/// Replace all files for a task, recording each file's source.
pub fn set_task_files_sourced(
    conn: &Connection,
    task_uuid: &Uuid,
    files: &[(String, String)],
) -> Result<()> {
    let before: std::collections::HashSet<String> = get_task_files(conn, task_uuid)
        .unwrap_or_default()
        .into_iter()
        .collect();

    atomically(conn, "set_task_files", || {
        conn.execute(
            "DELETE FROM task_files WHERE task_uuid=?1",
            [task_uuid.to_string()],
        )?;
        for (path, source) in files {
            conn.execute(
                "INSERT OR IGNORE INTO task_files (task_uuid, path, source) VALUES (?1,?2,?3)",
                params![task_uuid.to_string(), path, source],
            )?;
        }

        // Log each added / removed path so the change shows up in task history.
        let after: std::collections::HashSet<String> =
            files.iter().map(|(p, _)| p.clone()).collect();
        for path in after.difference(&before) {
            record_history(conn, task_uuid, "file", None, Some(path))?;
        }
        for path in before.difference(&after) {
            record_history(conn, task_uuid, "file", Some(path), None)?;
        }
        Ok(())
    })
}
pub fn get_task_files(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM task_files WHERE task_uuid=?1 ORDER BY path")?;
    let paths = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(paths)
}

/// Return `(path, source)` pairs for a task.
pub fn get_task_files_sourced(
    conn: &Connection,
    task_uuid: &Uuid,
) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT path, source FROM task_files WHERE task_uuid=?1 ORDER BY source DESC, path",
    )?;
    let rows = stmt
        .query_map([task_uuid.to_string()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ── annotations ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Annotation {
    pub id: i64,
    pub text: String,
    pub entry: DateTime<Utc>,
    /// comment | finding | thought | constraint | assumption | open_question |
    /// non_goal | decision | risk | pattern
    pub kind: String,
    /// "human" or "ai".
    pub author: String,
    /// What this note anchors to: e.g. "step", "acceptance", "anchor", "note", or NULL (task-level).
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    /// "open" or "resolved".
    pub status: String,
    /// The "reconsider this" flag.
    pub request_revision: bool,
    pub resolved_by_run: Option<i64>,
}

pub const NOTE_KIND_COMMENT: &str = "comment";

fn row_to_annotation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Annotation> {
    let entry_str: String = row.get(2)?;
    Ok(Annotation {
        id: row.get(0)?,
        text: row.get(1)?,
        entry: str_to_dt(&entry_str).unwrap_or_else(|_| Utc::now()),
        kind: row.get(3)?,
        author: row.get(4)?,
        target_kind: row.get(5)?,
        target_id: row.get(6)?,
        status: row.get(7)?,
        request_revision: row.get::<_, i64>(8)? != 0,
        resolved_by_run: row.get(9)?,
    })
}

const ANN_COLUMNS: &str = "id, text, entry, kind, author, target_kind, target_id, status, request_revision, resolved_by_run";

pub fn add_annotation(conn: &Connection, task_uuid: &Uuid, text: &str) -> Result<()> {
    add_annotation_full(
        conn,
        task_uuid,
        text,
        NOTE_KIND_COMMENT,
        "human",
        None,
        None,
        false,
    )
    .map(|_| ())
}

/// Insert a typed, attributed note (or anchored feedback); returns its id.
#[allow(clippy::too_many_arguments)]
pub fn add_annotation_full(
    conn: &Connection,
    task_uuid: &Uuid,
    text: &str,
    kind: &str,
    author: &str,
    target_kind: Option<&str>,
    target_id: Option<&str>,
    request_revision: bool,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO annotations
           (task_uuid, text, entry, kind, author, target_kind, target_id, status, request_revision)
         VALUES (?1,?2,?3,?4,?5,?6,?7,'open',?8)",
        params![
            task_uuid.to_string(),
            text,
            dt_to_str(&Utc::now()),
            kind,
            author,
            target_kind,
            target_id,
            request_revision as i64,
        ],
    )?;
    // Capture the annotation id *before* record_history inserts into task_history,
    // otherwise last_insert_rowid() would return the history row's id instead.
    let id = conn.last_insert_rowid();
    record_history(conn, task_uuid, "annotation", None, Some(text))?;
    Ok(id)
}

pub fn get_annotations(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Annotation>> {
    let sql =
        format!("SELECT {ANN_COLUMNS} FROM annotations WHERE task_uuid=?1 ORDER BY entry ASC");
    let mut stmt = conn.prepare(&sql)?;
    let anns = stmt
        .query_map([task_uuid.to_string()], row_to_annotation)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(anns)
}

/// Open human feedback (comments) for a task, flagged-for-revision first.
pub fn get_open_feedback(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Annotation>> {
    let sql = format!(
        "SELECT {ANN_COLUMNS} FROM annotations
         WHERE task_uuid=?1 AND kind='comment' AND status='open'
         ORDER BY request_revision DESC, entry ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let anns = stmt
        .query_map([task_uuid.to_string()], row_to_annotation)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(anns)
}

/// Mark a piece of feedback resolved, optionally linking the run that addressed it.
pub fn resolve_annotation(conn: &Connection, ann_id: i64, run_id: Option<i64>) -> Result<bool> {
    let n = conn.execute(
        "UPDATE annotations SET status='resolved', resolved_by_run=?2 WHERE id=?1",
        params![ann_id, run_id],
    )?;
    Ok(n > 0)
}

/// Toggle the "reconsider this" flag on a note.
pub fn set_request_revision(conn: &Connection, ann_id: i64, flag: bool) -> Result<bool> {
    let n = conn.execute(
        "UPDATE annotations SET request_revision=?2, status=CASE WHEN ?2=1 THEN 'open' ELSE status END WHERE id=?1",
        params![ann_id, flag as i64],
    )?;
    Ok(n > 0)
}

pub fn delete_annotation(conn: &Connection, ann_id: i64) -> Result<bool> {
    // Capture the text + owning task before deletion so we can log the event.
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT task_uuid, text FROM annotations WHERE id=?1",
            [ann_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .ok();

    let n = conn.execute("DELETE FROM annotations WHERE id=?1", [ann_id])?;
    if n > 0
        && let Some((uuid_str, text)) = existing
        && let Ok(uuid) = Uuid::parse_str(&uuid_str)
    {
        record_history(conn, &uuid, "annotation", Some(&text), None)?;
    }
    Ok(n > 0)
}
