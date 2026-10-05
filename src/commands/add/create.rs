use anyhow::Result;
use rusqlite::Connection;

use super::types::{AddRequest, Created};
use super::{duplicate, input, persist, similar};
use crate::commands::shared::split_csv;
use crate::infrastructure::config::Config;

const SIMILAR_LIMIT: i64 = 5;

pub(super) fn create(
    conn: &Connection,
    cfg: &Config,
    req: &AddRequest,
    yes: bool,
) -> Result<Option<Created>> {
    let Some((form, recur)) = input::resolve(conn, cfg, req, yes)? else {
        return Ok(None);
    };

    let similar = similar::find_similar(
        conn,
        cfg,
        &form.description,
        &split_csv(&form.tags),
        &form.project,
        SIMILAR_LIMIT,
    );
    let duplicate = duplicate::find_duplicate_open_task(conn, &form.project, &form.description);

    let task = persist::save(conn, cfg, form, recur, req)?;
    let branch = persist::tie_branch(conn, &task);

    Ok(Some(Created {
        task,
        branch,
        similar,
        duplicate,
    }))
}
