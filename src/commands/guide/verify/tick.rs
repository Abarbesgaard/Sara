use anyhow::Result;
use rusqlite::Connection;

use super::commands::nth_step;
use crate::commands::guide::support::shell;
use crate::commands::shared::project_head;
use crate::infrastructure::db::{self, ChecklistItem};
use crate::infrastructure::model::Task;

pub(super) fn run(
    conn: &Connection,
    task: &Task,
    steps: &[ChecklistItem],
    acceptance: &[ChecklistItem],
    step: Option<usize>,
    dir: Option<&str>,
) -> Result<()> {
    let commit = project_head(conn, &task.project);
    let targets: Vec<&ChecklistItem> = match step {
        Some(n) => vec![nth_step(steps, n)?],
        None => steps.iter().chain(acceptance).collect(),
    };

    let (mut ran, mut passed) = (0usize, 0usize);
    for s in targets {
        let Some(cmd) = &s.verify_cmd else { continue };
        if ran == 0 && db::ensure_started(conn, &task.uuid)? {
            println!("Task {} is now active.", task.id.unwrap_or(0));
        }
        ran += 1;
        println!("$ {cmd}");
        match shell(cmd, dir).status() {
            Ok(st) if st.success() => {
                let note = format!("verify passed: {cmd}");
                db::set_step_done(conn, s.id, true, Some(&note), commit.as_deref())?;
                passed += 1;
                println!("  ✓ passed — ticked \"{}\"", s.text);
            }
            Ok(st) => {
                let code = st.code().unwrap_or(-1);
                println!("  ✗ exit {code} — left unticked: \"{}\"", s.text);
            }
            Err(e) => {
                println!("  ✗ failed to run ({e}) — left unticked: \"{}\"", s.text);
            }
        }
    }
    if ran == 0 {
        println!("No steps or acceptance criteria have a verify command to run.");
    } else {
        println!("Ticked {passed}/{ran} on pass.");
    }
    Ok(())
}
