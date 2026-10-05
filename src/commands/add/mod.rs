mod create;
mod duplicate;
mod input;
mod persist;
mod render;
mod similar;
mod types;

use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;

pub use types::AddRequest;

#[cfg(test)]
use {duplicate::find_duplicate_open_task, render::text::render_similar_body};

pub fn run(conn: &Connection, cfg: &Config, req: &AddRequest, yes: bool) -> Result<()> {
    match create::create(conn, cfg, req, yes)? {
        Some(created) => render::text::print_created(&created),
        None => println!("Cancelled."),
    }
    Ok(())
}

pub fn run_value(conn: &Connection, cfg: &Config, req: &AddRequest) -> Result<serde_json::Value> {
    let created = create::create(conn, cfg, req, true)?
        .ok_or_else(|| anyhow::anyhow!("task creation was cancelled"))?;
    Ok(render::value::created_json(created))
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/add/mod.rs"]
mod tests;
