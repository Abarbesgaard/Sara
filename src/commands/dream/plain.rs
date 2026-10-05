use anyhow::Result;
use rusqlite::Connection;

use crate::commands::shared::{canonical_labels, derived_from_suffix, strength_label};

use super::load::{load_web, open};
use super::types::NodeKind;

pub(super) fn memory(conn: &Connection, handle: &str) -> Result<()> {
    let data = open(conn, handle)?;
    let (derived_labels, derived_from_labels) = canonical_labels(conn, &data.item);
    let canonical_str = if derived_labels.is_empty() {
        String::new()
    } else {
        format!(
            " [canonical, {} derived: {}]",
            derived_labels.len(),
            derived_labels.join(", ")
        )
    };
    let derived_from_str = derived_from_suffix(&derived_from_labels);
    println!(
        "{} — {} ({:.1}){}{}{}",
        data.label,
        strength_label(data.strength),
        data.strength,
        if data.provisional {
            " [provisional]"
        } else {
            ""
        },
        canonical_str,
        derived_from_str,
    );
    println!(
        "created: {}   recalls (30d): {}",
        data.item.created.to_rfc3339(),
        data.recall_total_30d
    );
    if !data.item.tags.is_empty() {
        println!("tags: {}", data.item.tags.join(", "));
    }
    if !data.files.is_empty() {
        println!("files: {}", data.files.join(", "));
    }
    for nb in &data.neighbors {
        match &nb.kind {
            NodeKind::Memory { label, relation } => println!("link: {relation} {label}"),
            NodeKind::Task { id, desc, source } => println!("task: #{id} ({source}) {desc}"),
            NodeKind::File { .. } => {}
        }
    }
    println!("\n{}", data.item.body);
    Ok(())
}

pub(super) fn web(conn: &Connection) -> Result<()> {
    let web = load_web(conn)?;
    println!("{} memories, {} bonds", web.stars.len(), web.bonds.len());
    for b in &web.bonds {
        println!(
            "{} —[{}]→ {}",
            web.stars[b.a].label, b.relation, web.stars[b.b].label
        );
    }
    Ok(())
}
