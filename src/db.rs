use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use rusqlite_migration::{M, Migrations};

use crate::config;
use crate::model::{Item, Priority, Project, Status, Task};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub fn open() -> Result<Connection> {
    let path = config::db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(&path)
        .with_context(|| format!("Failed to open database: {}", path.display()))?;

    // PRAGMAs must be set outside migrations
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=5000;",
    )?;

    apply_migrations(&mut conn)?;
    Ok(conn)
}

fn apply_migrations(conn: &mut Connection) -> Result<()> {
    let migrations = Migrations::new(vec![
        M::up(
            "CREATE TABLE IF NOT EXISTS projects (
                name           TEXT PRIMARY KEY,
                path           TEXT,
                goal           TEXT,
                stack          TEXT,
                conventions    TEXT,
                notes          TEXT,
                initialized_at TEXT,
                last_seen      TEXT
            );
            CREATE TABLE IF NOT EXISTS tasks (
                rowid       INTEGER PRIMARY KEY AUTOINCREMENT,
                uuid        TEXT NOT NULL UNIQUE,
                id          INTEGER,
                description TEXT NOT NULL,
                project     TEXT NOT NULL DEFAULT 'inbox',
                status      TEXT NOT NULL DEFAULT 'pending',
                priority    TEXT,
                due         TEXT,
                entry       TEXT NOT NULL,
                modified    TEXT NOT NULL,
                end         TEXT,
                tags_json   TEXT NOT NULL DEFAULT '[]',
                urgency     REAL NOT NULL DEFAULT 0.0
            );
            CREATE TABLE IF NOT EXISTS dependencies (
                task_uuid       TEXT NOT NULL,
                depends_on_uuid TEXT NOT NULL,
                PRIMARY KEY (task_uuid, depends_on_uuid),
                FOREIGN KEY (task_uuid)       REFERENCES tasks(uuid) ON DELETE CASCADE,
                FOREIGN KEY (depends_on_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS task_files (
                task_uuid TEXT NOT NULL,
                path      TEXT NOT NULL,
                PRIMARY KEY (task_uuid, path),
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );",
        ),
        M::up(
            "ALTER TABLE tasks ADD COLUMN started_at TEXT;
             ALTER TABLE tasks ADD COLUMN time_spent INTEGER NOT NULL DEFAULT 0;",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS annotations (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid TEXT NOT NULL,
                text      TEXT NOT NULL,
                entry     TEXT NOT NULL,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );",
        ),
        M::up(
            // Track whether a file was attached by the user ('manual') or
            // proposed by the LLM ('suggested').
            "ALTER TABLE task_files ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS task_history (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid  TEXT NOT NULL,
                field      TEXT NOT NULL,
                old_value  TEXT,
                new_value  TEXT,
                changed_at TEXT NOT NULL,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_task_history_task
                ON task_history(task_uuid, changed_at);",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS task_links (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid TEXT NOT NULL,
                url       TEXT NOT NULL,
                label     TEXT,
                entry     TEXT NOT NULL,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );",
        ),
        M::up(
            // Records full task snapshots so the most recent command can be reverted.
            // before_json is NULL when the task was newly created (undo = remove it);
            // rows from a single CLI invocation share a batch_id.
            "CREATE TABLE IF NOT EXISTS undo_log (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                batch_id    TEXT NOT NULL,
                command     TEXT NOT NULL,
                task_uuid   TEXT NOT NULL,
                before_json TEXT,
                after_json  TEXT,
                created_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_undo_log_batch ON undo_log(batch_id);",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS task_branches (
                task_uuid          TEXT PRIMARY KEY,
                branch             TEXT NOT NULL,
                base               TEXT,
                changed_files_json TEXT,
                logged_at          TEXT,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );",
        ),
        M::up(
            "ALTER TABLE tasks ADD COLUMN estimate_mins INTEGER;
             CREATE TABLE IF NOT EXISTS task_checklist (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid TEXT NOT NULL,
                text      TEXT NOT NULL,
                done      INTEGER NOT NULL DEFAULT 0,
                position  INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
             );",
        ),
        M::up(
            // recur: a recurrence interval string like "daily", "weekly", "2w", "1m", etc.
            // NULL means the task does not recur.
            "ALTER TABLE tasks ADD COLUMN recur TEXT;",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS items (
                uuid        TEXT PRIMARY KEY,
                kind        TEXT NOT NULL,
                display_id  INTEGER,
                title       TEXT NOT NULL,
                url         TEXT,
                project     TEXT,
                tags_json   TEXT NOT NULL DEFAULT '[]',
                path        TEXT NOT NULL,
                summary     TEXT,
                body        TEXT NOT NULL DEFAULT '',
                created     TEXT NOT NULL,
                modified    TEXT NOT NULL,
                status      TEXT NOT NULL DEFAULT 'active'
            );
            CREATE TABLE IF NOT EXISTS events (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                action      TEXT NOT NULL,
                ref_uuid    TEXT,
                kind        TEXT,
                tags_json   TEXT,
                project     TEXT,
                at          TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS embeddings (
                ref_uuid    TEXT PRIMARY KEY,
                vector_json TEXT NOT NULL
            );",
        ),
        M::up(
            // AI-first guide expansion: each task becomes a self-contained,
            // LLM-authored implementation guide with provenance + freshness.
            "ALTER TABLE tasks ADD COLUMN assignment TEXT;
             ALTER TABLE tasks ADD COLUMN rationale TEXT;
             ALTER TABLE tasks ADD COLUMN validated_commit TEXT;
             ALTER TABLE tasks ADD COLUMN validated_at TEXT;
             ALTER TABLE tasks ADD COLUMN meta_json TEXT;

             ALTER TABLE task_checklist ADD COLUMN intent TEXT;
             ALTER TABLE task_checklist ADD COLUMN source TEXT NOT NULL DEFAULT 'human';
             ALTER TABLE task_checklist ADD COLUMN kind TEXT NOT NULL DEFAULT 'step';
             ALTER TABLE task_checklist ADD COLUMN verify_cmd TEXT;
             ALTER TABLE task_checklist ADD COLUMN result TEXT;
             ALTER TABLE task_checklist ADD COLUMN done_commit TEXT;
             ALTER TABLE task_checklist ADD COLUMN done_at TEXT;

             ALTER TABLE annotations ADD COLUMN kind TEXT NOT NULL DEFAULT 'comment';
             ALTER TABLE annotations ADD COLUMN author TEXT NOT NULL DEFAULT 'human';
             ALTER TABLE annotations ADD COLUMN target_kind TEXT;
             ALTER TABLE annotations ADD COLUMN target_id TEXT;
             ALTER TABLE annotations ADD COLUMN status TEXT NOT NULL DEFAULT 'open';
             ALTER TABLE annotations ADD COLUMN request_revision INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE annotations ADD COLUMN resolved_by_run INTEGER;

             ALTER TABLE task_files ADD COLUMN reason TEXT;
             ALTER TABLE task_files ADD COLUMN symbol TEXT;
             ALTER TABLE task_files ADD COLUMN line_start INTEGER;
             ALTER TABLE task_files ADD COLUMN line_end INTEGER;

             ALTER TABLE projects ADD COLUMN setup_cmd TEXT;
             ALTER TABLE projects ADD COLUMN test_cmd TEXT;
             ALTER TABLE projects ADD COLUMN lint_cmd TEXT;
             ALTER TABLE projects ADD COLUMN run_cmd TEXT;

             CREATE TABLE IF NOT EXISTS task_ai_runs (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid     TEXT NOT NULL,
                kind          TEXT NOT NULL,
                model         TEXT,
                provider      TEXT,
                prompt        TEXT,
                response_json TEXT,
                created_at    TEXT NOT NULL,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_task_ai_runs_task
                ON task_ai_runs(task_uuid, created_at);",
        ),
        M::up(
            // Non-secret GitHub sync identity fields for projects, plus a
            // stable index to let project detection look up repos by full_name.
            "ALTER TABLE projects ADD COLUMN github_repo        TEXT;
             ALTER TABLE projects ADD COLUMN github_login       TEXT;
             ALTER TABLE projects ADD COLUMN github_sync_scope  TEXT;
             CREATE INDEX IF NOT EXISTS idx_projects_github_repo
               ON projects(github_repo) WHERE github_repo IS NOT NULL;",
        ),
        M::up(
            // Lean on the SQLite engine: an FTS5 index for cross-task memory
            // kept in sync by triggers, and a JSON-producing view that
            // assembles the whole guide in one query (json1 is bundled).
            "CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
                ref_kind UNINDEXED, ref_id UNINDEXED, task_uuid UNINDEXED, text
             );

             CREATE TRIGGER IF NOT EXISTS trg_tasks_ai AFTER INSERT ON tasks BEGIN
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('task', new.uuid, new.uuid,
                    coalesce(new.description,'')||' '||coalesce(new.rationale,'')||' '||coalesce(new.assignment,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_tasks_au AFTER UPDATE ON tasks BEGIN
                DELETE FROM search_index WHERE ref_kind='task' AND ref_id=old.uuid;
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('task', new.uuid, new.uuid,
                    coalesce(new.description,'')||' '||coalesce(new.rationale,'')||' '||coalesce(new.assignment,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_tasks_ad AFTER DELETE ON tasks BEGIN
                DELETE FROM search_index WHERE ref_kind='task' AND ref_id=old.uuid;
             END;

             CREATE TRIGGER IF NOT EXISTS trg_ann_ai AFTER INSERT ON annotations BEGIN
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('note', new.id, new.task_uuid, coalesce(new.text,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_ann_au AFTER UPDATE ON annotations BEGIN
                DELETE FROM search_index WHERE ref_kind='note' AND ref_id=old.id;
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('note', new.id, new.task_uuid, coalesce(new.text,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_ann_ad AFTER DELETE ON annotations BEGIN
                DELETE FROM search_index WHERE ref_kind='note' AND ref_id=old.id;
             END;

             CREATE TRIGGER IF NOT EXISTS trg_files_ai AFTER INSERT ON task_files BEGIN
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('anchor', new.rowid, new.task_uuid,
                    coalesce(new.path,'')||' '||coalesce(new.reason,'')||' '||coalesce(new.symbol,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_files_ad AFTER DELETE ON task_files BEGIN
                DELETE FROM search_index WHERE ref_kind='anchor' AND ref_id=old.rowid;
             END;

             CREATE VIEW IF NOT EXISTS task_guide AS
             SELECT t.uuid AS uuid, json_object(
                'uuid', t.uuid,
                'id', t.id,
                'description', t.description,
                'project', t.project,
                'status', t.status,
                'priority', t.priority,
                'due', t.due,
                'entry', t.entry,
                'modified', t.modified,
                'tags', json(t.tags_json),
                'urgency', t.urgency,
                'assignment', t.assignment,
                'rationale', t.rationale,
                'validated_commit', t.validated_commit,
                'validated_at', t.validated_at,
                'meta', CASE WHEN t.meta_json IS NOT NULL AND t.meta_json != '' THEN json(t.meta_json) ELSE NULL END,
                'steps', (SELECT json_group_array(json_object(
                        'id', c.id, 'position', c.position, 'text', c.text, 'intent', c.intent,
                        'done', c.done, 'kind', c.kind, 'source', c.source, 'verify_cmd', c.verify_cmd,
                        'result', c.result, 'done_commit', c.done_commit, 'done_at', c.done_at))
                    FROM task_checklist c WHERE c.task_uuid = t.uuid),
                'files', (SELECT json_group_array(json_object(
                        'path', f.path, 'source', f.source, 'reason', f.reason,
                        'symbol', f.symbol, 'line_start', f.line_start, 'line_end', f.line_end))
                    FROM task_files f WHERE f.task_uuid = t.uuid),
                'notes', (SELECT json_group_array(json_object(
                        'id', a.id, 'kind', a.kind, 'author', a.author, 'text', a.text,
                        'target_kind', a.target_kind, 'target_id', a.target_id,
                        'status', a.status, 'request_revision', a.request_revision,
                        'resolved_by_run', a.resolved_by_run, 'entry', a.entry))
                    FROM annotations a WHERE a.task_uuid = t.uuid),
                'links', (SELECT json_group_array(json_object('id', l.id, 'url', l.url, 'label', l.label))
                    FROM task_links l WHERE l.task_uuid = t.uuid),
                'ai_runs', (SELECT json_group_array(json_object(
                        'id', r.id, 'kind', r.kind, 'model', r.model, 'provider', r.provider, 'created_at', r.created_at))
                    FROM task_ai_runs r WHERE r.task_uuid = t.uuid),
                'blocked_by', (SELECT json_group_array(b.id)
                    FROM dependencies d JOIN tasks b ON b.uuid = d.depends_on_uuid
                    WHERE d.task_uuid = t.uuid AND b.status = 'pending')
             ) AS guide_json
             FROM tasks t;",
        ),
    ]);
    migrations
        .to_latest(conn)
        .context("Database migration failed")
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn dt_to_str(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

fn str_to_dt(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .with_context(|| format!("Invalid datetime: {s}"))
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let uuid_str: String = row.get(0)?;
    let id: Option<i64> = row.get(1)?;
    let description: String = row.get(2)?;
    let project: String = row.get(3)?;
    let status_str: String = row.get(4)?;
    let priority_str: Option<String> = row.get(5)?;
    let due_str: Option<String> = row.get(6)?;
    let entry_str: String = row.get(7)?;
    let modified_str: String = row.get(8)?;
    let end_str: Option<String> = row.get(9)?;
    let tags_json: String = row.get(10)?;
    let urgency: f64 = row.get(11)?;
    let started_str: Option<String> = row.get(12)?;
    let time_spent: i64 = row.get(13)?;
    let estimate_mins: Option<i64> = row.get(14)?;
    let recur: Option<String> = row.get(15)?;

    let uuid = Uuid::parse_str(&uuid_str).unwrap_or_else(|_| Uuid::new_v4());
    let status = match status_str.as_str() {
        "completed" => Status::Completed,
        "deleted" => Status::Deleted,
        _ => Status::Pending,
    };
    let priority = priority_str.and_then(|s| s.parse::<Priority>().ok());
    let due = due_str.and_then(|s| str_to_dt(&s).ok());
    let entry = str_to_dt(&entry_str).unwrap_or_else(|_| Utc::now());
    let modified = str_to_dt(&modified_str).unwrap_or_else(|_| Utc::now());
    let end = end_str.and_then(|s| str_to_dt(&s).ok());
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let started_at = started_str.and_then(|s| str_to_dt(&s).ok());

    Ok(Task {
        uuid,
        id,
        description,
        project,
        status,
        priority,
        due,
        entry,
        modified,
        end,
        tags,
        urgency,
        started_at,
        time_spent,
        estimate_mins,
        recur,
    })
}

const TASK_COLUMNS: &str = "uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur";

// ── undo ─────────────────────────────────────────────────────────────────────

struct UndoCtx {
    batch_id: String,
    command: String,
}

thread_local! {
    /// Active undo batch for the current process/thread. When set, every task
    /// write records a snapshot so the whole command can later be reverted.
    static UNDO_CTX: std::cell::RefCell<Option<UndoCtx>> = const { std::cell::RefCell::new(None) };
}

/// Open an undo batch for the current command (e.g. "done 3"). All task writes
/// until process exit are grouped under one batch that `undo` reverts together.
pub fn begin_undo_batch(command: &str) {
    UNDO_CTX.with(|c| {
        *c.borrow_mut() = Some(UndoCtx {
            batch_id: Uuid::new_v4().to_string(),
            command: command.to_string(),
        });
    });
}

/// Record a single task snapshot into the active batch (no-op if none is open).
fn log_undo(
    conn: &Connection,
    task_uuid: &Uuid,
    before: Option<&Task>,
    after: Option<&Task>,
) -> Result<()> {
    let entry = UNDO_CTX.with(|c| {
        c.borrow()
            .as_ref()
            .map(|ctx| (ctx.batch_id.clone(), ctx.command.clone()))
    });
    let Some((batch_id, command)) = entry else {
        return Ok(());
    };
    let before_json = before.map(serde_json::to_string).transpose()?;
    let after_json = after.map(serde_json::to_string).transpose()?;
    conn.execute(
        "INSERT INTO undo_log (batch_id, command, task_uuid, before_json, after_json, created_at)
         VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            batch_id,
            command,
            task_uuid.to_string(),
            before_json,
            after_json,
            dt_to_str(&Utc::now()),
        ],
    )?;
    Ok(())
}

/// Restore a task row to a previous snapshot. Uses UPDATE (never REPLACE) so
/// dependent rows in other tables keyed by uuid are preserved.
fn restore_task_row(conn: &Connection, t: &Task) -> Result<()> {
    let n = conn.execute(
        "UPDATE tasks SET id=?1, description=?2, project=?3, status=?4, priority=?5, due=?6,
                          entry=?7, modified=?8, end=?9, tags_json=?10, urgency=?11,
                          started_at=?12, time_spent=?13
         WHERE uuid=?14",
        params![
            t.id,
            t.description,
            t.project,
            t.status.to_string(),
            t.priority.as_ref().map(|p| p.label()),
            t.due.as_ref().map(dt_to_str),
            dt_to_str(&t.entry),
            dt_to_str(&t.modified),
            t.end.as_ref().map(dt_to_str),
            serde_json::to_string(&t.tags).unwrap_or_else(|_| "[]".into()),
            t.urgency,
            t.started_at.as_ref().map(dt_to_str),
            t.time_spent,
            t.uuid.to_string(),
        ],
    )?;
    if n == 0 {
        conn.execute(
            "INSERT INTO tasks (uuid, id, description, project, status, priority, due,
                                entry, modified, end, tags_json, urgency, started_at, time_spent,
                                estimate_mins, recur)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                t.uuid.to_string(),
                t.id,
                t.description,
                t.project,
                t.status.to_string(),
                t.priority.as_ref().map(|p| p.label()),
                t.due.as_ref().map(dt_to_str),
                dt_to_str(&t.entry),
                dt_to_str(&t.modified),
                t.end.as_ref().map(dt_to_str),
                serde_json::to_string(&t.tags).unwrap_or_else(|_| "[]".into()),
                t.urgency,
                t.started_at.as_ref().map(dt_to_str),
                t.time_spent,
                t.estimate_mins,
                t.recur,
            ],
        )?;
    }
    Ok(())
}

/// Revert the most recent recorded command. Returns the command label that was
/// undone, or None when there is nothing to undo.
pub fn undo(conn: &Connection) -> Result<Option<String>> {
    let latest: Option<(String, String)> = conn
        .query_row(
            "SELECT batch_id, command FROM undo_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((batch_id, command)) = latest else {
        return Ok(None);
    };

    // Reverse the writes newest-first within the batch.
    let entries: Vec<(Option<String>, String)> = {
        let mut stmt = conn.prepare(
            "SELECT before_json, task_uuid FROM undo_log WHERE batch_id=?1 ORDER BY id DESC",
        )?;
        stmt.query_map([&batch_id], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (before_json, task_uuid) in entries {
        match before_json {
            // Task existed before: restore that snapshot.
            Some(json) => {
                let task: Task =
                    serde_json::from_str(&json).context("Failed to decode undo snapshot")?;
                restore_task_row(conn, &task)?;
            }
            // Task was created by this command: removing it (and cascaded rows) undoes it.
            None => {
                conn.execute("DELETE FROM tasks WHERE uuid=?1", [&task_uuid])?;
            }
        }
    }

    conn.execute("DELETE FROM undo_log WHERE batch_id=?1", [&batch_id])?;
    repack_ids(conn)?;
    Ok(Some(command))
}

// ── task CRUD ────────────────────────────────────────────────────────────────

pub fn next_display_id(conn: &Connection) -> Result<i64> {
    // Find the smallest positive integer not in use by pending tasks
    let mut stmt = conn.prepare(
        "SELECT id FROM tasks WHERE status='pending' AND id IS NOT NULL ORDER BY id ASC",
    )?;
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    let mut next = 1i64;
    for id in ids {
        if id == next {
            next += 1;
        } else {
            break;
        }
    }
    Ok(next)
}

pub fn insert_task(conn: &Connection, task: &mut Task) -> Result<()> {
    let id = next_display_id(conn)?;
    task.id = Some(id);
    conn.execute(
        "INSERT INTO tasks (uuid, id, description, project, status, priority, due,
                            entry, modified, end, tags_json, urgency, started_at, time_spent,
                            estimate_mins, recur)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            task.uuid.to_string(),
            task.id,
            task.description,
            task.project,
            task.status.to_string(),
            task.priority.as_ref().map(|p| p.label()),
            task.due.as_ref().map(dt_to_str),
            dt_to_str(&task.entry),
            dt_to_str(&task.modified),
            task.end.as_ref().map(dt_to_str),
            serde_json::to_string(&task.tags).unwrap_or_else(|_| "[]".into()),
            task.urgency,
            task.started_at.as_ref().map(dt_to_str),
            task.time_spent,
            task.estimate_mins,
            task.recur,
        ],
    )?;
    conn.execute(
        "INSERT INTO task_history (task_uuid, field, old_value, new_value, changed_at)
         VALUES (?1, 'created', NULL, ?2, ?3)",
        params![
            task.uuid.to_string(),
            task.description,
            dt_to_str(&task.entry),
        ],
    )?;
    log_undo(conn, &task.uuid, None, Some(task))?;
    Ok(())
}

pub fn get_task_by_id(conn: &Connection, id: i64) -> Result<Option<Task>> {
    let mut stmt = conn.prepare(
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
         FROM tasks WHERE id=?1 AND status='pending' LIMIT 1",
    )?;
    let mut rows = stmt.query_map([id], row_to_task)?;
    Ok(rows.next().transpose()?)
}

pub fn get_task_by_uuid_prefix(conn: &Connection, prefix: &str) -> Result<Option<Task>> {
    let pattern = format!("{prefix}%");
    let mut stmt = conn.prepare(
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
         FROM tasks WHERE uuid LIKE ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query_map([pattern], row_to_task)?;
    Ok(rows.next().transpose()?)
}

/// Resolve "3" (display id) or a uuid prefix to a Task
pub fn resolve_task(conn: &Connection, id_or_uuid: &str) -> Result<Task> {
    if let Ok(n) = id_or_uuid.parse::<i64>()
        && let Some(t) = get_task_by_id(conn, n)?
    {
        return Ok(t);
    }
    if let Some(t) = get_task_by_uuid_prefix(conn, id_or_uuid)? {
        return Ok(t);
    }
    Err(anyhow::anyhow!(
        "No pending task with id or uuid matching '{id_or_uuid}'"
    ))
}

pub fn list_tasks(conn: &Connection, project: Option<&str>) -> Result<Vec<Task>> {
    let sql = if project.is_some() {
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
         FROM tasks WHERE status='pending' AND project=?1 ORDER BY urgency DESC"
    } else {
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
         FROM tasks WHERE status='pending' ORDER BY urgency DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(p) = project {
        stmt.query_map([p], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map([], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

/// Pending tasks (by urgency DESC) followed by completed tasks (by end DESC) for a project.
/// Used by `sara board` to show the full feature progress view.
pub fn list_tasks_for_board(conn: &Connection, project: &str) -> Result<Vec<Task>> {
    let mut tasks = Vec::new();
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE project=?1 AND status='pending' ORDER BY urgency DESC"
    ))?;
    tasks.extend(
        stmt.query_map([project], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE project=?1 AND status='completed' ORDER BY end DESC"
    ))?;
    tasks.extend(
        stmt.query_map([project], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    Ok(tasks)
}

/// All dependency edges between tasks that both belong to `project`, regardless of
/// status. Each tuple is `(task, depends_on)` — i.e. `task` is blocked by / comes
/// after `depends_on` in the chain. Used by `sara board` to group tasks into the
/// features (dependency chains) they belong to.
pub fn dependency_edges_for_project(conn: &Connection, project: &str) -> Result<Vec<(Uuid, Uuid)>> {
    let mut stmt = conn.prepare(
        "SELECT d.task_uuid, d.depends_on_uuid
         FROM dependencies d
         JOIN tasks t ON t.uuid = d.task_uuid
         JOIN tasks b ON b.uuid = d.depends_on_uuid
         WHERE t.project = ?1 AND b.project = ?1",
    )?;
    let rows = stmt
        .query_map([project], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(a, b)| Some((Uuid::parse_str(&a).ok()?, Uuid::parse_str(&b).ok()?)))
        .collect();
    Ok(rows)
}

/// The dependency chain (connected component of the dependency graph) that
/// contains `task_uuid`, returned as tasks in blockers-first (topological) order.
/// Scoped to the task's project. Returns an empty vec when the task stands alone
/// (no linked tasks) so callers can skip rendering a one-node "chain".
pub fn feature_chain(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Task>> {
    let current = match get_task_by_uuid_prefix(conn, &task_uuid.to_string()[..8])? {
        Some(t) => t,
        None => return Ok(vec![]),
    };
    let all = list_tasks_for_board(conn, &current.project)?;
    let pos: std::collections::HashMap<Uuid, usize> =
        all.iter().enumerate().map(|(i, t)| (t.uuid, i)).collect();
    let Some(&start) = pos.get(task_uuid) else {
        return Ok(vec![]);
    };
    let edges = dependency_edges_for_project(conn, &current.project)?;
    let n = all.len();

    // Undirected adjacency (for the component) + directed dependents (for topo).
    let mut undirected: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut dependents: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    let mut indeg = vec![0usize; n];
    for (task, dep) in &edges {
        if let (Some(&ti), Some(&di)) = (pos.get(task), pos.get(dep)) {
            undirected[ti].push(di);
            undirected[di].push(ti);
            dependents.entry(di).or_default().push(ti);
            indeg[ti] += 1;
        }
    }

    // Connected component containing `start` (undirected BFS).
    let mut seen = vec![false; n];
    let mut comp: Vec<usize> = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start);
    seen[start] = true;
    while let Some(x) = queue.pop_front() {
        comp.push(x);
        for &y in &undirected[x] {
            if !seen[y] {
                seen[y] = true;
                queue.push_back(y);
            }
        }
    }
    if comp.len() <= 1 {
        return Ok(vec![]);
    }

    // Kahn topological sort within the component (blockers first). Ties break by
    // position in `all` (pending-by-urgency, then completed) for stable output.
    let comp_set: std::collections::HashSet<usize> = comp.iter().copied().collect();
    let mut remaining: std::collections::HashMap<usize, usize> =
        comp.iter().map(|&i| (i, indeg[i])).collect();
    let mut ready: Vec<usize> = comp.iter().copied().filter(|i| indeg[*i] == 0).collect();
    ready.sort_unstable_by(|a, b| b.cmp(a)); // pop() yields smallest position
    let mut order: Vec<usize> = Vec::with_capacity(comp.len());
    while let Some(i) = ready.pop() {
        order.push(i);
        if let Some(deps) = dependents.get(&i) {
            for &j in deps {
                if !comp_set.contains(&j) {
                    continue;
                }
                if let Some(d) = remaining.get_mut(&j) {
                    *d -= 1;
                    if *d == 0 {
                        ready.push(j);
                        ready.sort_unstable_by(|a, b| b.cmp(a));
                    }
                }
            }
        }
    }
    if order.len() < comp.len() {
        let mut leftover: Vec<usize> = comp
            .iter()
            .copied()
            .filter(|i| !order.contains(i))
            .collect();
        leftover.sort_unstable();
        order.extend(leftover);
    }

    Ok(order.into_iter().map(|i| all[i].clone()).collect())
}

pub fn update_task(conn: &Connection, task: &Task) -> Result<()> {
    let prev = get_task_by_uuid_prefix(conn, &task.uuid.to_string())?;
    conn.execute(
        "UPDATE tasks SET description=?1, project=?2, status=?3, priority=?4, due=?5,
                         modified=?6, end=?7, tags_json=?8, urgency=?9,
                         started_at=?10, time_spent=?11, estimate_mins=?12, recur=?13
         WHERE uuid=?14",
        params![
            task.description,
            task.project,
            task.status.to_string(),
            task.priority.as_ref().map(|p| p.label()),
            task.due.as_ref().map(dt_to_str),
            dt_to_str(&task.modified),
            task.end.as_ref().map(dt_to_str),
            serde_json::to_string(&task.tags).unwrap_or_else(|_| "[]".into()),
            task.urgency,
            task.started_at.as_ref().map(dt_to_str),
            task.time_spent,
            task.estimate_mins,
            task.recur,
            task.uuid.to_string(),
        ],
    )?;
    if let Some(prev) = prev {
        log_undo(conn, &task.uuid, Some(&prev), Some(task))?;
        record_changes(conn, &prev, task)?;
    }
    Ok(())
}

/// Display-ready values for each tracked field, used to diff task revisions.
fn tracked_field_values(t: &Task) -> [(&'static str, Option<String>); 9] {
    [
        ("description", non_empty(&t.description)),
        ("project", non_empty(&t.project)),
        ("status", Some(t.status.to_string())),
        (
            "priority",
            t.priority.as_ref().map(|p| p.label().to_string()),
        ),
        (
            "due",
            t.due.map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d")
                    .to_string()
            }),
        ),
        (
            "tags",
            if t.tags.is_empty() {
                None
            } else {
                Some(t.tags.join(", "))
            },
        ),
        ("estimate", t.estimate_mins.map(fmt_estimate)),
        ("recur", t.recur.clone()),
        ("timer", t.started_at.map(|_| "running".to_string())),
    ]
}

/// Human-readable estimate (e.g. "90" minutes -> "1h30m").
fn fmt_estimate(mins: i64) -> String {
    if mins >= 60 {
        let h = mins / 60;
        let r = mins % 60;
        if r == 0 {
            format!("{h}h")
        } else {
            format!("{h}h{r}m")
        }
    } else {
        format!("{mins}m")
    }
}

fn non_empty(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Append a single history row for a task.
fn record_history(
    conn: &Connection,
    task_uuid: &Uuid,
    field: &str,
    old_value: Option<&str>,
    new_value: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO task_history (task_uuid, field, old_value, new_value, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            task_uuid.to_string(),
            field,
            old_value,
            new_value,
            dt_to_str(&Utc::now())
        ],
    )?;
    Ok(())
}

/// Record one history row per tracked field that changed between revisions.
fn record_changes(conn: &Connection, old: &Task, new: &Task) -> Result<()> {
    let olds = tracked_field_values(old);
    let news = tracked_field_values(new);
    let at = dt_to_str(&new.modified);
    for ((field, old_val), (_, new_val)) in olds.into_iter().zip(news) {
        if old_val != new_val {
            conn.execute(
                "INSERT INTO task_history (task_uuid, field, old_value, new_value, changed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![new.uuid.to_string(), field, old_val, new_val, at],
            )?;
        }
    }
    Ok(())
}

/// After completing a task, compact pending IDs to stay small
pub fn repack_ids(conn: &Connection) -> Result<()> {
    let mut stmt =
        conn.prepare("SELECT uuid FROM tasks WHERE status='pending' ORDER BY entry ASC")?;
    let uuids: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for (i, uuid) in uuids.iter().enumerate() {
        conn.execute(
            "UPDATE tasks SET id=?1 WHERE uuid=?2",
            params![i as i64 + 1, uuid],
        )?;
    }
    Ok(())
}

// ── dependencies ─────────────────────────────────────────────────────────────

pub fn add_dependency(conn: &Connection, task_uuid: &Uuid, dep_uuid: &Uuid) -> Result<()> {
    if task_uuid == dep_uuid {
        anyhow::bail!("A task cannot depend on itself");
    }
    if would_create_cycle(conn, task_uuid, dep_uuid)? {
        anyhow::bail!("Adding this dependency would create a cycle");
    }
    let n = conn.execute(
        "INSERT OR IGNORE INTO dependencies (task_uuid, depends_on_uuid) VALUES (?1,?2)",
        params![task_uuid.to_string(), dep_uuid.to_string()],
    )?;
    if n > 0 {
        let label = dep_label(conn, dep_uuid);
        record_history(conn, task_uuid, "dependency", None, Some(&label))?;
    }
    Ok(())
}

pub fn remove_dependency(conn: &Connection, task_uuid: &Uuid, dep_uuid: &Uuid) -> Result<()> {
    let n = conn.execute(
        "DELETE FROM dependencies WHERE task_uuid=?1 AND depends_on_uuid=?2",
        params![task_uuid.to_string(), dep_uuid.to_string()],
    )?;
    if n > 0 {
        let label = dep_label(conn, dep_uuid);
        record_history(conn, task_uuid, "dependency", Some(&label), None)?;
    }
    Ok(())
}

/// "[id] description" label for a dependency task, falling back to the uuid.
fn dep_label(conn: &Connection, dep_uuid: &Uuid) -> String {
    get_task_by_uuid_prefix(conn, &dep_uuid.to_string()[..8])
        .ok()
        .flatten()
        .map(|t| format!("[{}] {}", t.id.unwrap_or(0), t.description))
        .unwrap_or_else(|| dep_uuid.to_string())
}

fn would_create_cycle(conn: &Connection, task: &Uuid, new_dep: &Uuid) -> Result<bool> {
    // If new_dep transitively depends on task, adding task->new_dep creates a cycle
    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![new_dep.to_string()];
    while let Some(cur) = queue.pop() {
        if cur == task.to_string() {
            return Ok(true);
        }
        if !visited.insert(cur.clone()) {
            continue;
        }
        let mut stmt =
            conn.prepare("SELECT depends_on_uuid FROM dependencies WHERE task_uuid=?1")?;
        let deps: Vec<String> = stmt
            .query_map([&cur], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        queue.extend(deps);
    }
    Ok(false)
}

pub fn get_blockers(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Uuid>> {
    let mut stmt = conn.prepare(
        "SELECT d.depends_on_uuid FROM dependencies d
         JOIN tasks t ON t.uuid=d.depends_on_uuid
         WHERE d.task_uuid=?1 AND t.status='pending'",
    )?;
    let uuids = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(uuids)
}

pub fn get_blocking(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Uuid>> {
    let mut stmt = conn.prepare("SELECT task_uuid FROM dependencies WHERE depends_on_uuid=?1")?;
    let uuids = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(uuids)
}

/// All dependency (blocker) uuids for a task regardless of the blocker's status.
/// Unlike [`get_blockers`], which only returns *pending* blockers for urgency and
/// readiness, this returns every `depends_on` edge — used when exporting a task's
/// full dependency closure.
pub fn get_dependency_uuids(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Uuid>> {
    let mut stmt = conn.prepare("SELECT depends_on_uuid FROM dependencies WHERE task_uuid=?1")?;
    let uuids = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(uuids)
}

/// Dependency state of a single task, for at-a-glance list rendering.
#[derive(Debug, Default, Clone)]
pub struct DepInfo {
    /// Display IDs of the pending tasks that block this task (sorted).
    pub blocked_by: Vec<i64>,
    /// How many pending tasks this task is blocking.
    pub blocking: usize,
}

impl DepInfo {
    pub fn is_blocked(&self) -> bool {
        !self.blocked_by.is_empty()
    }
}

/// Dependency state for every task that has any, keyed by task uuid string.
/// Only pending tasks count as live blockers/dependents, matching the
/// semantics of `get_blockers`/`get_blocking`. Computed in two batch queries
/// so `sara list` stays O(1) in round-trips regardless of task count.
pub fn dep_info_by_task(conn: &Connection) -> Result<std::collections::HashMap<String, DepInfo>> {
    let mut map: std::collections::HashMap<String, DepInfo> = std::collections::HashMap::new();

    // Pending blockers (with their display id) for each task.
    let mut stmt = conn.prepare(
        "SELECT d.task_uuid, b.id
         FROM dependencies d
         JOIN tasks b ON b.uuid = d.depends_on_uuid
         WHERE b.status='pending'",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
    })?;
    for row in rows {
        let (task_uuid, blocker_id) = row?;
        if let Some(id) = blocker_id {
            map.entry(task_uuid).or_default().blocked_by.push(id);
        }
    }

    // How many pending tasks each task is blocking.
    let mut stmt = conn.prepare(
        "SELECT d.depends_on_uuid, COUNT(*)
         FROM dependencies d
         JOIN tasks t ON t.uuid = d.task_uuid
         WHERE t.status='pending'
         GROUP BY d.depends_on_uuid",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (dep_uuid, count) = row?;
        map.entry(dep_uuid).or_default().blocking = count as usize;
    }

    for info in map.values_mut() {
        info.blocked_by.sort_unstable();
    }
    Ok(map)
}

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
    let after: std::collections::HashSet<String> = files.iter().map(|(p, _)| p.clone()).collect();
    for path in after.difference(&before) {
        record_history(conn, task_uuid, "file", None, Some(path))?;
    }
    for path in before.difference(&after) {
        record_history(conn, task_uuid, "file", Some(path), None)?;
    }
    Ok(())
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

// ── links ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Link {
    pub id: i64,
    pub url: String,
    pub label: Option<String>,
    pub entry: DateTime<Utc>,
}

impl Link {
    /// A human-friendly display string (explicit label, else derived from URL).
    pub fn display(&self) -> String {
        self.label
            .clone()
            .or_else(|| derive_link_label(&self.url))
            .unwrap_or_else(|| self.url.clone())
    }
}

/// Heuristic: does this string look like a web URL rather than a file path?
pub fn is_url(s: &str) -> bool {
    let s = s.trim();
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.contains("://")
        || s.starts_with("www.")
}

/// Derive a nice label from common URLs (e.g. GitHub PRs/issues).
/// Returns None when no special pattern applies.
pub fn derive_link_label(url: &str) -> Option<String> {
    // https://github.com/<owner>/<repo>/pull/<n>  or  /issues/<n>
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("github.com/"))?;
    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 4 {
        let owner = parts[0];
        let repo = parts[1];
        let kind = parts[2];
        let num = parts[3]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap_or("");
        let tag = match kind {
            "pull" => Some("PR"),
            "issues" => Some("Issue"),
            _ => None,
        };
        if let (Some(tag), false) = (tag, num.is_empty()) {
            return Some(format!("{tag} #{num} · {owner}/{repo}"));
        }
    }
    None
}

pub fn add_link(conn: &Connection, task_uuid: &Uuid, url: &str, label: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO task_links (task_uuid, url, label, entry) VALUES (?1,?2,?3,?4)",
        params![task_uuid.to_string(), url, label, dt_to_str(&Utc::now())],
    )?;
    let display = label
        .map(|s| s.to_string())
        .or_else(|| derive_link_label(url))
        .unwrap_or_else(|| url.to_string());
    record_history(conn, task_uuid, "link", None, Some(&display))?;
    Ok(())
}

pub fn get_links(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Link>> {
    let mut stmt = conn.prepare(
        "SELECT id, url, label, entry FROM task_links WHERE task_uuid=?1 ORDER BY entry ASC",
    )?;
    let links = stmt
        .query_map([task_uuid.to_string()], |row| {
            let entry_str: String = row.get(3)?;
            Ok(Link {
                id: row.get(0)?,
                url: row.get(1)?,
                label: row.get(2)?,
                entry: str_to_dt(&entry_str).unwrap_or_else(|_| Utc::now()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(links)
}

/// Link presence summary for a single task, for at-a-glance list markers.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinkFlags {
    /// Task has at least one link of any kind.
    pub any: bool,
    /// Task has at least one GitHub PR link.
    pub pr: bool,
}

/// Build a per-task link-flag map in a single query (keyed by task uuid string).
pub fn link_flags_by_task(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, LinkFlags>> {
    let mut stmt = conn.prepare("SELECT task_uuid, url FROM task_links")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map: std::collections::HashMap<String, LinkFlags> = std::collections::HashMap::new();
    for row in rows {
        let (uuid, url) = row?;
        let is_pr = derive_link_label(&url)
            .map(|l| l.starts_with("PR "))
            .unwrap_or(false);
        let entry = map.entry(uuid).or_default();
        entry.any = true;
        entry.pr = entry.pr || is_pr;
    }
    Ok(map)
}

pub fn delete_link(conn: &Connection, link_id: i64) -> Result<bool> {
    let existing: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT task_uuid, url, label FROM task_links WHERE id=?1",
            [link_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .ok();

    let n = conn.execute("DELETE FROM task_links WHERE id=?1", [link_id])?;
    if n > 0
        && let Some((uuid_str, url, label)) = existing
        && let Ok(uuid) = Uuid::parse_str(&uuid_str)
    {
        let display = label.or_else(|| derive_link_label(&url)).unwrap_or(url);
        record_history(conn, &uuid, "link", Some(&display), None)?;
    }
    Ok(n > 0)
}

// ── history ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub changed_at: DateTime<Utc>,
}

/// All recorded changes for a task, oldest first.
pub fn get_history(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT field, old_value, new_value, changed_at
         FROM task_history WHERE task_uuid=?1 ORDER BY changed_at ASC, id ASC",
    )?;
    let rows = stmt
        .query_map([task_uuid.to_string()], |row| {
            let changed_str: String = row.get(3)?;
            Ok(HistoryEntry {
                field: row.get(0)?,
                old_value: row.get(1)?,
                new_value: row.get(2)?,
                changed_at: str_to_dt(&changed_str).unwrap_or_else(|_| Utc::now()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ── branch snapshots ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BranchRecord {
    pub branch: String,
    pub base: Option<String>,
    /// Files changed on `branch` since merge-base with `base`; None until first snapshot.
    pub files: Option<Vec<String>>,
    pub logged_at: Option<DateTime<Utc>>,
}

fn parse_files_json(s: Option<String>) -> Option<Vec<String>> {
    s.and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
}

pub fn set_task_branch(conn: &Connection, task_uuid: &Uuid, branch: &str) -> Result<()> {
    // Get previous branch for history.
    let prev = get_task_branch(conn, task_uuid).map(|r| r.branch);
    conn.execute(
        "INSERT INTO task_branches (task_uuid, branch)
         VALUES (?1, ?2)
         ON CONFLICT(task_uuid) DO UPDATE SET
           branch             = ?2,
           base               = NULL,
           changed_files_json = NULL,
           logged_at          = NULL",
        params![task_uuid.to_string(), branch],
    )?;
    record_history(conn, task_uuid, "branch", prev.as_deref(), Some(branch))?;
    Ok(())
}

pub fn log_branch_changes(
    conn: &Connection,
    task_uuid: &Uuid,
    base: &str,
    files: &[String],
) -> Result<()> {
    let json = serde_json::to_string(files)?;
    let now = dt_to_str(&Utc::now());
    conn.execute(
        "UPDATE task_branches SET base=?2, changed_files_json=?3, logged_at=?4
         WHERE task_uuid=?1",
        params![task_uuid.to_string(), base, json, now],
    )?;
    Ok(())
}

pub fn get_task_branch(conn: &Connection, task_uuid: &Uuid) -> Option<BranchRecord> {
    conn.query_row(
        "SELECT branch, base, changed_files_json, logged_at FROM task_branches WHERE task_uuid=?1",
        [task_uuid.to_string()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )
    .ok()
    .map(|(branch, base, files_json, logged_at)| BranchRecord {
        branch,
        base,
        files: parse_files_json(files_json),
        logged_at: logged_at.and_then(|s| str_to_dt(&s).ok()),
    })
}

pub fn clear_task_branch(conn: &Connection, task_uuid: &Uuid) -> Result<()> {
    let prev = get_task_branch(conn, task_uuid).map(|r| r.branch);
    let n = conn.execute(
        "DELETE FROM task_branches WHERE task_uuid=?1",
        [task_uuid.to_string()],
    )?;
    if n > 0 {
        record_history(conn, task_uuid, "branch", prev.as_deref(), None)?;
    }
    Ok(())
}

/// All pending tasks in `project` (excluding `exclude_uuid`) that have a branch record.
/// Returns `(task_id, description, BranchRecord)`.
pub fn branched_pending_in_project(
    conn: &Connection,
    project: &str,
    exclude_uuid: &Uuid,
) -> Result<Vec<(i64, String, BranchRecord)>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.description, tb.branch, tb.base, tb.changed_files_json, tb.logged_at
         FROM tasks t
         JOIN task_branches tb ON tb.task_uuid = t.uuid
         WHERE t.project=?1 AND t.status='pending' AND t.uuid != ?2
         ORDER BY t.id ASC",
    )?;
    let rows = stmt.query_map(params![project, exclude_uuid.to_string()], |row| {
        Ok((
            row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut result = vec![];
    for row in rows {
        let (id, desc, branch, base, files_json, logged_at) = row?;
        result.push((
            id,
            desc,
            BranchRecord {
                branch,
                base,
                files: parse_files_json(files_json),
                logged_at: logged_at.and_then(|s| str_to_dt(&s).ok()),
            },
        ));
    }
    Ok(result)
}

// ── projects ─────────────────────────────────────────────────────────────────

pub fn upsert_project_seen(conn: &Connection, name: &str, path: Option<&str>) -> Result<()> {
    let now = dt_to_str(&Utc::now());
    conn.execute(
        "INSERT INTO projects (name, path, last_seen) VALUES (?1,?2,?3)
         ON CONFLICT(name) DO UPDATE SET
           path     = COALESCE(?2, path),
           last_seen = ?3",
        params![name, path, now],
    )?;
    Ok(())
}

pub fn save_project_profile(conn: &Connection, project: &Project) -> Result<()> {
    let now = dt_to_str(&Utc::now());
    conn.execute(
        "INSERT INTO projects (name, path, goal, stack, conventions, notes,
                              initialized_at, last_seen,
                              github_repo, github_login, github_sync_scope)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7,?8,?9,?10)
         ON CONFLICT(name) DO UPDATE SET
           path          = COALESCE(?2, path),
           goal          = COALESCE(?3, goal),
           stack         = COALESCE(?4, stack),
           conventions   = COALESCE(?5, conventions),
           notes         = COALESCE(?6, notes),
           initialized_at = COALESCE(?7, initialized_at),
           last_seen      = ?7,
           github_repo    = COALESCE(?8, github_repo),
           github_login   = COALESCE(?9, github_login),
           github_sync_scope = COALESCE(?10, github_sync_scope)",
        params![
            project.name,
            project.path,
            project.goal,
            project.stack,
            project.conventions,
            project.notes,
            now,
            project.github_repo,
            project.github_login,
            project.github_sync_scope,
        ],
    )?;
    Ok(())
}

pub fn get_project(conn: &Connection, name: &str) -> Result<Option<Project>> {
    let mut stmt = conn.prepare(
        "SELECT name,path,goal,stack,conventions,notes,initialized_at,last_seen,
                github_repo,github_login,github_sync_scope
         FROM projects WHERE name=?1",
    )?;
    let mut rows = stmt.query_map([name], |row| {
        Ok(Project {
            name: row.get(0)?,
            path: row.get(1)?,
            goal: row.get(2)?,
            stack: row.get(3)?,
            conventions: row.get(4)?,
            notes: row.get(5)?,
            initialized_at: row
                .get::<_, Option<String>>(6)?
                .and_then(|s| str_to_dt(&s).ok()),
            last_seen: row
                .get::<_, Option<String>>(7)?
                .and_then(|s| str_to_dt(&s).ok()),
            github_repo: row.get(8)?,
            github_login: row.get(9)?,
            github_sync_scope: row.get(10)?,
        })
    })?;
    Ok(rows.next().transpose()?)
}

/// All known project names — the union of registered profiles and any project
/// referenced by a task — sorted. Used for shell-completion candidates.
pub fn project_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT project FROM tasks
         UNION
         SELECT name FROM projects
         ORDER BY 1",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Look up a project profile by its (canonical) path. When several profiles
/// share a path — e.g. stale rows from before path resolution was fixed — the
/// most-recently-seen one wins.
pub fn get_project_by_path(conn: &Connection, path: &str) -> Result<Option<Project>> {
    let mut stmt = conn.prepare(
        "SELECT name,path,goal,stack,conventions,notes,initialized_at,last_seen,
                github_repo,github_login,github_sync_scope
         FROM projects WHERE path=?1 ORDER BY last_seen DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([path], |row| {
        Ok(Project {
            name: row.get(0)?,
            path: row.get(1)?,
            goal: row.get(2)?,
            stack: row.get(3)?,
            conventions: row.get(4)?,
            notes: row.get(5)?,
            initialized_at: row
                .get::<_, Option<String>>(6)?
                .and_then(|s| str_to_dt(&s).ok()),
            last_seen: row
                .get::<_, Option<String>>(7)?
                .and_then(|s| str_to_dt(&s).ok()),
            github_repo: row.get(8)?,
            github_login: row.get(9)?,
            github_sync_scope: row.get(10)?,
        })
    })?;
    Ok(rows.next().transpose()?)
}

/// How many tasks a project currently owns (any status).
pub fn count_project_tasks(conn: &Connection, name: &str) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(n as usize)
}

/// Nuke a project: delete all of its tasks (cascading to their dependencies,
/// files, links, annotations and history), purge undo-log rows for those tasks,
/// and remove the project profile itself. Returns the number of tasks deleted.
pub fn reset_project(conn: &mut Connection, name: &str) -> Result<usize> {
    let tx = conn.transaction()?;

    // Collect the task uuids first so we can clean the undo log (no FK cascade).
    let uuids: Vec<String> = {
        let mut stmt = tx.prepare("SELECT uuid FROM tasks WHERE project=?1")?;
        let rows = stmt.query_map([name], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    for uuid in &uuids {
        tx.execute("DELETE FROM undo_log WHERE task_uuid=?1", [uuid])?;
    }

    // Cascades remove dependencies, task_files, annotations, task_history,
    // and task_links for each deleted task.
    let deleted = tx.execute("DELETE FROM tasks WHERE project=?1", [name])?;
    tx.execute("DELETE FROM projects WHERE name=?1", [name])?;

    tx.commit()?;
    Ok(deleted)
}

// ── urgency ──────────────────────────────────────────────────────────────────

pub fn compute_urgency(
    task: &Task,
    cfg: &crate::config::UrgencyConfig,
    is_blocked: bool,
    blocking_count: usize,
) -> f64 {
    let mut score = 0.0;

    if let Some(ref p) = task.priority {
        score += p.urgency_coefficient();
    }

    if let Some(due) = task.due {
        let days_until: f64 = (due - Utc::now()).num_seconds() as f64 / 86400.0;
        // ramp: overdue -> 1.0, due in 7+ days -> 0.0
        let factor = if days_until <= 0.0 {
            1.0
        } else if days_until >= 7.0 {
            0.0
        } else {
            1.0 - (days_until / 7.0)
        };
        score += cfg.due * factor;
    }

    if blocking_count > 0 {
        score += cfg.blocking;
    }
    if is_blocked {
        score += cfg.blocked;
    }
    if task.is_active() {
        score += cfg.active;
    }
    if !task.tags.is_empty() {
        score += cfg.has_tags;
    }
    if task.project != "inbox" {
        score += cfg.project;
    }

    let age_days = (Utc::now() - task.entry).num_days() as f64;
    let age_factor = (age_days / cfg.age_max).min(1.0);
    score += cfg.age * age_factor;

    score
}

pub fn refresh_urgency(
    conn: &Connection,
    cfg: &crate::config::UrgencyConfig,
    task_uuid: &Uuid,
) -> Result<()> {
    let task = {
        let mut stmt = conn.prepare(
            "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
             FROM tasks WHERE uuid=?1",
        )?;
        let mut rows = stmt.query_map([task_uuid.to_string()], row_to_task)?;
        rows.next()
            .ok_or_else(|| anyhow::anyhow!("Task not found"))??
    };
    let blockers = get_blockers(conn, task_uuid)?;
    let blocking = get_blocking(conn, task_uuid)?;
    let urgency = compute_urgency(&task, cfg, !blockers.is_empty(), blocking.len());
    conn.execute(
        "UPDATE tasks SET urgency=?1 WHERE uuid=?2",
        params![urgency, task_uuid.to_string()],
    )?;
    Ok(())
}

// ── urgency breakdown ─────────────────────────────────────────────────────────

pub struct UrgencyBreakdown {
    pub priority: f64,
    pub due: f64,
    pub blocking: f64,
    pub blocked: f64,
    pub active: f64,
    pub tags: f64,
    pub project: f64,
    pub age: f64,
}

pub fn compute_urgency_breakdown(
    task: &Task,
    cfg: &crate::config::UrgencyConfig,
    is_blocked: bool,
    blocking_count: usize,
) -> UrgencyBreakdown {
    let priority = task
        .priority
        .as_ref()
        .map(|p| p.urgency_coefficient())
        .unwrap_or(0.0);

    let due = if let Some(due) = task.due {
        let days_until: f64 = (due - Utc::now()).num_seconds() as f64 / 86400.0;
        let factor = if days_until <= 0.0 {
            1.0
        } else if days_until >= 7.0 {
            0.0
        } else {
            1.0 - (days_until / 7.0)
        };
        cfg.due * factor
    } else {
        0.0
    };

    let blocking = if blocking_count > 0 {
        cfg.blocking
    } else {
        0.0
    };
    let blocked = if is_blocked { cfg.blocked } else { 0.0 };
    let active = if task.is_active() { cfg.active } else { 0.0 };
    let tags = if !task.tags.is_empty() {
        cfg.has_tags
    } else {
        0.0
    };
    let project = if task.project != "inbox" {
        cfg.project
    } else {
        0.0
    };
    let age_days = (Utc::now() - task.entry).num_days() as f64;
    let age = cfg.age * (age_days / cfg.age_max).min(1.0);

    UrgencyBreakdown {
        priority,
        due,
        blocking,
        blocked,
        active,
        tags,
        project,
        age,
    }
}

// ── similar tasks ─────────────────────────────────────────────────────────────

/// Tasks in the same project sharing at least one tag, excluding the task itself.
pub fn similar_tasks(
    conn: &Connection,
    task_uuid: &Uuid,
    project: &str,
    tags: &[String],
) -> Result<Vec<(i64, String, f64)>> {
    if tags.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE status='pending' AND project=?1 AND uuid!=?2"
    ))?;
    let all: Vec<Task> = stmt
        .query_map(
            rusqlite::params![project, task_uuid.to_string()],
            row_to_task,
        )?
        .filter_map(|r| r.ok())
        .collect();

    let result = all
        .into_iter()
        .filter(|t| t.tags.iter().any(|tag| tags.contains(tag)))
        .filter_map(|t| t.id.map(|id| (id, t.description.clone(), t.urgency)))
        .collect();
    Ok(result)
}

// ── checklist ─────────────────────────────────────────────────────────────────

pub struct ChecklistItem {
    pub id: i64,
    pub text: String,
    pub done: bool,
    pub position: i64,
    /// Fuller "what this step does" written by the LLM (None for legacy items).
    pub intent: Option<String>,
    /// "step" (default) or "acceptance" (a definition-of-done criterion).
    pub kind: String,
    /// "human" or "ai".
    pub source: String,
    /// Command that verifies this step / criterion.
    pub verify_cmd: Option<String>,
    /// Execution outcome recorded when the step is marked done.
    pub result: Option<String>,
    /// Git commit the step was completed at.
    pub done_commit: Option<String>,
    pub done_at: Option<String>,
}

/// Step kinds.
pub const STEP_KIND_STEP: &str = "step";
pub const STEP_KIND_ACCEPTANCE: &str = "acceptance";

fn row_to_step(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChecklistItem> {
    Ok(ChecklistItem {
        id: r.get(0)?,
        text: r.get(1)?,
        done: r.get::<_, i64>(2)? != 0,
        position: r.get(3)?,
        intent: r.get(4)?,
        kind: r.get(5)?,
        source: r.get(6)?,
        verify_cmd: r.get(7)?,
        result: r.get(8)?,
        done_commit: r.get(9)?,
        done_at: r.get(10)?,
    })
}

const STEP_COLUMNS: &str =
    "id, text, done, position, intent, kind, source, verify_cmd, result, done_commit, done_at";

pub fn get_checklist(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<ChecklistItem>> {
    let sql = format!(
        "SELECT {STEP_COLUMNS} FROM task_checklist WHERE task_uuid=?1 ORDER BY position, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let items = stmt
        .query_map([task_uuid.to_string()], row_to_step)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(items)
}

/// Steps (or acceptance criteria) for a task, filtered by kind, ordered.
pub fn get_steps(conn: &Connection, task_uuid: &Uuid, kind: &str) -> Result<Vec<ChecklistItem>> {
    let sql = format!(
        "SELECT {STEP_COLUMNS} FROM task_checklist WHERE task_uuid=?1 AND kind=?2 ORDER BY position, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let items = stmt
        .query_map(rusqlite::params![task_uuid.to_string(), kind], row_to_step)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(items)
}

pub fn add_checklist_item(conn: &Connection, task_uuid: &Uuid, text: &str) -> Result<()> {
    add_step(conn, task_uuid, text, None, STEP_KIND_STEP, "human", None).map(|_| ())
}

/// Insert a step / acceptance criterion with full metadata; returns its id.
pub fn add_step(
    conn: &Connection,
    task_uuid: &Uuid,
    text: &str,
    intent: Option<&str>,
    kind: &str,
    source: &str,
    verify_cmd: Option<&str>,
) -> Result<i64> {
    let pos: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position),0)+1 FROM task_checklist WHERE task_uuid=?1",
            [task_uuid.to_string()],
            |r| r.get(0),
        )
        .unwrap_or(1);
    conn.execute(
        "INSERT INTO task_checklist (task_uuid, text, done, position, intent, kind, source, verify_cmd)
         VALUES (?1,?2,0,?3,?4,?5,?6,?7)",
        rusqlite::params![task_uuid.to_string(), text, pos, intent, kind, source, verify_cmd],
    )?;
    let id = conn.last_insert_rowid();
    let label = if kind == STEP_KIND_ACCEPTANCE {
        "acceptance"
    } else {
        "checklist"
    };
    record_history(conn, task_uuid, label, None, Some(text))?;
    Ok(id)
}

/// Mark a step done/undone, recording the execution result and git commit.
pub fn set_step_done(
    conn: &Connection,
    item_id: i64,
    done: bool,
    result: Option<&str>,
    commit: Option<&str>,
) -> Result<()> {
    let task_uuid_str: String = conn.query_row(
        "SELECT task_uuid FROM task_checklist WHERE id=?1",
        [item_id],
        |r| r.get(0),
    )?;
    if done {
        conn.execute(
            "UPDATE task_checklist SET done=1, result=COALESCE(?2,result), done_commit=?3, done_at=?4 WHERE id=?1",
            rusqlite::params![item_id, result, commit, dt_to_str(&Utc::now())],
        )?;
    } else {
        conn.execute(
            "UPDATE task_checklist SET done=0, done_commit=NULL, done_at=NULL WHERE id=?1",
            [item_id],
        )?;
    }
    if let Ok(uuid) = Uuid::parse_str(&task_uuid_str) {
        record_history(
            conn,
            &uuid,
            "checklist",
            None,
            Some(if done { "step done" } else { "step reopened" }),
        )?;
    }
    Ok(())
}

/// Find a step id by its 1-based position among the task's steps of a kind.
pub fn step_id_by_index(
    conn: &Connection,
    task_uuid: &Uuid,
    kind: &str,
    index: usize,
) -> Result<i64> {
    let steps = get_steps(conn, task_uuid, kind)?;
    steps
        .get(index.saturating_sub(1))
        .map(|s| s.id)
        .ok_or_else(|| anyhow::anyhow!("No {kind} #{index} on this task"))
}

pub fn toggle_checklist_item(conn: &Connection, item_id: i64) -> Result<bool> {
    let (task_uuid_str, text, done): (String, String, i64) = conn.query_row(
        "SELECT task_uuid, text, done FROM task_checklist WHERE id=?1",
        [item_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let new_done = if done == 0 { 1i64 } else { 0i64 };
    conn.execute(
        "UPDATE task_checklist SET done=?1 WHERE id=?2",
        rusqlite::params![new_done, item_id],
    )?;
    if let Ok(uuid) = Uuid::parse_str(&task_uuid_str) {
        let old = format!("{} {text}", if done == 0 { "[ ]" } else { "[x]" });
        let new = format!("{} {text}", if new_done == 0 { "[ ]" } else { "[x]" });
        record_history(conn, &uuid, "checklist", Some(&old), Some(&new))?;
    }
    Ok(new_done != 0)
}

pub fn delete_checklist_item(conn: &Connection, item_id: i64) -> Result<()> {
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT task_uuid, text FROM task_checklist WHERE id=?1",
            [item_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let n = conn.execute("DELETE FROM task_checklist WHERE id=?1", [item_id])?;
    if n > 0
        && let Some((uuid_str, text)) = existing
        && let Ok(uuid) = Uuid::parse_str(&uuid_str)
    {
        record_history(conn, &uuid, "checklist", Some(&text), None)?;
    }
    Ok(())
}

// ── estimate ──────────────────────────────────────────────────────────────────

pub fn set_estimate(conn: &Connection, task_uuid: &Uuid, mins: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET estimate_mins=?1 WHERE uuid=?2",
        rusqlite::params![mins, task_uuid.to_string()],
    )?;
    Ok(())
}

// ── activity heatmap ──────────────────────────────────────────────────────────

/// Returns a map of NaiveDate → activity count for the last `days` days.
/// Activity = tasks created + tasks completed + history change events.
/// When `project` is Some, filter to that project only.
pub fn activity_counts(
    conn: &Connection,
    days: u32,
    project: Option<&str>,
) -> Result<std::collections::HashMap<chrono::NaiveDate, u32>> {
    let mut map: std::collections::HashMap<chrono::NaiveDate, u32> =
        std::collections::HashMap::new();
    let since = (Utc::now() - chrono::Duration::days(days as i64))
        .format("%Y-%m-%d")
        .to_string();

    let proj_filter = if project.is_some() {
        "AND project=?2"
    } else {
        ""
    };

    // Tasks created
    {
        let sql = format!(
            "SELECT substr(entry,1,10), COUNT(*) FROM tasks WHERE entry >= ?1 {proj_filter} GROUP BY substr(entry,1,10)"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(String, u32)> = if let Some(p) = project {
            stmt.query_map(rusqlite::params![since, p], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![since], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        };
        for (date_str, count) in rows {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                *map.entry(d).or_insert(0) += count;
            }
        }
    }

    // Tasks completed
    {
        let sql = format!(
            "SELECT substr(end,1,10), COUNT(*) FROM tasks WHERE end IS NOT NULL AND end >= ?1 {proj_filter} GROUP BY substr(end,1,10)"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(String, u32)> = if let Some(p) = project {
            stmt.query_map(rusqlite::params![since, p], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![since], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        };
        for (date_str, count) in rows {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                *map.entry(d).or_insert(0) += count * 2; // completions count double
            }
        }
    }

    // History events (modifications, annotations, etc.)
    {
        let proj_join = if project.is_some() {
            "JOIN tasks t ON t.uuid = h.task_uuid"
        } else {
            ""
        };
        let proj_where = if project.is_some() {
            "AND t.project=?2"
        } else {
            ""
        };
        let sql = format!(
            "SELECT substr(h.changed_at,1,10), COUNT(*) FROM task_history h {proj_join}
             WHERE h.changed_at >= ?1 {proj_where}
             GROUP BY substr(h.changed_at,1,10)"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(String, u32)> = if let Some(p) = project {
            stmt.query_map(rusqlite::params![since, p], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![since], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        };
        for (date_str, count) in rows {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                *map.entry(d).or_insert(0) += count;
            }
        }
    }

    Ok(map)
}

/// Returns (total_created, total_completed, current_streak_days, longest_streak_days)
pub fn activity_stats(conn: &Connection, project: Option<&str>) -> Result<(u32, u32, u32, u32)> {
    let proj_filter = if project.is_some() {
        "WHERE project=?1"
    } else {
        ""
    };

    let created: u32 = if let Some(p) = project {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM tasks {proj_filter}"),
            rusqlite::params![p],
            |r| r.get(0),
        )?
    } else {
        conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?
    };

    let completed: u32 = if let Some(p) = project {
        conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status='completed' AND project=?1",
            rusqlite::params![p],
            |r| r.get(0),
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status='completed'",
            [],
            |r| r.get(0),
        )?
    };

    // Streak: consecutive days with any activity (from activity_counts).
    // We'll compute this from completion dates for simplicity.
    let mut dates: Vec<chrono::NaiveDate> = {
        let sql = if project.is_some() {
            "SELECT DISTINCT substr(end,1,10) FROM tasks WHERE end IS NOT NULL AND project=?1 ORDER BY end DESC"
        } else {
            "SELECT DISTINCT substr(end,1,10) FROM tasks WHERE end IS NOT NULL ORDER BY end DESC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows: Vec<String> = if let Some(p) = project {
            stmt.query_map(rusqlite::params![p], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map([], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect()
        };
        rows.iter()
            .filter_map(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .collect()
    };
    dates.sort_unstable();
    dates.dedup();

    let today = Utc::now().date_naive();
    let mut current_streak = 0u32;
    let mut longest_streak = 0u32;
    let mut streak = 0u32;
    let mut prev: Option<chrono::NaiveDate> = None;
    for d in &dates {
        if let Some(p) = prev {
            if (*d - p).num_days() == 1 {
                streak += 1;
            } else {
                streak = 1;
            }
        } else {
            streak = 1;
        }
        longest_streak = longest_streak.max(streak);
        prev = Some(*d);
    }
    // Current streak: count backwards from today
    if let Some(&last) = dates.last()
        && (today - last).num_days() <= 1
    {
        current_streak = 1;
        let mut d = last;
        for &prev_d in dates.iter().rev().skip(1) {
            if (d - prev_d).num_days() == 1 {
                current_streak += 1;
                d = prev_d;
            } else {
                break;
            }
        }
    }

    Ok((created, completed, current_streak, longest_streak))
}

// ── project stats ─────────────────────────────────────────────────────────────

pub struct ProjectStats {
    pub pending: u32,
    pub active: u32,
    pub completed_total: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub no_pri: u32,
    pub overdue: u32,
    pub due_today: u32,
    pub due_week: u32,
}

pub fn project_stats(conn: &Connection, project: &str) -> Result<ProjectStats> {
    let now_str = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let today_str = Utc::now().format("%Y-%m-%d").to_string();
    let week_str = (Utc::now() + chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();

    let pending: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending'",
        [project],
        |r| r.get(0),
    )?;
    let active: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND started_at IS NOT NULL",
        [project], |r| r.get(0),
    )?;
    let completed_total: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='completed'",
        [project],
        |r| r.get(0),
    )?;
    let high: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND priority='H'",
        [project],
        |r| r.get(0),
    )?;
    let medium: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND priority='M'",
        [project],
        |r| r.get(0),
    )?;
    let low: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND priority='L'",
        [project],
        |r| r.get(0),
    )?;
    let no_pri: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND priority IS NULL",
        [project],
        |r| r.get(0),
    )?;
    let overdue: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND due IS NOT NULL AND due < ?2",
        rusqlite::params![project, now_str], |r| r.get(0),
    )?;
    let due_today: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND due IS NOT NULL AND substr(due,1,10)=?2",
        rusqlite::params![project, today_str], |r| r.get(0),
    )?;
    let due_week: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND due IS NOT NULL AND due >= ?2 AND substr(due,1,10) <= ?3",
        rusqlite::params![project, now_str, week_str], |r| r.get(0),
    )?;

    Ok(ProjectStats {
        pending,
        active,
        completed_total,
        high,
        medium,
        low,
        no_pri,
        overdue,
        due_today,
        due_week,
    })
}

// ── items (notes & links) ───────────────────────────────────────────────────

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
    let tags_json: String = row.get(6)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Item {
        uuid: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::new_v4()),
        display_id: row.get(2)?,
        kind: row.get(1)?,
        title: row.get(3)?,
        url: row.get(4)?,
        project: row.get(5)?,
        tags,
        path: Some(row.get(7)?),
        summary: row.get(8)?,
        body: row.get(9)?,
        created: str_to_dt(&row.get::<_, String>(10)?).unwrap_or_else(|_| Utc::now()),
        modified: str_to_dt(&row.get::<_, String>(11)?).unwrap_or_else(|_| Utc::now()),
        status: row.get(12)?,
    })
}

fn next_item_display_id(conn: &Connection, kind: &str) -> Result<i64> {
    let max: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(display_id), 0) FROM items WHERE kind = ?1 AND status = 'active'",
            [kind],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(max + 1)
}

pub fn insert_item(conn: &Connection, item: &mut Item) -> Result<()> {
    if item.display_id.is_none() {
        item.display_id = Some(next_item_display_id(conn, &item.kind)?);
    }
    let path = item
        .path
        .clone()
        .context("item path must be set before insert")?;
    let tags_json = serde_json::to_string(&item.tags)?;
    conn.execute(
        "INSERT INTO items (uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        rusqlite::params![
            item.uuid.to_string(),
            item.kind,
            item.display_id,
            item.title,
            item.url,
            item.project,
            tags_json,
            path,
            item.summary,
            item.body,
            dt_to_str(&item.created),
            dt_to_str(&item.modified),
            item.status,
        ],
    )?;
    Ok(())
}

pub fn list_items(conn: &Connection, kind: Option<&str>) -> Result<Vec<Item>> {
    let mut items = vec![];
    if let Some(k) = kind {
        let mut stmt = conn.prepare(
            "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status
             FROM items WHERE status = 'active' AND kind = ?1 ORDER BY display_id",
        )?;
        let rows = stmt.query_map([k], row_to_item)?;
        for r in rows {
            items.push(r?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status
             FROM items WHERE status = 'active' ORDER BY kind, display_id",
        )?;
        let rows = stmt.query_map([], row_to_item)?;
        for r in rows {
            items.push(r?);
        }
    }
    Ok(items)
}

pub fn get_item_by_handle(conn: &Connection, handle: &str) -> Result<Item> {
    let handle = handle.trim().to_lowercase();
    let (kind, id_str) = if let Some(rest) = handle.strip_prefix('n') {
        ("note", rest)
    } else if let Some(rest) = handle.strip_prefix('l') {
        ("link", rest)
    } else {
        anyhow::bail!("Item handle must start with n or l (e.g. n1, l2)");
    };
    let id: i64 = id_str.parse().context("Invalid item id")?;
    conn.query_row(
        "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status
         FROM items WHERE kind = ?1 AND display_id = ?2 AND status = 'active'",
        rusqlite::params![kind, id],
        row_to_item,
    )
    .map_err(|_| anyhow::anyhow!("No active {kind} with id {id}"))
}

pub fn update_item(conn: &Connection, item: &Item) -> Result<()> {
    let tags_json = serde_json::to_string(&item.tags)?;
    conn.execute(
        "UPDATE items SET title=?2, url=?3, project=?4, tags_json=?5, path=?6, summary=?7, body=?8, modified=?9
         WHERE uuid=?1",
        rusqlite::params![
            item.uuid.to_string(),
            item.title,
            item.url,
            item.project,
            tags_json,
            item.path,
            item.summary,
            item.body,
            dt_to_str(&item.modified),
        ],
    )?;
    Ok(())
}

pub fn archive_item(conn: &Connection, uuid: &Uuid) -> Result<()> {
    conn.execute(
        "UPDATE items SET status='archived', modified=?2 WHERE uuid=?1",
        rusqlite::params![uuid.to_string(), dt_to_str(&Utc::now())],
    )?;
    Ok(())
}

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

pub fn upsert_embedding(conn: &Connection, ref_uuid: &Uuid, vector: &[f32]) -> Result<()> {
    let vector_json = serde_json::to_string(vector)?;
    conn.execute(
        "INSERT INTO embeddings (ref_uuid, vector_json) VALUES (?1, ?2)
         ON CONFLICT(ref_uuid) DO UPDATE SET vector_json = excluded.vector_json",
        rusqlite::params![ref_uuid.to_string(), vector_json],
    )?;
    Ok(())
}

pub fn all_embeddings(conn: &Connection) -> Result<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare("SELECT ref_uuid, vector_json FROM embeddings")?;
    let rows = stmt.query_map([], |r| {
        let uuid: String = r.get(0)?;
        let json: String = r.get(1)?;
        let vec: Vec<f32> = serde_json::from_str(&json).unwrap_or_default();
        Ok((uuid, vec))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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

// ── AI run audit trail ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AiRun {
    pub id: i64,
    pub kind: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub created_at: DateTime<Utc>,
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
) -> Result<i64> {
    conn.execute(
        "INSERT INTO task_ai_runs (task_uuid, kind, model, provider, prompt, response_json, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            task_uuid.to_string(),
            kind,
            model,
            provider,
            prompt,
            response_json,
            dt_to_str(&Utc::now()),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_ai_runs(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<AiRun>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, model, provider, created_at FROM task_ai_runs
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

// ── project env commands ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ProjectCommands {
    pub setup_cmd: Option<String>,
    pub test_cmd: Option<String>,
    pub lint_cmd: Option<String>,
    pub run_cmd: Option<String>,
}

pub fn get_project_commands(conn: &Connection, name: &str) -> Result<ProjectCommands> {
    conn.query_row(
        "SELECT setup_cmd, test_cmd, lint_cmd, run_cmd FROM projects WHERE name=?1",
        [name],
        |r| {
            Ok(ProjectCommands {
                setup_cmd: r.get(0)?,
                test_cmd: r.get(1)?,
                lint_cmd: r.get(2)?,
                run_cmd: r.get(3)?,
            })
        },
    )
    .optional()
    .map(|o| o.unwrap_or_default())
    .map_err(Into::into)
}

pub fn set_project_commands(conn: &Connection, name: &str, cmds: &ProjectCommands) -> Result<()> {
    conn.execute(
        "INSERT INTO projects (name, setup_cmd, test_cmd, lint_cmd, run_cmd, last_seen)
         VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(name) DO UPDATE SET
            setup_cmd = COALESCE(?2, setup_cmd),
            test_cmd  = COALESCE(?3, test_cmd),
            lint_cmd  = COALESCE(?4, lint_cmd),
            run_cmd   = COALESCE(?5, run_cmd)",
        params![
            name,
            cmds.setup_cmd,
            cmds.test_cmd,
            cmds.lint_cmd,
            cmds.run_cmd,
            dt_to_str(&Utc::now()),
        ],
    )?;
    Ok(())
}

// ── GitHub sync settings ─────────────────────────────────────────────────────

/// Non-secret GitHub sync identity for a project.
/// Contains a repo full_name, the authenticated login, and the sync scope.
/// No PAT or credential is stored — authentication is always resolved at
/// runtime (e.g. from `gh auth status` or the environment).
#[derive(Debug, Clone, Default)]
pub struct GithubSyncSettings {
    /// GitHub full repository name (owner/repo).
    pub repo: Option<String>,
    /// GitHub username (login) associated with the sync, not a token.
    pub login: Option<String>,
    /// Comma-separated sync scopes, e.g. "issues" or "issues,prs".
    pub scope: Option<String>,
}

/// Persist GitHub sync identity for a project.  Only the three non-secret
/// fields (repo, login, scope) are written; no token is accepted.
pub fn set_github_sync(conn: &Connection, project: &str, s: &GithubSyncSettings) -> Result<()> {
    conn.execute(
        "INSERT INTO projects (name, github_repo, github_login, github_sync_scope, last_seen)
         VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(name) DO UPDATE SET
           github_repo       = COALESCE(?2, github_repo),
           github_login      = COALESCE(?3, github_login),
           github_sync_scope = COALESCE(?4, github_sync_scope),
           last_seen         = ?5",
        params![project, s.repo, s.login, s.scope, dt_to_str(&Utc::now()),],
    )?;
    Ok(())
}

/// Load GitHub sync identity for a project from the projects table.
pub fn get_github_sync(conn: &Connection, project: &str) -> Result<GithubSyncSettings> {
    conn.query_row(
        "SELECT github_repo, github_login, github_sync_scope FROM projects WHERE name=?1",
        [project],
        |r| {
            Ok(GithubSyncSettings {
                repo: r.get(0)?,
                login: r.get(1)?,
                scope: r.get(2)?,
            })
        },
    )
    .optional()
    .map(|o| o.unwrap_or_default())
    .map_err(Into::into)
}

// ── GitHub issue provenance (stored in tasks.meta_json["github"]) ─────────────

/// Read the GitHub provenance embedded in a task's `meta_json`, if any.
pub fn get_github_provenance(
    conn: &Connection,
    task_uuid: &Uuid,
) -> Result<Option<crate::model::GithubProvenance>> {
    let fields = get_guide_fields(conn, task_uuid)?;
    let Some(raw) = fields.meta_json else {
        return Ok(None);
    };
    let obj: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
    let prov = obj
        .get("github")
        .and_then(|v| serde_json::from_value::<crate::model::GithubProvenance>(v.clone()).ok());
    Ok(prov)
}

/// Write (or replace) the GitHub provenance inside a task's `meta_json`.
/// Merges with any existing keys so other meta_json entries are preserved.
/// No token or secret is accepted by the type — `GithubProvenance` contains
/// only repo name, number, timestamp and login.
pub fn set_github_provenance(
    conn: &Connection,
    task_uuid: &Uuid,
    prov: &crate::model::GithubProvenance,
) -> Result<()> {
    let fields = get_guide_fields(conn, task_uuid)?;
    let mut obj: serde_json::Map<String, serde_json::Value> = fields
        .meta_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    obj.insert(
        "github".to_string(),
        serde_json::to_value(prov).unwrap_or(serde_json::Value::Null),
    );
    let json = serde_json::to_string(&obj)?;
    set_meta_json(conn, task_uuid, &json)
}

/// Transitive set of tasks `task_uuid` depends on (its blockers, recursively),
/// returned blockers-first so a briefing reads in execution order.
pub fn dependency_closure(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Uuid>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE deps(uuid, depth) AS (
            SELECT ?1, 0
            UNION
            SELECT d.depends_on_uuid, deps.depth + 1
              FROM dependencies d JOIN deps ON d.task_uuid = deps.uuid
         )
         SELECT uuid FROM deps GROUP BY uuid ORDER BY MAX(depth) DESC",
    )?;
    let rows = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        apply_migrations(&mut conn).unwrap();
        conn
    }

    fn seed_task(conn: &Connection) -> Task {
        let mut task = Task::new("demo".into(), "tk".into());
        insert_task(conn, &mut task).unwrap();
        task
    }

    #[test]
    fn set_task_files_defaults_to_manual_source() {
        let conn = mem();
        let task = seed_task(&conn);
        set_task_files(&conn, &task.uuid, &["a.rs".into(), "b.rs".into()]).unwrap();
        let sourced = get_task_files_sourced(&conn, &task.uuid).unwrap();
        assert!(sourced.iter().all(|(_, s)| s == SOURCE_MANUAL));
        assert_eq!(sourced.len(), 2);
    }

    #[test]
    fn sourced_files_round_trip_and_split() {
        let conn = mem();
        let task = seed_task(&conn);
        set_task_files_sourced(
            &conn,
            &task.uuid,
            &[
                ("Cargo.toml".into(), SOURCE_MANUAL.into()),
                (".gitignore".into(), SOURCE_MANUAL.into()),
                ("src/llm/mod.rs".into(), SOURCE_SUGGESTED.into()),
            ],
        )
        .unwrap();

        let sourced = get_task_files_sourced(&conn, &task.uuid).unwrap();
        let manual: Vec<_> = sourced
            .iter()
            .filter(|(_, s)| s == SOURCE_MANUAL)
            .map(|(p, _)| p.clone())
            .collect();
        let suggested: Vec<_> = sourced
            .iter()
            .filter(|(_, s)| s == SOURCE_SUGGESTED)
            .map(|(p, _)| p.clone())
            .collect();
        assert_eq!(manual.len(), 2);
        assert_eq!(suggested, vec!["src/llm/mod.rs".to_string()]);
    }

    #[test]
    fn project_names_unions_tasks_and_profiles_sorted() {
        let conn = mem();
        let mut t = Task::new("x".into(), "alpha".into());
        insert_task(&conn, &mut t).unwrap();
        upsert_project_seen(&conn, "beta", Some("/p/beta")).unwrap();

        let names = project_names(&conn).unwrap();
        assert!(names.contains(&"alpha".to_string()), "{names:?}");
        assert!(names.contains(&"beta".to_string()), "{names:?}");
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "project_names should be sorted");
    }

    #[test]
    fn get_project_by_path_finds_registered_project() {
        let conn = mem();
        upsert_project_seen(&conn, "cardpsp-workspace", Some("/home/u/workspace")).unwrap();
        let found = get_project_by_path(&conn, "/home/u/workspace").unwrap();
        assert_eq!(found.map(|p| p.name), Some("cardpsp-workspace".to_string()));
        assert!(get_project_by_path(&conn, "/elsewhere").unwrap().is_none());
    }

    #[test]
    fn get_project_by_path_prefers_most_recently_seen_on_collision() {
        let conn = mem();
        upsert_project_seen(&conn, "stale", Some("/home/u/workspace")).unwrap();
        upsert_project_seen(&conn, "current", Some("/home/u/workspace")).unwrap();
        // Force deterministic ordering regardless of timestamp resolution.
        conn.execute(
            "UPDATE projects SET last_seen='2020-01-01T00:00:00Z' WHERE name='stale'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE projects SET last_seen='2030-01-01T00:00:00Z' WHERE name='current'",
            [],
        )
        .unwrap();
        let found = get_project_by_path(&conn, "/home/u/workspace").unwrap();
        assert_eq!(found.map(|p| p.name), Some("current".to_string()));
    }

    #[test]
    fn adding_annotation_records_a_history_event() {
        let conn = mem();
        let task = seed_task(&conn);
        add_annotation(&conn, &task.uuid, "This is a test comment").unwrap();

        let history = get_history(&conn, &task.uuid).unwrap();
        let ann: Vec<_> = history.iter().filter(|h| h.field == "annotation").collect();
        assert_eq!(ann.len(), 1);
        assert_eq!(ann[0].new_value.as_deref(), Some("This is a test comment"));
        assert!(ann[0].old_value.is_none());
    }

    #[test]
    fn deleting_annotation_records_a_removal_event() {
        let conn = mem();
        let task = seed_task(&conn);
        add_annotation(&conn, &task.uuid, "temp note").unwrap();
        let anns = get_annotations(&conn, &task.uuid).unwrap();
        assert_eq!(anns.len(), 1);

        delete_annotation(&conn, anns[0].id).unwrap();

        let history = get_history(&conn, &task.uuid).unwrap();
        let removals: Vec<_> = history
            .iter()
            .filter(|h| h.field == "annotation" && h.new_value.is_none())
            .collect();
        assert_eq!(removals.len(), 1);
        assert_eq!(removals[0].old_value.as_deref(), Some("temp note"));
    }

    #[test]
    fn reset_project_nukes_tasks_children_and_profile() {
        let mut conn = mem();
        let task = seed_task(&conn);
        // Attach children that should cascade away.
        set_task_files(&conn, &task.uuid, &["src/main.rs".into()]).unwrap();
        add_link(&conn, &task.uuid, "https://example.com", None).unwrap();
        add_annotation(&conn, &task.uuid, "a note").unwrap();
        save_project_profile(
            &conn,
            &crate::model::Project {
                name: "tk".into(),
                path: None,
                goal: Some("g".into()),
                stack: None,
                conventions: None,
                notes: None,
                initialized_at: None,
                last_seen: None,
                github_repo: None,
                github_login: None,
                github_sync_scope: None,
            },
        )
        .unwrap();

        assert_eq!(count_project_tasks(&conn, "tk").unwrap(), 1);
        let deleted = reset_project(&mut conn, "tk").unwrap();
        assert_eq!(deleted, 1);

        assert_eq!(count_project_tasks(&conn, "tk").unwrap(), 0);
        assert!(get_project(&conn, "tk").unwrap().is_none());
        assert!(get_task_files(&conn, &task.uuid).unwrap().is_empty());
        assert!(get_links(&conn, &task.uuid).unwrap().is_empty());
        assert!(get_annotations(&conn, &task.uuid).unwrap().is_empty());
    }

    #[test]
    fn github_pr_url_gets_nice_label() {
        assert_eq!(
            derive_link_label("https://github.com/acme/widgets/pull/42"),
            Some("PR #42 · acme/widgets".to_string())
        );
        assert_eq!(
            derive_link_label("https://github.com/acme/widgets/issues/7"),
            Some("Issue #7 · acme/widgets".to_string())
        );
        assert_eq!(derive_link_label("https://example.com/foo"), None);
    }

    #[test]
    fn is_url_detects_links_vs_paths() {
        assert!(is_url("https://github.com/a/b/pull/1"));
        assert!(is_url("http://example.com"));
        assert!(is_url("www.test.dk"));
        assert!(!is_url("src/main.rs"));
        assert!(!is_url("Cargo.toml"));
    }

    #[test]
    fn add_and_get_links_with_history() {
        let conn = mem();
        let task = seed_task(&conn);
        add_link(
            &conn,
            &task.uuid,
            "https://github.com/acme/widgets/pull/42",
            None,
        )
        .unwrap();
        let links = get_links(&conn, &task.uuid).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].display(), "PR #42 · acme/widgets");

        // History event recorded for the added link.
        let history = get_history(&conn, &task.uuid).unwrap();
        assert!(
            history
                .iter()
                .any(|h| h.field == "link"
                    && h.new_value.as_deref() == Some("PR #42 · acme/widgets"))
        );
    }

    #[test]
    fn delete_link_records_removal_history() {
        let conn = mem();
        let task = seed_task(&conn);
        add_link(&conn, &task.uuid, "https://example.com/x", Some("My link")).unwrap();
        let links = get_links(&conn, &task.uuid).unwrap();
        assert!(delete_link(&conn, links[0].id).unwrap());
        assert!(get_links(&conn, &task.uuid).unwrap().is_empty());

        let history = get_history(&conn, &task.uuid).unwrap();
        assert!(
            history
                .iter()
                .any(|h| h.field == "link" && h.old_value.as_deref() == Some("My link"))
        );
    }

    #[test]
    fn undo_reverts_a_completed_task_to_pending() {
        let conn = mem();
        let mut task = seed_task(&conn);

        begin_undo_batch("done 1");
        task.status = Status::Completed;
        task.end = Some(Utc::now());
        task.modified = Utc::now();
        update_task(&conn, &task).unwrap();

        // Task is now completed and no longer pending.
        assert!(get_task_by_id(&conn, 1).unwrap().is_none());

        let undone = undo(&conn).unwrap();
        assert_eq!(undone.as_deref(), Some("done 1"));

        let restored = get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
            .unwrap()
            .unwrap();
        assert_eq!(restored.status, Status::Pending);
        assert!(restored.end.is_none());
    }

    #[test]
    fn undo_removes_a_newly_added_task() {
        let conn = mem();
        begin_undo_batch("add demo");
        let mut task = Task::new("demo".into(), "tk".into());
        insert_task(&conn, &mut task).unwrap();
        assert!(
            get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
                .unwrap()
                .is_some()
        );

        let undone = undo(&conn).unwrap();
        assert_eq!(undone.as_deref(), Some("add demo"));
        assert!(
            get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn undo_with_empty_log_returns_none() {
        let conn = mem();
        assert!(undo(&conn).unwrap().is_none());
    }

    #[test]
    fn undo_only_reverts_the_latest_command() {
        let conn = mem();
        let mut task = seed_task(&conn);

        begin_undo_batch("modify 1");
        task.description = "first edit".into();
        task.modified = Utc::now();
        update_task(&conn, &task).unwrap();

        begin_undo_batch("modify 1 again");
        task.description = "second edit".into();
        task.modified = Utc::now();
        update_task(&conn, &task).unwrap();

        undo(&conn).unwrap();
        let after_first_undo = get_task_by_id(&conn, 1).unwrap().unwrap();
        assert_eq!(after_first_undo.description, "first edit");

        undo(&conn).unwrap();
        let after_second_undo = get_task_by_id(&conn, 1).unwrap().unwrap();
        assert_eq!(after_second_undo.description, "demo");
    }

    #[test]
    fn set_task_files_sourced_replaces_previous() {
        let conn = mem();
        let task = seed_task(&conn);
        set_task_files_sourced(
            &conn,
            &task.uuid,
            &[("x.rs".into(), SOURCE_SUGGESTED.into())],
        )
        .unwrap();
        set_task_files_sourced(&conn, &task.uuid, &[("y.rs".into(), SOURCE_MANUAL.into())])
            .unwrap();
        let sourced = get_task_files_sourced(&conn, &task.uuid).unwrap();
        assert_eq!(
            sourced,
            vec![("y.rs".to_string(), SOURCE_MANUAL.to_string())]
        );
    }

    fn seed_named_task(conn: &Connection, desc: &str) -> Task {
        let mut task = Task::new(desc.into(), "demo".into());
        insert_task(conn, &mut task).unwrap();
        task
    }

    // ── steps / acceptance criteria ─────────────────────────────────────────

    #[test]
    fn add_step_stores_full_metadata_and_get_steps_filters_by_kind() {
        let conn = mem();
        let task = seed_task(&conn);
        add_step(
            &conn,
            &task.uuid,
            "wire the parser",
            Some("parse the plan JSON"),
            STEP_KIND_STEP,
            "ai",
            Some("cargo test"),
        )
        .unwrap();
        add_step(
            &conn,
            &task.uuid,
            "it compiles",
            None,
            STEP_KIND_ACCEPTANCE,
            "human",
            None,
        )
        .unwrap();

        let steps = get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap();
        assert_eq!(steps.len(), 1);
        let s = &steps[0];
        assert_eq!(s.text, "wire the parser");
        assert_eq!(s.intent.as_deref(), Some("parse the plan JSON"));
        assert_eq!(s.kind, STEP_KIND_STEP);
        assert_eq!(s.source, "ai");
        assert_eq!(s.verify_cmd.as_deref(), Some("cargo test"));
        assert!(!s.done);

        let acc = get_steps(&conn, &task.uuid, STEP_KIND_ACCEPTANCE).unwrap();
        assert_eq!(acc.len(), 1);
        assert_eq!(acc[0].text, "it compiles");
    }

    #[test]
    fn steps_get_sequential_positions_and_index_lookup_is_one_based() {
        let conn = mem();
        let task = seed_task(&conn);
        for t in ["first", "second", "third"] {
            add_step(&conn, &task.uuid, t, None, STEP_KIND_STEP, "human", None).unwrap();
        }
        let steps = get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap();
        assert_eq!(
            steps.iter().map(|s| s.position).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        let id2 = step_id_by_index(&conn, &task.uuid, STEP_KIND_STEP, 2).unwrap();
        assert_eq!(id2, steps[1].id);
        assert!(step_id_by_index(&conn, &task.uuid, STEP_KIND_STEP, 99).is_err());
    }

    #[test]
    fn set_step_done_records_result_and_commit_then_undone_clears_them() {
        let conn = mem();
        let task = seed_task(&conn);
        let id = add_step(
            &conn,
            &task.uuid,
            "do it",
            None,
            STEP_KIND_STEP,
            "human",
            None,
        )
        .unwrap();

        set_step_done(&conn, id, true, Some("all green"), Some("abc1234")).unwrap();
        let s = &get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap()[0];
        assert!(s.done);
        assert_eq!(s.result.as_deref(), Some("all green"));
        assert_eq!(s.done_commit.as_deref(), Some("abc1234"));
        assert!(s.done_at.is_some());

        set_step_done(&conn, id, false, None, None).unwrap();
        let s = &get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap()[0];
        assert!(!s.done);
        assert!(s.done_commit.is_none());
        assert!(s.done_at.is_none());
        // Result is preserved across reopen (COALESCE only writes, never clears it).
        assert_eq!(s.result.as_deref(), Some("all green"));
    }

    // ── code anchors ────────────────────────────────────────────────────────

    #[test]
    fn add_task_file_upserts_anchor_metadata() {
        let conn = mem();
        let task = seed_task(&conn);
        add_task_file(
            &conn,
            &task.uuid,
            "src/db.rs",
            SOURCE_SUGGESTED,
            Some("initial reason"),
            None,
            None,
            None,
        )
        .unwrap();
        // Same path again: ON CONFLICT updates in place, no duplicate row.
        add_task_file(
            &conn,
            &task.uuid,
            "src/db.rs",
            SOURCE_MANUAL,
            Some("better reason"),
            Some("add_step"),
            Some(10),
            Some(57),
        )
        .unwrap();

        let anchors = get_task_anchors(&conn, &task.uuid).unwrap();
        assert_eq!(anchors.len(), 1);
        let a = &anchors[0];
        assert_eq!(a.source, SOURCE_MANUAL);
        assert_eq!(a.reason.as_deref(), Some("better reason"));
        assert_eq!(a.symbol.as_deref(), Some("add_step"));
        assert_eq!((a.line_start, a.line_end), (Some(10), Some(57)));
        assert_eq!(a.location(), " :: add_step (10-57)");
    }

    #[test]
    fn anchor_location_formats_partial_ranges() {
        let single = Anchor {
            path: "x".into(),
            source: SOURCE_MANUAL.into(),
            reason: None,
            symbol: None,
            line_start: Some(42),
            line_end: None,
        };
        assert_eq!(single.location(), " (L42)");

        let bare = Anchor {
            path: "x".into(),
            source: SOURCE_MANUAL.into(),
            reason: None,
            symbol: Some("foo".into()),
            line_start: None,
            line_end: None,
        };
        assert_eq!(bare.location(), " :: foo");
    }

    // ── guide fields ────────────────────────────────────────────────────────

    #[test]
    fn guide_fields_round_trip() {
        let conn = mem();
        let task = seed_task(&conn);
        set_assignment(&conn, &task.uuid, "the original prompt").unwrap();
        set_rationale(&conn, &task.uuid, "because reasons").unwrap();
        set_validated(&conn, &task.uuid, "deadbeef").unwrap();
        set_meta_json(&conn, &task.uuid, r#"{"k":1}"#).unwrap();

        let g = get_guide_fields(&conn, &task.uuid).unwrap();
        assert_eq!(g.assignment.as_deref(), Some("the original prompt"));
        assert_eq!(g.rationale.as_deref(), Some("because reasons"));
        assert_eq!(g.validated_commit.as_deref(), Some("deadbeef"));
        assert!(g.validated_at.is_some());
        assert_eq!(g.meta_json.as_deref(), Some(r#"{"k":1}"#));
    }

    // ── AI run audit trail ──────────────────────────────────────────────────

    #[test]
    fn ai_runs_are_recorded_and_returned_in_order() {
        let conn = mem();
        let task = seed_task(&conn);
        let r1 = record_ai_run(
            &conn,
            &task.uuid,
            "enrich",
            Some("opus"),
            Some("azure"),
            Some("prompt"),
            Some("{}"),
        )
        .unwrap();
        let r2 = record_ai_run(&conn, &task.uuid, "refine", None, None, None, None).unwrap();
        assert!(r2 > r1);

        let runs = get_ai_runs(&conn, &task.uuid).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].kind, "enrich");
        assert_eq!(runs[0].model.as_deref(), Some("opus"));
        assert_eq!(runs[1].kind, "refine");
        assert!(runs[1].model.is_none());
    }

    // ── feedback lifecycle ──────────────────────────────────────────────────

    #[test]
    fn open_feedback_lists_comments_flagged_first_and_resolves() {
        let conn = mem();
        let task = seed_task(&conn);
        // A plain comment, a flagged comment, and a non-comment note.
        add_annotation_full(
            &conn, &task.uuid, "plain", "comment", "human", None, None, false,
        )
        .unwrap();
        let flagged = add_annotation_full(
            &conn,
            &task.uuid,
            "reconsider this",
            "comment",
            "human",
            Some("step"),
            Some("2"),
            true,
        )
        .unwrap();
        add_annotation_full(
            &conn,
            &task.uuid,
            "a finding",
            "finding",
            "ai",
            None,
            None,
            false,
        )
        .unwrap();

        let open = get_open_feedback(&conn, &task.uuid).unwrap();
        assert_eq!(open.len(), 2, "only open comments count as feedback");
        assert_eq!(
            open[0].text, "reconsider this",
            "flagged feedback sorts first"
        );

        // Resolving links the run and drops it from the open set.
        assert!(resolve_annotation(&conn, flagged, Some(7)).unwrap());
        let open = get_open_feedback(&conn, &task.uuid).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].text, "plain");
    }

    // ── cross-task FTS memory ───────────────────────────────────────────────

    #[test]
    fn search_fts_matches_tasks_notes_and_anchors() {
        let conn = mem();
        let task = seed_named_task(&conn, "implement frobnicator widget");
        add_annotation_full(
            &conn,
            &task.uuid,
            "the frobnicator caches results",
            "finding",
            "ai",
            None,
            None,
            false,
        )
        .unwrap();
        add_task_file(
            &conn,
            &task.uuid,
            "src/frob.rs",
            SOURCE_MANUAL,
            Some("frobnicator lives here"),
            None,
            None,
            None,
        )
        .unwrap();

        let kinds: std::collections::HashSet<String> = search_fts(&conn, "frobnicator", 50)
            .unwrap()
            .into_iter()
            .map(|h| h.ref_kind)
            .collect();
        assert!(kinds.contains("task"));
        assert!(kinds.contains("note"));
        assert!(kinds.contains("anchor"));
    }

    #[test]
    fn search_fts_tolerates_quotes_in_query() {
        let conn = mem();
        let task = seed_named_task(&conn, "handle the \"weird\" input");
        // A query containing a double-quote must not blow up the FTS parser.
        let hits = search_fts(&conn, "\"weird\" input", 10).unwrap();
        assert!(hits.iter().any(|h| h.task_uuid == task.uuid.to_string()));
    }

    // ── dependency closure ──────────────────────────────────────────────────

    #[test]
    fn dependency_closure_returns_blockers_first() {
        let conn = mem();
        // c depends on b, b depends on a  →  closure of c is [a, b, c].
        let a = seed_named_task(&conn, "a");
        let b = seed_named_task(&conn, "b");
        let c = seed_named_task(&conn, "c");
        add_dependency(&conn, &b.uuid, &a.uuid).unwrap();
        add_dependency(&conn, &c.uuid, &b.uuid).unwrap();

        let closure = dependency_closure(&conn, &c.uuid).unwrap();
        assert_eq!(closure, vec![a.uuid, b.uuid, c.uuid]);
    }

    // ── feature chain (board / detail panel) ────────────────────────────────

    #[test]
    fn feature_chain_returns_linked_tasks_blockers_first() {
        let conn = mem();
        // c → b → a (c depends on b depends on a). Queried from the middle (b),
        // the whole chain comes back in blockers-first order.
        let a = seed_named_task(&conn, "a");
        let b = seed_named_task(&conn, "b");
        let c = seed_named_task(&conn, "c");
        add_dependency(&conn, &b.uuid, &a.uuid).unwrap();
        add_dependency(&conn, &c.uuid, &b.uuid).unwrap();

        let chain: Vec<Uuid> = feature_chain(&conn, &b.uuid)
            .unwrap()
            .into_iter()
            .map(|t| t.uuid)
            .collect();
        assert_eq!(chain, vec![a.uuid, b.uuid, c.uuid]);
    }

    #[test]
    fn feature_chain_is_empty_for_standalone_task() {
        let conn = mem();
        let a = seed_named_task(&conn, "a");
        // No dependencies → no chain to draw.
        assert!(feature_chain(&conn, &a.uuid).unwrap().is_empty());
    }

    #[test]
    fn feature_chain_excludes_unrelated_tasks() {
        let conn = mem();
        let a = seed_named_task(&conn, "a");
        let b = seed_named_task(&conn, "b");
        let unrelated = seed_named_task(&conn, "unrelated");
        add_dependency(&conn, &b.uuid, &a.uuid).unwrap();

        let chain: Vec<Uuid> = feature_chain(&conn, &a.uuid)
            .unwrap()
            .into_iter()
            .map(|t| t.uuid)
            .collect();
        assert_eq!(chain, vec![a.uuid, b.uuid]);
        assert!(!chain.contains(&unrelated.uuid));
    }

    // ── project commands ────────────────────────────────────────────────────

    #[test]
    fn project_commands_round_trip_and_partial_update_preserves_others() {
        let conn = mem();
        set_project_commands(
            &conn,
            "demo",
            &ProjectCommands {
                setup_cmd: Some("cargo fetch".into()),
                test_cmd: Some("cargo test".into()),
                lint_cmd: None,
                run_cmd: None,
            },
        )
        .unwrap();
        // A partial update (only lint) must COALESCE-preserve the earlier commands.
        set_project_commands(
            &conn,
            "demo",
            &ProjectCommands {
                setup_cmd: None,
                test_cmd: None,
                lint_cmd: Some("cargo clippy".into()),
                run_cmd: None,
            },
        )
        .unwrap();

        let c = get_project_commands(&conn, "demo").unwrap();
        assert_eq!(c.setup_cmd.as_deref(), Some("cargo fetch"));
        assert_eq!(c.test_cmd.as_deref(), Some("cargo test"));
        assert_eq!(c.lint_cmd.as_deref(), Some("cargo clippy"));
        assert!(c.run_cmd.is_none());
    }

    #[test]
    fn get_project_commands_defaults_to_empty_when_absent() {
        let conn = mem();
        let c = get_project_commands(&conn, "nope").unwrap();
        assert!(c.setup_cmd.is_none() && c.test_cmd.is_none());
    }

    // ── GitHub sync settings ─────────────────────────────────────────────────

    #[test]
    fn github_sync_settings_round_trip_through_project_storage() {
        let conn = mem();
        upsert_project_seen(&conn, "myrepo", Some("/home/u/myrepo")).unwrap();
        set_github_sync(
            &conn,
            "myrepo",
            &GithubSyncSettings {
                repo: Some("acme/myrepo".into()),
                login: Some("alice".into()),
                scope: Some("issues".into()),
            },
        )
        .unwrap();

        let s = get_github_sync(&conn, "myrepo").unwrap();
        assert_eq!(s.repo.as_deref(), Some("acme/myrepo"));
        assert_eq!(s.login.as_deref(), Some("alice"));
        assert_eq!(s.scope.as_deref(), Some("issues"));
    }

    #[test]
    fn save_project_profile_persists_github_fields() {
        let conn = mem();
        save_project_profile(
            &conn,
            &crate::model::Project {
                name: "myrepo".into(),
                path: Some("/home/u/myrepo".into()),
                goal: Some("g".into()),
                stack: None,
                conventions: None,
                notes: None,
                initialized_at: None,
                last_seen: None,
                github_repo: Some("acme/myrepo".into()),
                github_login: Some("alice".into()),
                github_sync_scope: Some("issues".into()),
            },
        )
        .unwrap();

        let project = get_project(&conn, "myrepo").unwrap().unwrap();
        assert_eq!(project.github_repo.as_deref(), Some("acme/myrepo"));
        assert_eq!(project.github_login.as_deref(), Some("alice"));
        assert_eq!(project.github_sync_scope.as_deref(), Some("issues"));
    }

    #[test]
    fn github_sync_partial_update_preserves_existing_fields() {
        let conn = mem();
        upsert_project_seen(&conn, "p", None).unwrap();
        set_github_sync(
            &conn,
            "p",
            &GithubSyncSettings {
                repo: Some("org/p".into()),
                login: Some("bob".into()),
                scope: Some("issues".into()),
            },
        )
        .unwrap();
        // Update only scope — repo and login must be preserved (COALESCE).
        set_github_sync(
            &conn,
            "p",
            &GithubSyncSettings {
                repo: None,
                login: None,
                scope: Some("issues,prs".into()),
            },
        )
        .unwrap();

        let s = get_github_sync(&conn, "p").unwrap();
        assert_eq!(s.repo.as_deref(), Some("org/p"), "repo preserved");
        assert_eq!(s.login.as_deref(), Some("bob"), "login preserved");
        assert_eq!(s.scope.as_deref(), Some("issues,prs"), "scope updated");
    }

    #[test]
    fn github_sync_no_secret_field_in_settings_struct() {
        // login is a username, not a token — the type only accepts non-secret strings.
        let s = GithubSyncSettings {
            repo: Some("org/repo".into()),
            login: Some("user".into()),
            scope: Some("issues".into()),
        };
        assert!(
            !s.login.as_deref().unwrap_or("").starts_with("ghp_"),
            "login field should hold a username, not a PAT"
        );
    }

    #[test]
    fn project_detection_loads_github_sync_metadata_for_path() {
        let conn = mem();
        upsert_project_seen(&conn, "sara", Some("/home/u/Sara")).unwrap();
        set_github_sync(
            &conn,
            "sara",
            &GithubSyncSettings {
                repo: Some("acme/sara".into()),
                login: Some("alice".into()),
                scope: Some("issues".into()),
            },
        )
        .unwrap();

        // Simulates what detect_current_project returns: the project loaded by path.
        let project = get_project_by_path(&conn, "/home/u/Sara")
            .unwrap()
            .expect("project must be found by path");

        assert_eq!(project.name, "sara");
        assert_eq!(project.github_repo.as_deref(), Some("acme/sara"));
        assert_eq!(project.github_login.as_deref(), Some("alice"));
        assert_eq!(project.github_sync_scope.as_deref(), Some("issues"));
    }

    // ── GitHub issue provenance ──────────────────────────────────────────────

    #[test]
    fn github_provenance_round_trips_through_meta_json() {
        let conn = mem();
        let task = seed_task(&conn);

        let prov = crate::model::GithubProvenance {
            repo: "acme/widgets".into(),
            number: 99,
            imported_at: Utc::now(),
            imported_by: Some("alice".into()),
        };
        set_github_provenance(&conn, &task.uuid, &prov).unwrap();

        let loaded = get_github_provenance(&conn, &task.uuid)
            .unwrap()
            .expect("provenance must be present");
        assert_eq!(loaded.repo, "acme/widgets");
        assert_eq!(loaded.number, 99);
        assert_eq!(loaded.imported_by.as_deref(), Some("alice"));
    }

    #[test]
    fn github_provenance_merges_with_existing_meta_json_keys() {
        let conn = mem();
        let task = seed_task(&conn);

        // Pre-populate meta_json with some other data.
        set_meta_json(&conn, &task.uuid, r#"{"my_key":"keep_me"}"#).unwrap();

        let prov = crate::model::GithubProvenance {
            repo: "org/repo".into(),
            number: 1,
            imported_at: Utc::now(),
            imported_by: None,
        };
        set_github_provenance(&conn, &task.uuid, &prov).unwrap();

        let raw = get_guide_fields(&conn, &task.uuid)
            .unwrap()
            .meta_json
            .unwrap();
        let obj: serde_json::Value = serde_json::from_str(&raw).unwrap();
        // Both the existing key and the new github key must be present.
        assert_eq!(obj["my_key"], "keep_me");
        assert_eq!(obj["github"]["repo"], "org/repo");
        assert_eq!(obj["github"]["number"], 1);
    }

    #[test]
    fn github_provenance_contains_no_secret_fields() {
        // GithubProvenance has: repo, number, imported_at, imported_by.
        // None of these fields can hold a PAT. imported_by is a login string.
        let prov = crate::model::GithubProvenance {
            repo: "org/repo".into(),
            number: 5,
            imported_at: Utc::now(),
            imported_by: Some("bob".into()),
        };
        let serialized = serde_json::to_string(&prov).unwrap();
        // Sanity: no "token" or "pat" key appears in the serialised provenance.
        assert!(!serialized.to_lowercase().contains("token"));
        assert!(!serialized.to_lowercase().contains(r#""pat""#));
    }
}
