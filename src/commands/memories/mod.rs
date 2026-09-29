use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

mod render;

pub fn run(conn: &Connection, as_json: bool) -> Result<()> {
    let memories = db::list_memories(conn)?;
    let strengths = db::item_strengths(conn, &memories);
    render::print_memories(conn, &memories, &strengths, as_json)
}
pub(crate) fn canonical_labels(
    conn: &Connection,
    item: &crate::infrastructure::model::Item,
) -> (Vec<String>, Vec<String>) {
    let uuid_str = item.uuid.to_string();
    let derived: Vec<String> = db::get_memory_links_to(conn, &uuid_str)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .filter_map(|l| db::get_item_by_uuid(conn, &l.from_uuid).ok())
        .map(|i| format!("m{}", i.display_id.unwrap_or(0)))
        .collect();
    let derived_from: Vec<String> = db::get_memory_links_from(conn, &uuid_str)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .filter_map(|l| db::get_item_by_uuid(conn, &l.to_uuid).ok())
        .map(|i| format!("m{}", i.display_id.unwrap_or(0)))
        .collect();
    (derived, derived_from)
}
