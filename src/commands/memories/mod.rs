use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

mod render;

pub fn run(conn: &Connection, as_json: bool) -> Result<()> {
    let memories = db::list_memories(conn)?;
    let strengths = db::item_strengths(conn, &memories);
    let provenance = db::memory_provenance_all(conn);
    render::print_memories(conn, &memories, &strengths, &provenance, as_json)
}
