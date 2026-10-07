use crate::infrastructure::db::*;
use crate::infrastructure::model::{Item, Status, Task};
use chrono::{TimeZone as _, Utc};
use rusqlite::Connection;

mod annotations;
mod checklist;
mod follow;
mod github;
mod guide;
mod links;
mod memory;
mod memory_links;
mod memory_maintenance;
mod memory_strength;
mod memory_uses;
mod migrations;
mod projects;
mod tasks;

pub(super) fn mem() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    super::migrations::apply_migrations(&mut conn).unwrap();
    conn
}

pub(super) fn projects_columns(conn: &Connection) -> std::collections::HashSet<String> {
    conn.prepare("PRAGMA table_info(projects)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

pub(super) fn seed_task(conn: &Connection) -> Task {
    let mut task = Task::new("demo".into(), "tk".into());
    insert_task(conn, &mut task).unwrap();
    task
}

pub(super) fn seed_named_task(conn: &Connection, desc: &str) -> Task {
    let mut task = Task::new(desc.into(), "demo".into());
    insert_task(conn, &mut task).unwrap();
    task
}

pub(super) fn make_completed_task(conn: &Connection, description: &str, file: &str) -> Task {
    let mut task = Task::new(description.to_string(), "Sara".to_string());
    task.status = Status::Completed;
    insert_task(conn, &mut task).unwrap();
    set_task_files(conn, &task.uuid, &[file.to_string()]).unwrap();
    task
}

pub(super) fn make_gh_comment(
    id: i64,
    author: &str,
    body: &str,
) -> crate::infrastructure::model::GithubComment {
    crate::infrastructure::model::GithubComment {
        comment_id: id,
        author: author.to_string(),
        body: body.to_string(),
        url: format!("https://github.com/a/b/issues/1#issuecomment-{id}"),
        created_at: Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 6, 2, 0, 0, 0).unwrap(),
    }
}

pub(super) fn make_memory(title: &str, tags: &[&str]) -> Item {
    let mut item = Item::new_note(title.to_string(), "body".to_string());
    item.kind = "memory".to_string();
    item.path = Some(String::new());
    item.tags = tags.iter().map(|t| t.to_string()).collect();
    item
}
