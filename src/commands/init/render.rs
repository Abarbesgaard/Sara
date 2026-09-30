use anyhow::Result;

use crate::commands::shared::prompt_line;
use crate::infrastructure::db::ProjectCommands;
use crate::infrastructure::model::Project;

pub(super) fn prompt(msg: &str, default: Option<&str>) -> Result<String> {
    let prompt_str = if let Some(d) = default {
        format!("{msg} [{d}]: ")
    } else {
        format!("{msg}: ")
    };
    let trimmed = prompt_line(&prompt_str)?;
    if trimmed.is_empty() {
        Ok(default.unwrap_or("").to_string())
    } else {
        Ok(trimmed)
    }
}

pub(super) fn note_not_in_git(name: &str) {
    println!("Note: not inside a git repo — initializing the current folder as project '{name}'.");
}

pub(super) fn print_intro(name: &str, stack: &str) {
    println!("Initializing project: {name}");
    println!("Detected stack: {stack}");
}

pub(super) fn print_saved(project: &Project, resolved_stack: &str, commands: &ProjectCommands) {
    println!("✔ Project '{}' profile saved.", project.name);
    if let Some(g) = &project.goal {
        println!("  Goal:  {g}");
    }
    println!("  Stack: {resolved_stack}");
    for (label, cmd) in [
        ("Setup", &commands.setup_cmd),
        ("Test", &commands.test_cmd),
        ("Lint", &commands.lint_cmd),
        ("Run", &commands.run_cmd),
    ] {
        if let Some(c) = cmd {
            println!("  {label}:  {c}");
        }
    }
}
