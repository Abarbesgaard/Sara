use crate::commands::shared::prompt_line;
use anyhow::Result;

pub(super) fn confirm_delete(task_id: i64, description: &str) -> Result<bool> {
    let input = prompt_line(&format!("Delete task {task_id} \"{description}\"? [y/N]: "))?;
    Ok(input.eq_ignore_ascii_case("y"))
}

pub(super) fn warn_unblocked(count: usize) {
    eprintln!("Warning: {count} task(s) depended on this task and will be unblocked.");
}

pub(super) fn print_deleted(description: &str) {
    println!("Deleted: {description}");
}
