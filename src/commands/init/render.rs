use std::io::{self, Write};

use anyhow::Result;

use crate::infrastructure::db::ProjectCommands;
use crate::infrastructure::model::Project;

/// Prompt for a value, showing `default` in brackets and returning it on empty
/// input.
pub(super) fn prompt(msg: &str, default: Option<&str>) -> Result<String> {
    let prompt_str = if let Some(d) = default {
        format!("{msg} [{d}]: ")
    } else {
        format!("{msg}: ")
    };
    print!("{prompt_str}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_string();
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

/// Print the saved-profile summary: goal, resolved stack, and any configured
/// setup/test/lint/run commands.
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
