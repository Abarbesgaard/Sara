use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::infrastructure::config;
use crate::infrastructure::model::{Priority, Status, Task};
use chrono::{DateTime, Utc};
use uuid::Uuid;

mod activity;
mod annotations;
mod branch;
mod checklist;
mod github;
mod guide;
mod links;
mod memory;
mod meta;
mod migrations;
mod projects;
mod tasks;
pub use activity::*;
pub use annotations::*;
pub use branch::*;
pub use checklist::*;
pub use github::*;
pub use guide::*;
pub use links::*;
pub use memory::*;
pub use meta::*;
pub use projects::*;
pub use tasks::*;

pub fn open() -> Result<Connection> {
    let path = config::db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(&path)
        .with_context(|| format!("Failed to open database: {}", path.display()))?;

    set_pragmas(&conn)?;
    migrations::apply_migrations(&mut conn)?;
    Ok(conn)
}

fn set_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=5000;",
    )?;
    Ok(())
}

#[cfg(test)]
pub fn open_in_memory_for_test() -> Connection {
    let mut conn = Connection::open_in_memory().expect("open in-memory db");
    set_pragmas(&conn).expect("set pragmas");
    migrations::apply_migrations(&mut conn).expect("apply migrations");
    conn
}

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

fn with_write_lock<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        return f();
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match f() {
        Ok(value) => {
            conn.execute_batch("COMMIT")?;
            Ok(value)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

fn fold_tag(tag: &str) -> String {
    tag.trim().to_lowercase()
}

fn atomically<T>(conn: &Connection, name: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    conn.execute_batch(&format!("SAVEPOINT {name}"))?;
    match f() {
        Ok(value) => {
            conn.execute_batch(&format!("RELEASE {name}"))?;
            Ok(value)
        }
        Err(e) => {
            let _ = conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
            Err(e)
        }
    }
}

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
#[path = "../../../tests/unit/infrastructure/db/tests/mod.rs"]
mod tests;
