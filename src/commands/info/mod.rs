mod edit;
mod handler;
mod plain;
mod render;
mod types;

use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::tui;

use edit::edit_loop;
use handler::load_detail;
use plain::{RenderOpts, render_markdown, render_plain};

/// Assemble the full guide (from the `task_guide` view) plus freshness +
/// open-feedback into one machine-readable document. Single source of truth for
/// the `--json` CLI path and the MCP `info` tool.
pub fn guide_value(conn: &Connection, id_or_uuid: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    let mut guide = db::guide_json(conn, &task.uuid)?;

    // Freshness: compare the validated commit against the project's current HEAD.
    let head = db::get_project(conn, &task.project)
        .ok()
        .flatten()
        .and_then(|p| p.path)
        .and_then(|path| crate::infrastructure::git::head_commit(std::path::Path::new(&path)));
    let validated = db::get_guide_fields(conn, &task.uuid)?.validated_commit;
    let stale = match (&head, &validated) {
        (Some(h), Some(v)) => h != v,
        _ => false,
    };

    // Open feedback the agent should act on (flagged-for-reconsider first).
    let feedback = db::get_open_feedback(conn, &task.uuid)?;
    let open_feedback: Vec<_> = feedback
        .iter()
        .map(|f| {
            serde_json::json!({
                "id": f.id,
                "text": f.text,
                "target_kind": f.target_kind,
                "target_id": f.target_id,
                "request_revision": f.request_revision,
            })
        })
        .collect();
    let needs_revision = feedback.iter().any(|f| f.request_revision);

    if let Some(obj) = guide.as_object_mut() {
        obj.insert(
            "freshness".to_string(),
            serde_json::json!({ "head": head, "validated_commit": validated, "stale": stale }),
        );
        obj.insert(
            "open_feedback".to_string(),
            serde_json::Value::Array(open_feedback),
        );
        obj.insert(
            "needs_revision".to_string(),
            serde_json::Value::Bool(needs_revision),
        );
    }

    Ok(guide)
}

/// `sara info --json` — emit the full guide (assembled by the `task_guide` view)
/// plus freshness + open-feedback, in one machine-readable document.
pub fn run_json(conn: &Connection, _cfg: &Config, id_or_uuid: &str) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&guide_value(conn, id_or_uuid)?)?
    );
    Ok(())
}

pub fn run(
    conn: &Connection,
    cfg: &Config,
    id_or_uuid: &str,
    plain: bool,
    md: bool,
    history: bool,
) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    let detail = load_detail(conn, cfg, task)?;
    let opts = RenderOpts { history };

    // Markdown digest — agent context / PR bodies. Never opens the TUI.
    if md {
        print!("{}", render_markdown(&detail, opts));
        return Ok(());
    }

    // Readable text digest: forced via --plain, or the automatic fallback when
    // stdout is not a TTY (e.g. piped into an agent).
    use std::io::IsTerminal;
    if plain || !std::io::stdout().is_terminal() {
        print!("{}", render_plain(&detail, opts));
        return Ok(());
    }

    let mut terminal = tui::init_terminal()?;
    let result = edit_loop(&mut terminal, conn, cfg, detail);
    tui::restore_terminal()?;
    result.map(|_| ())
}
