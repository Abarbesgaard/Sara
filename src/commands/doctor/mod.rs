//! `sara doctor` — a read-only health report over the memory store. It composes
//! the existing diagnostics (duplicate detection, the prune evaluation, the
//! embedding index, link integrity, recall activity, anchored-file drift) into one checklist, each
//! finding paired with the command that fixes it, plus the knowledge-reuse KPI.
//! Nothing here writes.

mod checks;
mod render;
mod reuse;
mod types;

pub use reuse::current_project;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::commands::shared::print_json;
use crate::infrastructure::db;
use checks::{anchors, decay, duplicates, embedding, hygiene, orphans};
use types::{Check, Labels, Status};

pub fn doctor_value(conn: &Connection, project: Option<&str>) -> Result<Value> {
    let memories = db::list_memories(conn)?;
    let labels = Labels::new(&memories);

    let checks = vec![
        embedding::embedding_check(conn, &memories, &labels)?,
        orphans::orphan_check(conn, &labels)?,
        duplicates::duplicate_check(conn)?,
        hygiene::superseded_and_backlog_checks(conn)?,
        decay::decay_check(conn, &memories, &labels)?,
        anchors::stale_anchor_check(conn, &labels)?,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<Check>>();

    let tally = |s: Status| checks.iter().filter(|c| c.status == s).count();
    let warn = tally(Status::Warn);
    Ok(json!({
        "healthy": warn == 0,
        "memories": memories.len(),
        "summary": { "ok": tally(Status::Ok), "warn": warn, "info": tally(Status::Info) },
        "checks": checks.iter().map(Check::to_json).collect::<Vec<_>>(),
        "knowledge_reuse": reuse::knowledge_reuse_json(conn, project)?,
    }))
}

pub fn run(conn: &Connection, project: Option<&str>, json: bool, strict: bool) -> Result<()> {
    let v = doctor_value(conn, project)?;
    if json {
        print_json(&v)?;
    } else {
        render::print_report(&v);
    }
    if strict && v["healthy"] == Value::Bool(false) {
        let warn = v["summary"]["warn"].as_u64().unwrap_or(0);
        anyhow::bail!("doctor found {warn} warning(s)");
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/doctor/mod.rs"]
mod tests;
