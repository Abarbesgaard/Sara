use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use rusqlite_migration::{Migrations, M};

use crate::config;
use crate::model::{Priority, Project, Status, Task};
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
    })
}

const TASK_COLUMNS: &str =
    "uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent";

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
                            entry, modified, end, tags_json, urgency, started_at, time_spent)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
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
        ],
    )?;
    Ok(())
}

pub fn get_task_by_id(conn: &Connection, id: i64) -> Result<Option<Task>> {
    let mut stmt = conn.prepare(
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent
         FROM tasks WHERE id=?1 AND status='pending' LIMIT 1",
    )?;
    let mut rows = stmt.query_map([id], row_to_task)?;
    Ok(rows.next().transpose()?)
}

pub fn get_task_by_uuid_prefix(conn: &Connection, prefix: &str) -> Result<Option<Task>> {
    let pattern = format!("{prefix}%");
    let mut stmt = conn.prepare(
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent
         FROM tasks WHERE uuid LIKE ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query_map([pattern], row_to_task)?;
    Ok(rows.next().transpose()?)
}

/// Resolve "3" (display id) or a uuid prefix to a Task
pub fn resolve_task(conn: &Connection, id_or_uuid: &str) -> Result<Task> {
    if let Ok(n) = id_or_uuid.parse::<i64>() {
        if let Some(t) = get_task_by_id(conn, n)? {
            return Ok(t);
        }
    }
    if let Some(t) = get_task_by_uuid_prefix(conn, id_or_uuid)? {
        return Ok(t);
    }
    Err(anyhow::anyhow!("No pending task with id or uuid matching '{id_or_uuid}'"))
}

pub fn list_tasks(conn: &Connection, project: Option<&str>) -> Result<Vec<Task>> {
    let sql = if project.is_some() {
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent
         FROM tasks WHERE status='pending' AND project=?1 ORDER BY urgency DESC"
    } else {
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent
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

pub fn update_task(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET description=?1, project=?2, status=?3, priority=?4, due=?5,
                         modified=?6, end=?7, tags_json=?8, urgency=?9,
                         started_at=?10, time_spent=?11
         WHERE uuid=?12",
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
            task.uuid.to_string(),
        ],
    )?;
    Ok(())
}

/// After completing a task, compact pending IDs to stay small
pub fn repack_ids(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT uuid FROM tasks WHERE status='pending' ORDER BY entry ASC",
    )?;
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
    conn.execute(
        "INSERT OR IGNORE INTO dependencies (task_uuid, depends_on_uuid) VALUES (?1,?2)",
        params![task_uuid.to_string(), dep_uuid.to_string()],
    )?;
    Ok(())
}

pub fn remove_dependency(conn: &Connection, task_uuid: &Uuid, dep_uuid: &Uuid) -> Result<()> {
    conn.execute(
        "DELETE FROM dependencies WHERE task_uuid=?1 AND depends_on_uuid=?2",
        params![task_uuid.to_string(), dep_uuid.to_string()],
    )?;
    Ok(())
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
        let mut stmt = conn.prepare(
            "SELECT depends_on_uuid FROM dependencies WHERE task_uuid=?1",
        )?;
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
    let mut stmt = conn.prepare(
        "SELECT task_uuid FROM dependencies WHERE depends_on_uuid=?1",
    )?;
    let uuids = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(uuids)
}

// ── task files ───────────────────────────────────────────────────────────────

pub fn set_task_files(conn: &Connection, task_uuid: &Uuid, paths: &[String]) -> Result<()> {
    conn.execute(
        "DELETE FROM task_files WHERE task_uuid=?1",
        [task_uuid.to_string()],
    )?;
    for path in paths {
        conn.execute(
            "INSERT OR IGNORE INTO task_files (task_uuid, path) VALUES (?1,?2)",
            params![task_uuid.to_string(), path],
        )?;
    }
    Ok(())
}

pub fn get_task_files(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT path FROM task_files WHERE task_uuid=?1 ORDER BY path")?;
    let paths = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(paths)
}

// ── annotations ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Annotation {
    pub id: i64,
    pub text: String,
    pub entry: DateTime<Utc>,
}

pub fn add_annotation(conn: &Connection, task_uuid: &Uuid, text: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO annotations (task_uuid, text, entry) VALUES (?1,?2,?3)",
        params![task_uuid.to_string(), text, dt_to_str(&Utc::now())],
    )?;
    Ok(())
}

pub fn get_annotations(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Annotation>> {
    let mut stmt = conn.prepare(
        "SELECT id, text, entry FROM annotations WHERE task_uuid=?1 ORDER BY entry ASC",
    )?;
    let anns = stmt
        .query_map([task_uuid.to_string()], |row| {
            let entry_str: String = row.get(2)?;
            Ok(Annotation {
                id: row.get(0)?,
                text: row.get(1)?,
                entry: str_to_dt(&entry_str).unwrap_or_else(|_| Utc::now()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(anns)
}

pub fn delete_annotation(conn: &Connection, ann_id: i64) -> Result<bool> {
    let n = conn.execute("DELETE FROM annotations WHERE id=?1", [ann_id])?;
    Ok(n > 0)
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
        "INSERT INTO projects (name, path, goal, stack, conventions, notes, initialized_at, last_seen)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
         ON CONFLICT(name) DO UPDATE SET
           path          = COALESCE(?2, path),
           goal          = COALESCE(?3, goal),
           stack         = COALESCE(?4, stack),
           conventions   = COALESCE(?5, conventions),
           notes         = COALESCE(?6, notes),
           initialized_at = COALESCE(?7, initialized_at),
           last_seen      = ?7",
        params![
            project.name,
            project.path,
            project.goal,
            project.stack,
            project.conventions,
            project.notes,
            now,
        ],
    )?;
    Ok(())
}

pub fn get_project(conn: &Connection, name: &str) -> Result<Option<Project>> {
    let mut stmt = conn.prepare(
        "SELECT name,path,goal,stack,conventions,notes,initialized_at,last_seen
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
            initialized_at: None,
            last_seen: None,
        })
    })?;
    Ok(rows.next().transpose()?)
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
            "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent
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
