use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::model::Task;

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
