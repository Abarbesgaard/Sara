use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

pub fn run(conn: &Connection) -> Result<()> {
    match db::undo(conn)? {
        Some(command) => println!("Undid: {command}"),
        None => println!("Nothing to undo."),
    }
    Ok(())
}
