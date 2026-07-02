//! Shared fixtures for integration tests under `tests/`.
//!
//! Lives at `tests/common/mod.rs` (not `tests/common.rs`) so Cargo does not
//! treat it as its own test binary — only files directly under `tests/`
//! become separate test targets; subdirectories are just modules other test
//! files can `mod common;` into.

use rusqlite::Connection;
use sara_tasks::infrastructure::db::{self, open_in_memory_for_test};
use sara_tasks::infrastructure::model::Task;

/// An in-memory database with foreign keys enforced and all migrations applied.
pub fn mem() -> Connection {
    open_in_memory_for_test()
}

/// Insert and return a minimal task in project "tk".
pub fn seed_task(conn: &Connection) -> Task {
    let mut task = Task::new("demo".into(), "tk".into());
    db::insert_task(conn, &mut task).unwrap();
    task
}
