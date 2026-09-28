//! Task guide fields, code anchors, AI-run audit trail, events, FTS search.
//!
//! Split out of the db monolith (issue #168); re-exported by `super` so
//! `db::*` call sites are unchanged. Shared low-level helpers live in the
//! parent `db` module, reached here via `use super::*`.

use super::*;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

pub fn record_event(
    conn: &Connection,
    action: &str,
    ref_uuid: Option<&Uuid>,
    kind: Option<&str>,
    tags: &[String],
    project: Option<&str>,
) -> Result<()> {
    let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO events (action, ref_uuid, kind, tags_json, project, at) VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params![
            action,
            ref_uuid.map(|u| u.to_string()),
            kind,
            tags_json,
            project,
            dt_to_str(&Utc::now()),
        ],
    )?;
    Ok(())
}

pub fn recent_events(
    conn: &Connection,
    limit: i64,
) -> Result<Vec<(String, Option<String>, String)>> {
    let mut stmt = conn.prepare("SELECT action, kind, at FROM events ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn recent_search_queries(conn: &Connection, limit: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT tags_json FROM events WHERE action = 'search' ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        let tags_json: String = r.get(0)?;
        Ok(tags_json)
    })?;
    let mut queries = Vec::new();
    for row in rows {
        let tags_json = row?;
        if let Ok(tags) = serde_json::from_str::<Vec<String>>(&tags_json)
            && let Some(q) = tags.first()
            && !q.is_empty()
        {
            queries.push(q.clone());
        }
    }
    Ok(queries)
}

/// Retention policy: delete events older than `days` days. Called at MCP
/// server startup to bound the table size without requiring a separate job.
/// Errors are non-fatal — a failed prune doesn't affect functionality.
pub fn prune_old_events(conn: &Connection, days: i64) -> Result<usize> {
    let cutoff = (Utc::now() - chrono::Duration::days(days))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let n = conn.execute("DELETE FROM events WHERE at < ?1", [&cutoff])?;
    Ok(n)
}

// ── code anchors (task_files with reason + symbol/lines) ─────────────────────

#[derive(Debug, Clone)]
pub struct Anchor {
    pub path: String,
    pub source: String,
    pub reason: Option<String>,
    pub symbol: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
}

impl Anchor {
    /// Human-friendly location suffix, e.g. " :: enrich_task (10-57)".
    pub fn location(&self) -> String {
        let mut s = String::new();
        if let Some(sym) = &self.symbol {
            s.push_str(" :: ");
            s.push_str(sym);
        }
        match (self.line_start, self.line_end) {
            (Some(a), Some(b)) => s.push_str(&format!(" ({a}-{b})")),
            (Some(a), None) => s.push_str(&format!(" (L{a})")),
            _ => {}
        }
        s
    }
}

pub fn get_task_anchors(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Anchor>> {
    let mut stmt = conn.prepare(
        "SELECT path, source, reason, symbol, line_start, line_end
         FROM task_files WHERE task_uuid=?1 ORDER BY source DESC, path",
    )?;
    let rows = stmt
        .query_map([task_uuid.to_string()], |r| {
            Ok(Anchor {
                path: r.get(0)?,
                source: r.get(1)?,
                reason: r.get(2)?,
                symbol: r.get(3)?,
                line_start: r.get(4)?,
                line_end: r.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Additively attach (or update) one code anchor with provenance + reason.
#[allow(clippy::too_many_arguments)]
pub fn add_task_file(
    conn: &Connection,
    task_uuid: &Uuid,
    path: &str,
    source: &str,
    reason: Option<&str>,
    symbol: Option<&str>,
    line_start: Option<i64>,
    line_end: Option<i64>,
) -> Result<()> {
    let existed: bool = conn
        .query_row(
            "SELECT 1 FROM task_files WHERE task_uuid=?1 AND path=?2",
            params![task_uuid.to_string(), path],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    conn.execute(
        "INSERT INTO task_files (task_uuid, path, source, reason, symbol, line_start, line_end)
         VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(task_uuid, path) DO UPDATE SET
            source=excluded.source, reason=excluded.reason, symbol=excluded.symbol,
            line_start=excluded.line_start, line_end=excluded.line_end",
        params![
            task_uuid.to_string(),
            path,
            source,
            reason,
            symbol,
            line_start,
            line_end
        ],
    )?;
    if !existed {
        record_history(conn, task_uuid, "file", None, Some(path))?;
    }
    Ok(())
}

// ── task-level guide fields (assignment / rationale / freshness / meta) ───────

#[derive(Debug, Clone, Default)]
pub struct TaskGuideFields {
    pub assignment: Option<String>,
    pub rationale: Option<String>,
    pub validated_commit: Option<String>,
    pub validated_at: Option<String>,
    pub meta_json: Option<String>,
}

pub fn get_guide_fields(conn: &Connection, task_uuid: &Uuid) -> Result<TaskGuideFields> {
    conn.query_row(
        "SELECT assignment, rationale, validated_commit, validated_at, meta_json
         FROM tasks WHERE uuid=?1",
        [task_uuid.to_string()],
        |r| {
            Ok(TaskGuideFields {
                assignment: r.get(0)?,
                rationale: r.get(1)?,
                validated_commit: r.get(2)?,
                validated_at: r.get(3)?,
                meta_json: r.get(4)?,
            })
        },
    )
    .map_err(Into::into)
}

pub fn set_assignment(conn: &Connection, task_uuid: &Uuid, text: &str) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET assignment=?2 WHERE uuid=?1",
        params![task_uuid.to_string(), text],
    )?;
    record_history(conn, task_uuid, "assignment", None, Some(text))?;
    Ok(())
}

pub fn set_rationale(conn: &Connection, task_uuid: &Uuid, text: &str) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET rationale=?2 WHERE uuid=?1",
        params![task_uuid.to_string(), text],
    )?;
    record_history(conn, task_uuid, "rationale", None, Some(text))?;
    Ok(())
}

/// Stamp the commit the guide was validated against (freshness guard).
pub fn set_validated(conn: &Connection, task_uuid: &Uuid, commit: &str) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET validated_commit=?2, validated_at=?3 WHERE uuid=?1",
        params![task_uuid.to_string(), commit, dt_to_str(&Utc::now())],
    )?;
    Ok(())
}

pub fn set_meta_json(conn: &Connection, task_uuid: &Uuid, json: &str) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET meta_json=?2 WHERE uuid=?1",
        params![task_uuid.to_string(), json],
    )?;
    Ok(())
}

/// Mark a task active (start its timer) if it is not already running. Sets
/// `started_at`/`modified` to now and returns `true` when the task transitioned
/// from idle to active, `false` if it was already active. Used to auto-transition
/// a task to "in progress" the first time work is recorded against it (a
/// step marked done or a verification run), so status reflects reality without
/// the caller remembering a separate `sara start`.
pub fn ensure_started(conn: &Connection, task_uuid: &Uuid) -> Result<bool> {
    let now = dt_to_str(&Utc::now());
    let changed = conn.execute(
        "UPDATE tasks SET started_at=?2, modified=?2 WHERE uuid=?1 AND started_at IS NULL",
        params![task_uuid.to_string(), now],
    )?;
    Ok(changed > 0)
}

// ── AI run audit trail ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AiRun {
    pub id: i64,
    pub kind: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub created_at: DateTime<Utc>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

/// Record one LLM interaction against a task; returns the run id.
pub fn record_ai_run(
    conn: &Connection,
    task_uuid: &Uuid,
    kind: &str,
    model: Option<&str>,
    provider: Option<&str>,
    prompt: Option<&str>,
    response_json: Option<&str>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO task_ai_runs (task_uuid, kind, model, provider, prompt, response_json, created_at, prompt_tokens, completion_tokens, total_tokens)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            task_uuid.to_string(),
            kind,
            model,
            provider,
            prompt,
            response_json,
            dt_to_str(&Utc::now()),
            prompt_tokens,
            completion_tokens,
            total_tokens,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_ai_runs(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<AiRun>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, model, provider, created_at, prompt_tokens, completion_tokens, total_tokens FROM task_ai_runs
         WHERE task_uuid=?1 ORDER BY created_at ASC, id ASC",
    )?;
    let rows = stmt
        .query_map([task_uuid.to_string()], |r| {
            let at: String = r.get(4)?;
            Ok(AiRun {
                id: r.get(0)?,
                kind: r.get(1)?,
                model: r.get(2)?,
                provider: r.get(3)?,
                created_at: str_to_dt(&at).unwrap_or_else(|_| Utc::now()),
                prompt_tokens: r.get(5)?,
                completion_tokens: r.get(6)?,
                total_tokens: r.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ── full guide JSON (single-query, via the task_guide view) ───────────────────

/// Assemble the entire guide for a task as a JSON value in one query.
pub fn guide_json(conn: &Connection, task_uuid: &Uuid) -> Result<serde_json::Value> {
    let raw: String = conn.query_row(
        "SELECT guide_json FROM task_guide WHERE uuid=?1",
        [task_uuid.to_string()],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null))
}

// ── cross-task memory (FTS5 keyword search) ──────────────────────────────────

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub ref_kind: String,
    pub task_uuid: String,
    pub text: String,
}

/// Keyword search across tasks/notes/anchors via the FTS5 index.
pub fn search_fts(conn: &Connection, query: &str, limit: i64) -> Result<Vec<SearchHit>> {
    // Quote the query as an FTS5 string literal to tolerate arbitrary input.
    let fts_query = format!("\"{}\"", query.replace('"', "\"\""));
    let mut stmt = conn.prepare(
        "SELECT ref_kind, task_uuid, text FROM search_index
         WHERE search_index MATCH ?1 ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![fts_query, limit], |r| {
            Ok(SearchHit {
                ref_kind: r.get(0)?,
                task_uuid: r.get(1)?,
                text: r.get(2)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Token-based AND search: each token is quoted individually and joined with
/// spaces (FTS5 AND semantics). Unlike `search_fts`, this tolerates different
/// word ordering and paraphrases — any row containing ALL the given tokens
/// (in any order) is returned. Callers should strip stop words and cap the
/// token list before calling to avoid over-constraining the query.
pub fn search_fts_tokens(
    conn: &Connection,
    tokens: &[String],
    limit: i64,
) -> Result<Vec<SearchHit>> {
    if tokens.is_empty() {
        return Ok(vec![]);
    }
    // Each token quoted as an FTS5 string literal; space-join = AND.
    let fts_query = tokens
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ");
    let mut stmt = conn.prepare(
        "SELECT ref_kind, task_uuid, text FROM search_index
         WHERE search_index MATCH ?1 ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![fts_query, limit], |r| {
            Ok(SearchHit {
                ref_kind: r.get(0)?,
                task_uuid: r.get(1)?,
                text: r.get(2)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Token-based OR search: like [`search_fts_tokens`] but tokens are joined with
/// the FTS5 `OR` operator, so a row matching ANY meaningful token surfaces.
/// Used as the loose Tier-3 fallback in recall — only when the phrase and the
/// token-AND search both miss — so partial-vocabulary / paraphrased queries
/// still return candidates instead of nothing. `ORDER BY rank` (bm25) floats
/// the higher-coverage hits (more / rarer matching tokens) to the top.
pub fn search_fts_tokens_or(
    conn: &Connection,
    tokens: &[String],
    limit: i64,
) -> Result<Vec<SearchHit>> {
    if tokens.is_empty() {
        return Ok(vec![]);
    }
    // Each token quoted as an FTS5 string literal; joined with OR.
    let fts_query = tokens
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut stmt = conn.prepare(
        "SELECT ref_kind, task_uuid, text FROM search_index
         WHERE search_index MATCH ?1 ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![fts_query, limit], |r| {
            Ok(SearchHit {
                ref_kind: r.get(0)?,
                task_uuid: r.get(1)?,
                text: r.get(2)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}
