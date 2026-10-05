mod interactive;
mod layout;
mod load;
mod plain;
mod render;
mod types;

use std::io::IsTerminal;

use anyhow::Result;
use rusqlite::Connection;

use interactive::{MemoryScreen, WebScreen, drive};

pub fn run(conn: &Connection, handle: &str) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        return plain::memory(conn, handle);
    }
    let data = load::open(conn, handle)?;
    drive(conn, &mut MemoryScreen::new(data))
}

pub fn run_web(conn: &Connection) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        return plain::web(conn);
    }
    let web = load::load_web(conn)?;
    if web.stars.is_empty() {
        println!("No memories yet — nothing to dream about. Use `sara learn` first.");
        return Ok(());
    }
    drive(conn, &mut WebScreen::new(web))
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/dream/mod.rs"]
mod tests;
