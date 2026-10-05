use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::files::collect_files;
use super::links::{auto_link_canonical, link_handles};
use super::overlap::{TagOverlap, warn_overlap};
use super::persist::save;
use super::types::LearnRequest;
use crate::commands::shared::{memory_handle, summarize};
use crate::infrastructure::config::Config;
use crate::infrastructure::project::detect_current_project;
use crate::infrastructure::util::safety;

pub(super) fn learn(conn: &Connection, cfg: &Config, req: &LearnRequest) -> Result<Value> {
    let text = req.text.trim();
    let files = collect_files(req.files, req.auto_files)?;
    let mut auto_canonical = Vec::new();
    if !req.force {
        safety::check_size(text)?;
        safety::check_secrets(text)?;
        let primary_project = req
            .projects
            .first()
            .cloned()
            .or_else(|| detect_current_project(conn, cfg).ok().map(|(p, _)| p));
        let overlap = TagOverlap::compute(conn, req.tags)?;
        warn_overlap(conn, &overlap, &files, primary_project.as_deref())?;
        auto_canonical = overlap.canonical_candidates(conn);
    }

    let item = save(conn, cfg, text, req.tags, req.projects, req.tasks, &files)?;
    let new_uuid = item.uuid.to_string();

    let superseded = link_handles(
        conn,
        &new_uuid,
        req.supersedes,
        "supersedes",
        "--supersedes",
    )?;
    let derived_from = link_handles(
        conn,
        &new_uuid,
        req.derived_from,
        "derived_from",
        "--derived-from",
    )?;
    let similar_to = link_handles(
        conn,
        &new_uuid,
        req.similar_to,
        "similar_to",
        "--similar-to",
    )?;
    let explicit: Vec<&String> = superseded
        .iter()
        .chain(&derived_from)
        .chain(&similar_to)
        .collect();
    let auto_derived = auto_link_canonical(conn, &new_uuid, &auto_canonical, &explicit)?;

    let linked_tasks: Vec<Value> = item
        .linked_tasks
        .iter()
        .map(|(id, desc, src)| {
            json!({
                "id": id.parse::<i64>().unwrap_or(0),
                "description": desc,
                "source": src,
            })
        })
        .collect();

    Ok(json!({
        "label": memory_handle(&item),
        "uuid": &new_uuid[..8],
        "text": summarize(text),
        "tags": item.tags,
        "files": files,
        "linked_tasks": linked_tasks,
        "superseded": superseded,
        "derived_from": derived_from,
        "similar_to": similar_to,
        "auto_derived_from": auto_derived,
    }))
}
