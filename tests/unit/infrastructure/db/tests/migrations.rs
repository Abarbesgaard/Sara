//! Unit tests for db::migrations.

use super::*;
use crate::infrastructure::db::*;
use uuid::Uuid;

#[test]
fn fresh_database_has_github_sync_columns() {
    let conn = mem();
    let cols = projects_columns(&conn);
    for col in ["github_repo", "github_login", "github_sync_scope"] {
        assert!(cols.contains(col), "fresh DB missing column {col}");
    }
}

#[test]
fn in_memory_test_db_enforces_foreign_keys() {
    // Regression for PR #58 review: open_in_memory_for_test must set the same
    // PRAGMAs as open(), notably foreign_keys=ON, so FK enforcement / cascade
    // behaviour matches production. A child row referencing a non-existent
    // task must be rejected.
    let conn = open_in_memory_for_test();
    let missing = Uuid::new_v4();
    let res = add_link(&conn, &missing, "https://example.com/pr/1", None);
    assert!(
        res.is_err(),
        "foreign_keys should be ON: linking to a non-existent task must fail"
    );
}

#[test]
fn appended_migration_backfills_github_columns_on_upgraded_db() {
    // Reproduce a database that upgraded across the point where the GitHub
    // sync columns migration was inserted mid-list: the projects table
    // predates those columns, yet user_version is already at the old list
    // length (14), so rusqlite_migration would otherwise skip the inserted
    // migration forever and the schema would stay broken.
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE projects (
                name TEXT PRIMARY KEY, path TEXT, goal TEXT, stack TEXT,
                conventions TEXT, notes TEXT, initialized_at TEXT, last_seen TEXT,
                setup_cmd TEXT, test_cmd TEXT, lint_cmd TEXT, run_cmd TEXT
            );
            CREATE TABLE items (
                uuid TEXT PRIMARY KEY, kind TEXT NOT NULL, display_id INTEGER,
                title TEXT NOT NULL, url TEXT, project TEXT,
                tags_json TEXT NOT NULL DEFAULT '[]', path TEXT NOT NULL,
                summary TEXT, body TEXT NOT NULL DEFAULT '',
                created TEXT NOT NULL, modified TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active'
            );
            CREATE TABLE task_ai_runs (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid     TEXT NOT NULL,
                kind          TEXT NOT NULL,
                model         TEXT,
                provider      TEXT,
                prompt        TEXT,
                response_json TEXT,
                created_at    TEXT NOT NULL
            );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (name, path) VALUES ('demo', '/tmp/demo')",
        [],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 14_i64).unwrap();
    assert!(!projects_columns(&conn).contains("github_repo"));

    super::super::migrations::apply_migrations(&mut conn).unwrap();

    let cols = projects_columns(&conn);
    for col in ["github_repo", "github_login", "github_sync_scope"] {
        assert!(cols.contains(col), "backfill missing column {col}");
    }
    // Existing rows survive the backfill and re-running is a no-op.
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    super::super::migrations::apply_migrations(&mut conn).unwrap();
}
