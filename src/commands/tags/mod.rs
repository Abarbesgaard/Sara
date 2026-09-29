use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

mod render;

/// `sara tags` — list all distinct memory tags with usage counts, most-used
/// first. Intended to be run before `sara learn` to discover the existing
/// vocabulary and avoid near-duplicate tags (service-a vs serviceA).
pub fn run(conn: &Connection, as_json: bool) -> Result<()> {
    let tags = db::list_tags_with_counts(conn)?;
    render::print_tags(&tags, as_json)
}
