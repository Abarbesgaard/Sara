use std::process::Command;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::next_actions::{next_actions, pr_link};
use crate::commands::guide::{GateOutput, validate_value};
use crate::commands::shared::with_citation;
use crate::commands::{annotate, done};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

#[derive(Debug, Default)]
pub struct FinishRequest<'a> {
    pub id: &'a str,
    pub pr_url: Option<&'a str>,
    /// Whether the PR has merged; `None` asks `gh`.
    pub merged: Option<bool>,
    pub used: &'a [String],
}

/// A PR's merge state as `gh` reports it: `Some(true)` merged, `Some(false)`
/// open or closed unmerged, `None` when `gh` is unavailable or fails.
fn probe_merged(url: &str) -> Option<bool> {
    let out = Command::new("gh")
        .args(["pr", "view", url, "--json", "state", "-q", ".state"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim() == "MERGED")
}

/// Close out a task from its pull request in one call: link the PR (once),
/// and when it has merged, prove the acceptance criteria green at the current
/// HEAD and mark the task done. Before the merge it only links and reports,
/// so it is safe to call as soon as the PR is opened.
pub fn finish_value(conn: &Connection, cfg: &Config, req: &FinishRequest) -> Result<Value> {
    let task = db::resolve_task(conn, req.id)?;
    let existing = pr_link(conn, &task)?;
    let mut linked = false;
    let url = match (req.pr_url, existing) {
        (Some(url), _) => {
            let known = db::get_links(conn, &task.uuid)?
                .iter()
                .any(|l| l.url == url);
            if !known {
                annotate::link_value(conn, req.id, url, None)?;
                linked = true;
            }
            url.to_string()
        }
        (None, Some(url)) => url,
        (None, None) => anyhow::bail!(
            "task has no linked pull request — pass `pr_url` (the PR you opened for this task)"
        ),
    };

    let merged = req.merged.or_else(|| probe_merged(&url));
    if merged != Some(true) {
        let state = match merged {
            Some(false) => "not merged yet",
            _ => "unknown (gh unavailable — pass merged=true once it has merged)",
        };
        return Ok(json!({
            "status": "waiting_for_merge",
            "uuid": task.uuid.to_string(),
            "pr_url": url,
            "linked": linked,
            "merge_state": state,
            "next_actions": [{
                "tool": "finish",
                "args": { "id": &task.uuid.to_string()[..8], "pr_url": url },
                "why": "call again once the PR has merged and the base branch is checked out and pulled; it then validates and closes the task",
            }],
        }));
    }

    let validated =
        validate_value(conn, req.id, false, GateOutput::Capture, false).map_err(|e| {
            anyhow::anyhow!(
                "finish: the PR has merged but the task stays open — {e:#} \
                 (check out and pull the base branch first if you are still on the feature branch)"
            )
        })?;
    let closed = with_citation(conn, req.id, req.used, || {
        done::done_value(conn, cfg, req.id, false)
    })?;
    let remaining = db::resolve_task(conn, req.id)
        .ok()
        .map(|t| next_actions(conn, &t).unwrap_or_default())
        .unwrap_or_default();
    Ok(json!({
        "status": "done",
        "uuid": task.uuid.to_string(),
        "pr_url": url,
        "linked": linked,
        "validated_commit": validated["validated_commit"],
        "done": closed,
        "next_actions": remaining,
    }))
}
