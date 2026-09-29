use std::io::{self, Write};

use anyhow::Result;

/// Ask the user to confirm deleting a task. Returns true only on an explicit `y`.
pub(super) fn confirm_delete(task_id: i64, description: &str) -> Result<bool> {
    print!("Delete task {task_id} \"{description}\"? [y/N]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

pub(super) fn print_cancelled() {
    println!("Cancelled.");
}

/// Warn that tasks which depended on the deleted one will be unblocked.
pub(super) fn warn_unblocked(count: usize) {
    eprintln!("Warning: {count} task(s) depended on this task and will be unblocked.");
}

pub(super) fn print_deleted(description: &str) {
    println!("Deleted: {description}");
}
