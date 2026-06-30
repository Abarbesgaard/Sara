//! Dynamic shell-completion candidates (clap_complete `unstable-dynamic`).
//!
//! These feed `#[arg(add = ArgValueCandidates::new(...))]` in `cli.rs` so that,
//! once completions are registered (`source <(COMPLETE=zsh sara)`), `sara done
//! <TAB>` offers the real pending task ids — each annotated with its
//! description — and `--project <TAB>` offers the known project names.
//!
//! Each helper opens its own DB connection (completion runs as a fresh
//! subprocess invocation) and degrades to an empty list on any error, so a
//! broken/locked DB never breaks the user's shell.

use clap_complete::engine::CompletionCandidate;
use rusqlite::Connection;

/// Candidates for a task id/uuid argument: every pending task's display id,
/// helped by its description.
pub fn task_ids() -> Vec<CompletionCandidate> {
    crate::infrastructure::db::open()
        .ok()
        .map(|conn| task_ids_from(&conn))
        .unwrap_or_default()
}

fn task_ids_from(conn: &Connection) -> Vec<CompletionCandidate> {
    crate::infrastructure::db::list_tasks(conn, None)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| {
            t.id.map(|id| CompletionCandidate::new(id.to_string()).help(Some(t.description.into())))
        })
        .collect()
}

/// Candidates for a project argument: every known project name.
pub fn projects() -> Vec<CompletionCandidate> {
    crate::infrastructure::db::open()
        .ok()
        .map(|conn| projects_from(&conn))
        .unwrap_or_default()
}

fn projects_from(conn: &Connection) -> Vec<CompletionCandidate> {
    crate::infrastructure::db::project_names(conn)
        .unwrap_or_default()
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}
