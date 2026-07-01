use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Deserialize;
use uuid::Uuid;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Priority, Task};

/// One entry in the human-friendly task graph JSON.
#[derive(Debug, Deserialize)]
pub struct ImportGraphEntry {
    /// Local string reference used to wire dependencies within this batch.
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Local ids (from this batch) that this task depends on.
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub annotation: Option<String>,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
}

pub fn run(
    conn: &mut Connection,
    cfg: &Config,
    source: Option<&str>,
    project_override: Option<&str>,
) -> Result<()> {
    let raw = read_source(source)?;
    let entries: Vec<ImportGraphEntry> =
        serde_json::from_str(&raw).context("parsing import-graph JSON")?;

    anyhow::ensure!(!entries.is_empty(), "import-graph: JSON array is empty");

    let tx = conn.transaction()?;

    // Pass 1 — insert every task and its annotations/links/steps.
    let mut id_map: HashMap<String, Uuid> = HashMap::with_capacity(entries.len());
    let mut display_ids: Vec<(String, i64, String)> = Vec::new(); // (local_id, display_id, desc)

    for entry in &entries {
        anyhow::ensure!(
            !entry.id.is_empty(),
            "import-graph: every entry must have a non-empty `id` field"
        );
        anyhow::ensure!(
            !id_map.contains_key(&entry.id),
            "import-graph: duplicate id '{}'",
            entry.id
        );

        let project = project_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                crate::infrastructure::project::detect_current_project(&tx, cfg)
                    .map(|(name, _)| name)
                    .unwrap_or_else(|_| "default".to_string())
            });

        let mut task = Task::new(entry.description.clone(), project);
        task.priority = entry
            .priority
            .as_deref()
            .and_then(|p| p.to_uppercase().parse::<Priority>().ok());
        task.tags = entry.tags.clone();
        task.urgency = db::compute_urgency(&task, &cfg.urgency, false, 0);

        db::insert_task(&tx, &mut task)?;
        let display_id = task.id.unwrap_or(0);
        id_map.insert(entry.id.clone(), task.uuid);
        display_ids.push((entry.id.clone(), display_id, entry.description.clone()));

        if let Some(text) = &entry.annotation {
            db::add_annotation_full(&tx, &task.uuid, text, "comment", "ai", None, None, false)?;
        }
        for url in &entry.links {
            db::add_link(&tx, &task.uuid, url, None)?;
        }
        for step in &entry.steps {
            db::add_step(&tx, &task.uuid, step, None, "step", "human", None)?;
        }
    }

    // Pass 2 — wire dependency edges now that all tasks exist.
    for entry in &entries {
        let task_uuid = id_map[&entry.id];
        for dep_ref in &entry.depends_on {
            match id_map.get(dep_ref) {
                Some(dep_uuid) => {
                    if let Err(e) = db::add_dependency(&tx, &task_uuid, dep_uuid) {
                        eprintln!("Warning: could not add dependency {} -> {}: {e}", entry.id, dep_ref);
                    }
                }
                None => eprintln!("Warning: unknown depends_on ref '{}' in entry '{}', skipping", dep_ref, entry.id),
            }
        }
    }

    // Pass 3 — recompute urgency.
    for uuid in id_map.values() {
        db::refresh_urgency(&tx, &cfg.urgency, uuid)?;
    }

    tx.commit()?;

    println!("Imported {} task{}:", display_ids.len(), if display_ids.len() == 1 { "" } else { "s" });
    for (local_id, display_id, desc) in &display_ids {
        let short = &id_map[local_id].to_string()[..8];
        println!("  {} ({}) — {}", display_id, short, desc);
    }
    Ok(())
}

fn read_source(source: Option<&str>) -> Result<String> {
    match source {
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading graph JSON from stdin")?;
            Ok(buf)
        }
        Some(s) => {
            let path = Path::new(s);
            if path.is_file() {
                std::fs::read_to_string(path)
                    .with_context(|| format!("reading graph JSON from {}", path.display()))
            } else {
                Ok(s.to_string())
            }
        }
    }
}
