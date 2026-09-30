use std::collections::HashMap;

use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use crate::commands::shared::{plural, truncate};
use crate::infrastructure::db;
use crate::infrastructure::util::portable::Bundle;

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
            plural(extra)
        );
    }
    if let Some(p) = project_override {
        println!("  reassigned all imported tasks to project '{p}'");
    }
    Ok(())
}
