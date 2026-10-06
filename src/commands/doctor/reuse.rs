//! The vision's KPI: the share of verified tasks that drew on prior knowledge.
//! It measures outcomes, not store health, so it never affects `healthy`.

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db::{self, ReuseStats};
use crate::infrastructure::project::project_identity_for_dir;

pub(super) const WINDOW_DAYS: i64 = 30;
pub(super) const MIN_TASKS: u64 = 5;

/// The project the current directory belongs to, resolved without writing
/// (unlike `detect_current_project`, which records the project as seen).
pub fn current_project(conn: &Connection, cfg: &Config) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let (name, path) = project_identity_for_dir(&cwd, cfg);
    match db::get_project_by_path(conn, &path) {
        Ok(Some(p)) => Some(p.name),
        _ => Some(name),
    }
}

/// Shares are fractions rounded to 3 decimals, null until `MIN_TASKS`.
pub(super) fn scope_json(s: ReuseStats) -> Value {
    let sufficient = s.verified >= MIN_TASKS;
    let share =
        |n: u64| sufficient.then(|| (n as f64 / s.verified as f64 * 1000.0).round() / 1000.0);
    json!({
        "verified": s.verified,
        "used_prior": s.used_prior,
        "cited_prior": s.cited_prior,
        "share": share(s.used_prior),
        "cited_share": share(s.cited_prior),
        "sufficient": sufficient,
    })
}

pub(super) fn knowledge_reuse_json(conn: &Connection, project: Option<&str>) -> Result<Value> {
    let since = Utc::now() - chrono::Duration::days(WINDOW_DAYS);
    let project = match project {
        Some(name) => {
            let mut v = scope_json(db::knowledge_reuse(conn, Some(name), &since)?);
            v["name"] = json!(name);
            v
        }
        None => Value::Null,
    };
    Ok(json!({
        "window_days": WINDOW_DAYS,
        "min_tasks": MIN_TASKS,
        "global": scope_json(db::knowledge_reuse(conn, None, &since)?),
        "project": project,
    }))
}

pub(super) fn scope_line(v: &Value) -> String {
    let n = |k: &str| v[k].as_u64().unwrap_or(0);
    if v["sufficient"] != Value::Bool(true) {
        return format!(
            "insufficient data ({} of {MIN_TASKS} verified tasks)",
            n("verified")
        );
    }
    let pct = |k: &str| (v[k].as_f64().unwrap_or(0.0) * 100.0).round();
    format!(
        "{}% drew on prior memory · {}% cited it ({} verified tasks)",
        pct("share"),
        pct("cited_share"),
        n("verified")
    )
}
