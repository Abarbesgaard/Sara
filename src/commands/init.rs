use anyhow::Result;
use rusqlite::Connection;
use std::io::{self, Write};

use crate::config::Config;
use crate::model::Project;
use crate::project::find_git_root;

/// Detect which tech stacks are present in the project root.
pub fn detect_stack(path: &str) -> String {
    let root = std::path::Path::new(path);
    let mut stacks = vec![];
    let markers = [
        ("Cargo.toml", "Rust"),
        ("package.json", "Node.js/JS"),
        ("pyproject.toml", "Python"),
        ("requirements.txt", "Python"),
        ("go.mod", "Go"),
        ("pom.xml", "Java/Maven"),
        ("build.gradle", "Java/Gradle"),
        ("Gemfile", "Ruby"),
        ("composer.json", "PHP"),
        ("pubspec.yaml", "Dart/Flutter"),
        ("*.swift", "Swift"),
        ("CMakeLists.txt", "C/C++"),
        ("mix.exs", "Elixir"),
    ];
    for (file, label) in &markers {
        if file.contains('*') {
            // glob-ish: skip for simplicity, just check extension presence
        } else if root.join(file).exists() {
            stacks.push(*label);
        }
    }
    // Check for Swift files manually
    if let Ok(rd) = std::fs::read_dir(root) {
        for entry in rd.flatten() {
            if entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e == "swift")
                .unwrap_or(false)
                && !stacks.contains(&"Swift")
            {
                stacks.push("Swift");
            }
        }
    }
    if stacks.is_empty() {
        "unknown".to_string()
    } else {
        stacks.join(", ")
    }
}

fn prompt(msg: &str, default: Option<&str>) -> Result<String> {
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

pub fn run(
    conn: &Connection,
    cfg: &Config,
    name_override: Option<&str>,
    goal_override: Option<&str>,
    yes: bool,
    no_llm: bool,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let git_root = find_git_root(&cwd);

    // Resolve the project from the current folder: a git repo is its own
    // project; otherwise the folder itself is initialized as the project.
    let (resolved_name, resolved_path) = crate::project::project_identity_for_dir(&cwd, cfg);
    let project_name = name_override.map(str::to_string).unwrap_or(resolved_name);
    let project_path = Some(resolved_path);

    if git_root.is_none() {
        println!(
            "Note: not inside a git repo — initializing the current folder as project '{}'.",
            project_name
        );
    }

    // Detect stack
    let detected_stack = project_path
        .as_deref()
        .map(detect_stack)
        .unwrap_or_else(|| "unknown".to_string());

    println!("Initializing project: {}", project_name);
    println!("Detected stack: {}", detected_stack);

    // Load existing profile if any
    let existing = crate::db::get_project(conn, &project_name)?;

    let goal = if let Some(g) = goal_override {
        g.to_string()
    } else if yes {
        existing
            .as_ref()
            .and_then(|p| p.goal.clone())
            .unwrap_or_default()
    } else {
        let current_goal = existing.as_ref().and_then(|p| p.goal.as_deref());
        prompt("What is this project? (one-line goal)", current_goal)?
    };

    let notes = if yes {
        existing
            .as_ref()
            .and_then(|p| p.notes.clone())
            .unwrap_or_default()
    } else {
        let current_notes = existing.as_ref().and_then(|p| p.notes.as_deref());
        prompt("Any conventions or notes? (optional)", current_notes)?
    };

    let project = Project {
        name: project_name.clone(),
        path: project_path,
        goal: if goal.is_empty() {
            None
        } else {
            Some(goal.clone())
        },
        stack: Some(detected_stack.clone()),
        conventions: None,
        notes: if notes.is_empty() { None } else { Some(notes) },
        initialized_at: Some(chrono::Utc::now()),
        last_seen: Some(chrono::Utc::now()),
    };

    crate::db::save_project_profile(conn, &project)?;
    println!("✔ Project '{}' profile saved.", project_name);

    if let Some(g) = &project.goal {
        println!("  Goal:  {g}");
    }
    println!("  Stack: {detected_stack}");

    // Optional LLM task seeding
    if !no_llm && !yes {
        print!("Seed initial tasks from goal via LLM? [y/N]: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("y") {
            seed_tasks(conn, cfg, &project)?;
        }
    }

    Ok(())
}

fn seed_tasks(conn: &Connection, cfg: &Config, project: &Project) -> Result<()> {
    use indicatif::{ProgressBar, ProgressStyle};
    use std::time::Duration;

    let Some(goal) = &project.goal else {
        println!("No goal set — skipping task seeding.");
        return Ok(());
    };

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message("Generating initial tasks…");
    spinner.enable_steady_tick(Duration::from_millis(80));

    let req = crate::llm::EnrichmentRequest {
        description: format!("Break this project goal into 3-5 initial tasks: {goal}"),
        project_name: project.name.clone(),
        project_goal: project.goal.clone(),
        project_stack: project.stack.clone(),
        project_notes: project.notes.clone(),
        existing_tasks: vec![],
        repo_tree: None,
        project_commands: None,
    };
    let provider = crate::llm::build_provider(cfg);
    let result = provider.enrich(&req);
    spinner.finish_and_clear();

    match result {
        Ok(resp) => {
            if let Some(cleaned) = &resp.description_suggestion {
                // Parse newline-separated tasks from the suggestion
                for (i, line) in cleaned.lines().enumerate() {
                    let line = line.trim().trim_start_matches(|c: char| !c.is_alphabetic());
                    if line.is_empty() {
                        continue;
                    }
                    println!("  {}. {}", i + 1, line);
                }
                print!("Accept and create these tasks? [y/N]: ");
                std::io::Write::flush(&mut std::io::stdout())?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().eq_ignore_ascii_case("y") {
                    for line in cleaned.lines() {
                        let desc = line.trim().trim_start_matches(|c: char| !c.is_alphabetic());
                        if desc.is_empty() {
                            continue;
                        }
                        let mut task =
                            crate::model::Task::new(desc.to_string(), project.name.clone());
                        crate::db::insert_task(conn, &mut task)?;
                    }
                    println!("✔ Tasks created.");
                }
            }
        }
        Err(e) => {
            eprintln!("LLM seeding failed: {e:#}");
        }
    }
    Ok(())
}
