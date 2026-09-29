use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

mod render;

/// `sara memories` — browse all saved memories, newest first, with derived
/// strength shown so a human can audit which memories the recall system trusts.
pub fn run(conn: &Connection, as_json: bool) -> Result<()> {
    let memories = db::list_memories(conn)?;
    // One grouped query per strength component instead of one per memory:
    // `item_strength` scans the `memory_recalled` event log every call, so the
    // per-item form made this command O(memories x recall events).
    let strengths = db::item_strengths(conn, &memories);
    render::print_memories(conn, &memories, &strengths, as_json)
}
/// via incoming derived_from edges, labels of canonicals via outgoing
/// derived_from edges)`. Shared by the plain and JSON output paths so
/// `sara memories` matches the vocabulary `recall` already established
/// (PR #79) and `sara dream` mirrors below.
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
