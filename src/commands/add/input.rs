use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Priority, Project};
use crate::infrastructure::project::{detect_current_project, parse_add_tokens};
use crate::infrastructure::tui;
use crate::infrastructure::tui::review_form::{FormContext, FormInput, run_form};

/// Parse and resolve all add-command inputs, run the interactive form if needed,
/// and return the completed `FormInput` plus the recur interval (if any).
/// Returns `None` when the user cancels the form.
pub(super) fn resolve(
    conn: &Connection,
    cfg: &Config,
    words: &[String],
    project_override: Option<&str>,
    priority_override: Option<&str>,
    extra_tags: &[String],
    yes: bool,
    recur_override: Option<&str>,
) -> Result<Option<(FormInput, Option<String>)>> {
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
    if let Some(r) = recur_override {
        parsed.recur = Some(r.to_string());
    }

    // Resolve project
    let (project_name, _path) = if let Some(ref p) = parsed.project {
        let path_opt = db::get_project(conn, p)?.and_then(|pr| pr.path);
        db::upsert_project_seen(conn, p, path_opt.as_deref())?;
        (p.clone(), path_opt)
    } else {
        detect_current_project(conn, cfg)?
    };

    let project_profile = db::get_project(conn, &project_name)?.unwrap_or(Project {
        name: project_name.clone(),
        path: None,
        goal: None,
        stack: None,
        conventions: None,
        notes: None,
        initialized_at: None,
        last_seen: None,
        github_repo: None,
        github_login: None,
        github_sync_scope: None,
    });

    let is_tty = atty_check();
    let yes = yes || words.iter().any(|w| w == "--yes" || w == "-y");

    let form_result: Option<FormInput> = if yes || !is_tty {
        let priority = parsed
            .priority
            .as_deref()
            .and_then(|p| p.parse::<Priority>().ok());
        Some(FormInput {
            description: parsed.description.clone(),
            project: project_name.clone(),
            priority,
            due: String::new(),
            tags: parsed.tags.join(","),
            selected_deps: vec![],
            selected_files: vec![],
        })
    } else {
        let pending = db::list_tasks(conn, None)?;
        let available_deps: Vec<(String, String)> = pending
            .iter()
            .map(|t| {
                let short = format!("{}", t.id.unwrap_or(0));
                (short, t.description.clone())
            })
            .collect();

        let project_files: Vec<String> = project_profile
            .path
            .as_deref()
            .map(|p| crate::infrastructure::files::collect_project_entries(std::path::Path::new(p)))
            .unwrap_or_default();

        let priority_init = parsed
            .priority
            .as_deref()
            .and_then(|p| p.parse::<Priority>().ok());

        let ctx = FormContext {
            initial: FormInput {
                description: parsed.description.clone(),
                project: project_name.clone(),
                priority: priority_init,
                due: String::new(),
                tags: parsed.tags.join(","),
                selected_deps: vec![],
                selected_files: vec![],
            },
            available_deps,
            available_files: project_files,
            suggested_dep_indices: vec![],
            suggested_files: vec![],
        };

        let mut terminal = tui::init_terminal()?;
        let result = run_form(&mut terminal, ctx);
        tui::restore_terminal()?;
        result?
    };

    Ok(form_result.map(|f| (f, parsed.recur.clone())))
}

fn atty_check() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}
