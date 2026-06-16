use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;
use std::time::Duration;

use crate::config::Config;
use crate::llm::{self, EnrichmentRequest, EnrichmentResponse};
use crate::model::Project;

/// Run LLM enrichment for a task description.
/// Returns `(suggestions, error_message)`.  `suggestions` is None when the
/// LLM call fails; `error_message` is Some in that case so the caller can
/// surface it to the user.
pub fn enrich_task(
    conn: &Connection,
    cfg: &Config,
    description: &str,
    project: &Project,
) -> (Option<EnrichmentResponse>, Option<String>) {
    // Gather existing tasks for dep suggestions
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

    let req = EnrichmentRequest {
        description: description.to_string(),
        project_name: project.name.clone(),
        project_goal: project.goal.clone(),
        project_stack: project.stack.clone(),
        project_notes: project.notes.clone(),
        existing_tasks,
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

/// Enrich a captured note or link with summary, tags, and PARA folder suggestion.
pub fn enrich_item(
    cfg: &Config,
    kind: &str,
    title: &str,
    body: &str,
    url: Option<&str>,
) -> (llm::ItemEnrichmentResponse, Option<String>) {
    let profile = crate::learn::read_profile_context(cfg);
    let req = llm::ItemEnrichmentRequest {
        kind: kind.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        url: url.map(|s| s.to_string()),
        profile_context: profile,
    };

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message("Sara is analyzing…");
    spinner.enable_steady_tick(Duration::from_millis(80));

    let provider = llm::build_provider(cfg);
    let result = provider.enrich_item(&req);
    spinner.finish_and_clear();

    match result {
        Ok(resp) if resp.summary.is_some() || !resp.tags.is_empty() => (resp, None),
        Ok(resp) => (resp, None),
        Err(e) => (llm::ItemEnrichmentResponse::default(), Some(format!("{e:#}"))),
    }
}
