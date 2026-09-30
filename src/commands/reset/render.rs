use crate::commands::shared::prompt_line;
use anyhow::Result;

pub(super) fn print_nothing_to_reset(name: &str) {
    println!("Nothing to reset: project '{name}' has no tasks or profile.");
}

pub(super) fn confirm(name: &str, task_count: usize) -> Result<bool> {
    println!(
        "This will permanently delete project '{name}':\n  \
         • {task_count} task(s) and all their files, links, comments and history\n  \
         • the project profile (you'll need to run `sara init` again)"
    );
    let input = prompt_line("Type the project name to confirm: ")?;
    Ok(input == name)
}

pub(super) fn print_aborted() {
    println!("Aborted — name did not match.");
}

pub(super) fn print_reset(name: &str, deleted: usize) {
    println!("✔ Reset project '{name}': removed {deleted} task(s) and its profile.");
    println!("Run `sara init` to set it up again.");
}
