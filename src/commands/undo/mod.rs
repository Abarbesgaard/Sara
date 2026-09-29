use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

mod render;

pub fn run(conn: &Connection) -> Result<()> {
    render::print_undo(db::undo(conn)?.as_deref());
    Ok(())
}
