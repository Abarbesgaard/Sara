use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

/// Print-free core shared by the CLI `forget` command and the MCP `forget` tool.
/// If the memory being forgotten is canonical (has incoming `derived_from`
/// links), its derived children are listed in the result so the caller can
/// review/archive them — never auto-archived unless `cascade` is set, in
/// which case they're archived too (one level: direct derived children only).
pub fn forget_value(conn: &Connection, handle: &str, cascade: bool) -> Result<Value> {
    let item = db::get_item_by_handle(conn, handle)?;
    let derived: Vec<(String, uuid::Uuid)> = db::get_memory_links_to(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .filter_map(|l| {
            db::get_item_by_uuid(conn, &l.from_uuid)
                .ok()
                .map(|i| (format!("m{}", i.display_id.unwrap_or(0)), i.uuid))
        })
        .collect();

    db::archive_item(conn, &item.uuid)?;
    // Drop any semantic-index entry too, so a forgotten memory can never
    // resurface via `recall --semantic`.
    let _ = db::delete_embedding(conn, &item.uuid.to_string());

    let mut cascaded: Vec<String> = Vec::new();
    if cascade {
        for (label, uuid) in &derived {
            if db::archive_item(conn, uuid).is_ok() {
                let _ = db::delete_embedding(conn, &uuid.to_string());
                cascaded.push(label.clone());
            }
        }
    }

    Ok(json!({
        "label": handle,
        "uuid": item.uuid.to_string(),
        "archived": true,
        "derived": derived.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(),
        "cascaded": cascaded,
    }))
}

/// `sara forget <label>` — archive a memory by its label (e.g. m3).
/// `--cascade` also archives any memories `derived_from` it.
pub fn run(conn: &Connection, handle: &str, cascade: bool) -> Result<()> {
    let v = forget_value(conn, handle, cascade)?;
    println!(
        "Forgot {}: archived.",
        v["label"].as_str().unwrap_or(handle),
    );
    let derived: Vec<&str> = v["derived"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let cascaded: Vec<&str> = v["cascaded"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    if !derived.is_empty() {
        if cascade {
            println!(
                "  ↳ cascaded: also archived {} derived {} ({})",
                cascaded.len(),
                if cascaded.len() == 1 {
                    "memory"
                } else {
                    "memories"
                },
                cascaded.join(", ")
            );
        } else {
            println!(
                "Warning: {} derived {} exist ({}) — review with `sara dream <label>` \
                 or archive with `sara forget <label>`, or re-run with --cascade.",
                derived.len(),
                if derived.len() == 1 {
                    "memory"
                } else {
                    "memories"
                },
                derived.join(", ")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::model::Item;

    fn seed(conn: &Connection, title: &str, tags: &[&str]) -> Item {
        let mut item = Item::new_memory(title.to_string(), format!("{title} body"), None);
        item.tags = tags.iter().map(|t| t.to_string()).collect();
        item.path = Some(String::new());
        db::insert_item(conn, &mut item).unwrap();
        item
    }

    /// Raw status lookup that (unlike `get_item_by_uuid`) doesn't filter out
    /// archived rows — needed to assert an item was actually archived.
    fn item_status(conn: &Connection, uuid: &str) -> String {
        db::item_status_for_test(conn, uuid)
    }

    #[test]
    fn forget_plain_memory_has_no_derived_children() {
        let conn = db::open_in_memory_for_test();
        let item = seed(&conn, "lone memory", &[]);
        let label = format!("m{}", item.display_id.unwrap());

        let v = forget_value(&conn, &label, false).unwrap();
        assert!(v["derived"].as_array().unwrap().is_empty());
        assert!(v["cascaded"].as_array().unwrap().is_empty());
        assert_eq!(item_status(&conn, &item.uuid.to_string()), "archived");
    }

    #[test]
    fn forget_canonical_lists_derived_children_but_does_not_archive_them() {
        let conn = db::open_in_memory_for_test();
        let canonical = seed(&conn, "canonical pattern", &[]);
        let child = seed(&conn, "derived application", &[]);
        db::insert_memory_link(
            &conn,
            &child.uuid.to_string(),
            &canonical.uuid.to_string(),
            "derived_from",
            1.0,
        )
        .unwrap();
        let canonical_label = format!("m{}", canonical.display_id.unwrap());
        let child_label = format!("m{}", child.display_id.unwrap());

        let v = forget_value(&conn, &canonical_label, false).unwrap();
        let derived: Vec<&str> = v["derived"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(derived, vec![child_label.clone()]);
        assert!(v["cascaded"].as_array().unwrap().is_empty());

        // Canonical archived, derived child still active.
        assert_eq!(item_status(&conn, &canonical.uuid.to_string()), "archived");
        assert_eq!(item_status(&conn, &child.uuid.to_string()), "active");
    }

    #[test]
    fn forget_cascade_archives_derived_children_too() {
        let conn = db::open_in_memory_for_test();
        let canonical = seed(&conn, "canonical pattern", &[]);
        let child = seed(&conn, "derived application", &[]);
        db::insert_memory_link(
            &conn,
            &child.uuid.to_string(),
            &canonical.uuid.to_string(),
            "derived_from",
            1.0,
        )
        .unwrap();
        let canonical_label = format!("m{}", canonical.display_id.unwrap());
        let child_label = format!("m{}", child.display_id.unwrap());

        let v = forget_value(&conn, &canonical_label, true).unwrap();
        let cascaded: Vec<&str> = v["cascaded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(cascaded, vec![child_label]);

        assert_eq!(item_status(&conn, &canonical.uuid.to_string()), "archived");
        assert_eq!(item_status(&conn, &child.uuid.to_string()), "archived");
    }
}
