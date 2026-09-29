use std::collections::HashMap;

use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use crate::infrastructure::db;
use crate::infrastructure::portable::Bundle;

pub(super) fn report(
    conn: &Connection,
    bundle: &Bundle,
    id_map: &HashMap<Uuid, Uuid>,
    project_override: Option<&str>,
) -> Result<()> {
    let total = bundle.tasks.len();
    let extra = total.saturating_sub(1);

    let root_line = id_map
        .get(&bundle.root)
        .and_then(|u| {
            db::get_task_by_uuid_prefix(conn, &u.to_string())
                .ok()
                .flatten()
        })
        .map(|t| {
            format!(
                "Imported task {} \"{}\" into project '{}'",
                t.id.unwrap_or(0),
                truncate(&t.description, 60),
                t.project
            )
        })
        .unwrap_or_else(|| "Imported task".to_string());

    println!("{root_line}");
    if extra > 0 {
        println!(
            "  + {extra} dependency task{} (edges remapped)",
            if extra == 1 { "" } else { "s" }
        );
    }
    if let Some(p) = project_override {
        println!("  reassigned all imported tasks to project '{p}'");
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}
