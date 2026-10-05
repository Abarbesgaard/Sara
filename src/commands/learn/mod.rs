mod create;
mod files;
mod links;
mod overlap;
mod persist;
mod render;
mod types;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;

use crate::infrastructure::config::Config;

pub use types::LearnRequest;

pub fn run(conn: &Connection, cfg: &Config, req: &LearnRequest) -> Result<()> {
    render::print_learned(&learn_value(conn, cfg, req)?);
    Ok(())
}

pub fn learn_value(conn: &Connection, cfg: &Config, req: &LearnRequest) -> Result<Value> {
    create::learn(conn, cfg, req)
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/learn/mod.rs"]
mod tests;
