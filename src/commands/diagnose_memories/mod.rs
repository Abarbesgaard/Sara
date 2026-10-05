mod corpus;
mod detect;
mod render;
mod types;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;

use crate::commands::shared::print_json;

pub const DEFAULT_CONFLICT_THRESHOLD: f32 = 0.75;

pub fn diagnose_value(
    conn: &Connection,
    threshold: f32,
    project: Option<&str>,
    limit: Option<usize>,
) -> Result<Value> {
    let corpus = corpus::load(conn, project)?;
    let candidates = detect::find_conflicts(&corpus, threshold);
    Ok(render::value::conflicts_json(
        candidates, threshold, project, limit,
    ))
}

pub fn run(
    conn: &Connection,
    json_output: bool,
    threshold: f32,
    project: Option<&str>,
    limit: Option<usize>,
) -> Result<()> {
    let v = diagnose_value(conn, threshold, project, limit)?;
    if json_output {
        return print_json(&v);
    }
    render::text::print_report(&v, threshold, project);
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/diagnose_memories/mod.rs"]
mod tests;
