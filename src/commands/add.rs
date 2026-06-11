use anyhow::Result;
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::enrich;
use crate::model::{Priority, Task};
use crate::project::{detect_current_project, parse_add_tokens};
use crate::tui;
use crate::tui::review_form::{FormContext, FormInput, run_form};

pub fn run(
    conn: &Connection,
    cfg: &Config,
    words: &[String],
    project_override: Option<&str>,
    priority_override: Option<&str>,
    extra_tags: &[String],
    yes: bool,
    llm: bool,
) -> Result<()> {
    // Parse inline tokens
    let mut parsed = parse_add_tokens(words);

    if parsed.description.trim().is_empty() {
        anyhow::bail!("Task description cannot be empty");
    }

    // Flags override inline tokens
    if let Some(p) = project_override {
        parsed.project = Some(p.to_string());
    }
    if let Some(p) = priority_override {
        parsed.priority = Some(p.to_uppercase());
    }
    parsed.tags.extend_from_slice(extra_tags);

    // Resolve project
    let (project_name, _path) = if let Some(ref p) = parsed.project {
        let path_opt = db::get_project(conn, p)?.and_then(|pr| pr.path);
        db::upsert_project_seen(conn, p, path_opt.as_deref())?;
        (p.clone(), path_opt)
    } else {
        detect_current_project(conn, cfg)?
    };

    let project_profile = db::get_project(conn, &project_name)?.unwrap_or(crate::model::Project {
        name: project_name.clone(),
        path: None,
        goal: None,
        stack: None,
        conventions: None,
        notes: None,
        initialized_at: None,
        last_seen: None,
    });

    // LLM enrichment is opt-in (it adds latency); enable with --llm/--ai.
    let enrichment = if llm {
        enrich::enrich_task(conn, cfg, &parsed.description, &project_profile)
    } else {
        None
    };

    // Check if we're in a TTY
    let is_tty = atty_check();

    let form_result: Option<FormInput> = if yes || !is_tty {
        // Non-interactive: accept LLM proposals directly
        let priority = enrichment
            .as_ref()
            .and_then(|e| e.priority.as_deref())
            .and_then(|p| p.parse::<Priority>().ok())
            .or_else(|| parsed.priority.as_deref().and_then(|p| p.parse().ok()));
        let due = enrichment
            .as_ref()
            .and_then(|e| e.due.clone())
            .unwrap_or_default();
        let mut tags = parsed.tags.clone();
        if let Some(ref e) = enrichment {
            for t in &e.tags {
                if !tags.contains(t) {
                    tags.push(t.clone());
                }
            }
        }
        Some(FormInput {
            description: parsed.description.clone(),
            project: project_name.clone(),
            priority,
            due,
            tags: tags.join(","),
            selected_deps: vec![],
            selected_files: vec![],
        })
    } else {
        // Build form context
        let pending = db::list_tasks(conn, None)?;
        let available_deps: Vec<(String, String)> = pending
            .iter()
            .map(|t| {
                let short = format!("{}", t.id.unwrap_or(0));
                (short, t.description.clone())
            })
            .collect();

        // Project files *and folders* for the manual picker.
        let project_files: Vec<String> = project_profile
            .path
            .as_deref()
            .map(|p| crate::files::collect_project_entries(std::path::Path::new(p)))
            .unwrap_or_default();

        let priority_init = enrichment
            .as_ref()
            .and_then(|e| e.priority.as_deref())
            .and_then(|p| p.parse::<Priority>().ok())
            .or_else(|| parsed.priority.as_deref().and_then(|p| p.parse().ok()));

        let mut init_tags = parsed.tags.clone();
        if let Some(ref e) = enrichment {
            for t in &e.tags {
                if !init_tags.contains(t) {
                    init_tags.push(t.clone());
                }
            }
        }

        let ctx = FormContext {
            initial: FormInput {
                description: parsed.description.clone(),
                project: project_name.clone(),
                priority: priority_init,
                due: enrichment
                    .as_ref()
                    .and_then(|e| e.due.clone())
                    .unwrap_or_default(),
                tags: init_tags.join(","),
                selected_deps: vec![],
                selected_files: vec![],
            },
            available_deps,
            available_files: project_files,
            suggested_dep_indices: vec![], // could map LLM dep suggestions here
            suggested_files: vec![],
        };

        let mut terminal = tui::init_terminal()?;
        let result = run_form(&mut terminal, ctx);
        tui::restore_terminal()?;
        result?
    };

    let Some(form) = form_result else {
        println!("Cancelled.");
        return Ok(());
    };

    // Build and insert the task
    let mut task = Task::new(form.description, form.project.clone());
    task.priority = form.priority;
    task.tags = form
        .tags
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Parse due date
    if !form.due.is_empty() {
        task.due = parse_due(&form.due, cfg);
    }

    // Compute initial urgency
    task.urgency = db::compute_urgency(&task, &cfg.urgency, false, 0);

    db::insert_task(conn, &mut task)?;

    // Attach selected files/folders. All are user-picked (manual). Paths come
    // straight from the form and may include arbitrary paths the user typed.
    if !form.selected_files.is_empty() {
        db::set_task_files(conn, &task.uuid, &form.selected_files)?;
    }

    // Add selected deps
    let pending = db::list_tasks(conn, None)?;
    for &dep_idx in &form.selected_deps {
        if let Some(dep_task) = pending.get(dep_idx) {
            if let Err(e) = db::add_dependency(conn, &task.uuid, &dep_task.uuid) {
                eprintln!("Warning: could not add dependency: {e}");
            }
        }
    }

    // Refresh urgency with blocking info
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    println!(
        "Created task {} [{}]: {}",
        task.id.unwrap_or(0),
        task.project,
        task.description
    );
    Ok(())
}

fn atty_check() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

pub fn parse_due(s: &str, cfg: &Config) -> Option<chrono::DateTime<chrono::Utc>> {
    crate::dates::parse_due(s, &cfg.date_dialect)
}
