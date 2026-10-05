use anyhow::Result;
use rusqlite::Connection;

use crate::commands::guide::support::one_based;
use crate::infrastructure::db::{self, ChecklistItem};
use crate::infrastructure::model::Task;

const META_COMMAND_KEYS: [&str; 2] = ["test_cmd", "lint_cmd"];

pub(super) fn nth_step(steps: &[ChecklistItem], n: usize) -> Result<&ChecklistItem> {
    let idx = one_based(n, db::STEP_KIND_STEP)?;
    steps
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("No step #{n}"))
}

pub(super) fn all_commands(
    conn: &Connection,
    task: &Task,
    steps: &[ChecklistItem],
    acceptance: &[ChecklistItem],
) -> Result<Vec<String>> {
    let pc = db::get_project_commands(conn, &task.project)?;
    let project = [pc.setup_cmd, pc.test_cmd, pc.lint_cmd]
        .into_iter()
        .flatten();
    let checklist = steps
        .iter()
        .chain(acceptance)
        .filter_map(|s| s.verify_cmd.clone());
    let meta = db::get_guide_fields(conn, &task.uuid)?
        .meta_json
        .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok());
    let from_meta = META_COMMAND_KEYS.into_iter().filter_map(|key| {
        meta.as_ref()?
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });
    Ok(project.chain(checklist).chain(from_meta).collect())
}
