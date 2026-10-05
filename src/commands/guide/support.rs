use std::process::Command;

use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db::{self, ChecklistItem};

pub(super) fn kind_arg(kind: Option<&str>) -> &str {
    match kind {
        Some("acceptance") => db::STEP_KIND_ACCEPTANCE,
        _ => db::STEP_KIND_STEP,
    }
}

pub(super) fn one_based(n: usize, kind: &str) -> Result<usize> {
    n.checked_sub(1)
        .ok_or_else(|| anyhow::anyhow!("{kind} index is 1-based; got 0"))
}

pub(super) fn working_dir(conn: &Connection, project: &str) -> Option<String> {
    db::get_project(conn, project)
        .ok()
        .flatten()
        .and_then(|p| p.path)
}

pub(super) fn shell(cmd: &str, dir: Option<&str>) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command
}

pub(super) fn print_checklist(header: &str, items: &[ChecklistItem]) {
    if items.is_empty() {
        return;
    }
    println!("{header}");
    for (i, s) in items.iter().enumerate() {
        let mark = if s.done { "[x]" } else { "[ ]" };
        let badge = if s.source == "ai" { " (ai)" } else { "" };
        println!("  {} {}. {}{}", mark, i + 1, s.text, badge);
        if let Some(intent) = &s.intent {
            println!("        {intent}");
        }
    }
}
