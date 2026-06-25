use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;
use std::time::Duration;

use crate::config::Config;
use crate::llm::{self, EnrichmentRequest, EnrichmentResponse};
use crate::model::Project;

/// Run LLM enrichment for a task description.
pub fn enrich_task(
    conn: &Connection,
    cfg: &Config,
    description: &str,
    project: &Project,
) -> (Option<EnrichmentResponse>, Option<String>) {
    let existing_tasks: Vec<(String, String)> = crate::db::list_tasks(conn, None)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t.description != description)
        .map(|t| {
            let short = t.uuid.to_string()[..8].to_string();
            (short, t.description.clone())
        })
        .take(20)
        .collect();

    // Ground the LLM in the actual repo so it suggests real paths/anchors.
    let repo_tree = project.path.as_deref().map(|p| {
        let root = std::path::Path::new(p);
        let files = crate::files::collect_project_files(root);
        crate::files::build_tree_summary(root, &files)
    });

    let project_commands = {
        let c = crate::db::get_project_commands(conn, &project.name).unwrap_or_default();
        let mut lines = vec![];
        if let Some(s) = c.setup_cmd {
            lines.push(format!("setup: {s}"));
        }
        if let Some(s) = c.test_cmd {
            lines.push(format!("test: {s}"));
        }
        if let Some(s) = c.lint_cmd {
            lines.push(format!("lint: {s}"));
        }
        if let Some(s) = c.run_cmd {
            lines.push(format!("run: {s}"));
        }
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    };

    let req = EnrichmentRequest {
        description: description.to_string(),
        project_name: project.name.clone(),
        project_goal: project.goal.clone(),
        project_stack: project.stack.clone(),
        project_notes: project.notes.clone(),
        existing_tasks,
        repo_tree,
        project_commands,
    };

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message("Asking LLM for suggestions…");
    spinner.enable_steady_tick(Duration::from_millis(80));

    let provider = llm::build_provider(cfg);
    let result = provider.enrich(&req);
    spinner.finish_and_clear();

    match result {
        Ok(resp) => (Some(resp), None),
        Err(e) => {
            let msg = format!("{e:#}");
            (None, Some(msg))
        }
    }
}
