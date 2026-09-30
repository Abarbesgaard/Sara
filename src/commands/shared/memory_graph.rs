use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::model::Item;

const DERIVED_FROM: &str = "derived_from";

pub fn item_label(item: &Item) -> String {
    format!(
        "{}{}",
        item.kind.chars().next().unwrap_or('m'),
        item.display_id.unwrap_or(0)
    )
}

pub fn memory_handle(item: &Item) -> String {
    format!("m{}", item.display_id.unwrap_or(0))
}

pub fn short_handle(item: &Item) -> String {
    item.display_id
        .map(|id| format!("m{id}"))
        .unwrap_or_else(|| item.uuid.to_string()[..8].to_string())
}

pub fn item_snippet(item: &Item, max: usize) -> String {
    item.summary
        .as_deref()
        .unwrap_or(&item.body)
        .chars()
        .take(max)
        .collect()
}

pub fn derived_from_suffix(parents: &[String]) -> String {
    if parents.is_empty() {
        String::new()
    } else {
        format!(" [derived from: {}]", parents.join(", "))
    }
}

pub fn derived_count(conn: &Connection, uuid: &str) -> usize {
    db::get_memory_links_to(conn, uuid)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == DERIVED_FROM)
        .count()
}

pub fn derived_children(conn: &Connection, uuid: &str) -> Vec<Item> {
    db::get_memory_links_to(conn, uuid)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == DERIVED_FROM)
        .filter_map(|l| db::get_item_by_uuid(conn, &l.from_uuid).ok())
        .collect()
}

pub fn derived_parents(conn: &Connection, uuid: &str) -> Vec<Item> {
    db::get_memory_links_from(conn, uuid)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == DERIVED_FROM)
        .filter_map(|l| db::get_item_by_uuid(conn, &l.to_uuid).ok())
        .collect()
}

pub fn canonical_labels(conn: &Connection, item: &Item) -> (Vec<String>, Vec<String>) {
    let uuid = item.uuid.to_string();
    let handles = |items: Vec<Item>| items.iter().map(memory_handle).collect();
    (
        handles(derived_children(conn, &uuid)),
        handles(derived_parents(conn, &uuid)),
    )
}
