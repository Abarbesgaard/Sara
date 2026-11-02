use rusqlite::Connection;

use crate::domain::Task;
use crate::infrastructure::db;

pub(super) fn find_duplicate_open_task(
    conn: &Connection,
    project: &str,
    description: &str,
) -> Option<Task> {
    let want = description.trim().to_lowercase();
    db::list_tasks(conn, Some(project))
        .ok()?
        .into_iter()
        .find(|t| t.description.trim().to_lowercase() == want)
}
