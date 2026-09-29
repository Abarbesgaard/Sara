use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

mod render;

pub fn run(conn: &Connection, as_json: bool) -> Result<()> {
    let tags = db::list_tags_with_counts(conn)?;
    render::print_tags(&tags, as_json)
}
