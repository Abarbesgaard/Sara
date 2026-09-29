use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

mod render;

/// Print-free core shared by the CLI `relearn` command and the MCP `relearn`
/// tool. Edits a memory in place — body, tags, and/or file associations —
/// preserving its uuid, label, created date, status, task links, and memory
/// links. This replaces the lossy `forget` + `learn` cycle for fixing a stale
/// sentence or retagging.
pub fn relearn_value(
    conn: &Connection,
    handle: &str,
    text: Option<&str>,
    tags: &[String],
    files: &[String],
    force: bool,
) -> Result<Value> {
    let text = text.map(str::trim).filter(|t| !t.is_empty());
    if text.is_none() && tags.is_empty() && files.is_empty() {
        anyhow::bail!(
            "Nothing to change — pass new body text, --tag, and/or --file.\n\
             (Tags and files each REPLACE the existing set.)"
        );
    }

    let mut item = db::get_item_by_handle(conn, handle)?;
    if item.kind != "memory" {
        anyhow::bail!("{handle} is not a memory.");
    }

    let mut updated: Vec<&str> = vec![];

    if let Some(body) = text {
        if !force {
            crate::infrastructure::safety::check_memory_body(body)?;
        }
        item.body = body.to_string();
        item.title = summarize(body);
        item.summary = None;
        updated.push("body");
    }

    if !tags.is_empty() {
        item.tags = tags.to_vec();
        updated.push("tags");
    }

    item.modified = chrono::Utc::now();
    db::update_item(conn, &item)?;

    // The body/title drives the semantic embedding. `update_item` re-indexes FTS
    // via its trigger, but embeddings are only written explicitly — so an edited
    // body would otherwise leave a stale vector and `recall --semantic` would
    // keep matching the OLD text. Refresh it here whenever the body changed, but
    // only for memories that were already indexed (preserving the learn-time
    // decision to embed or not, without needing the Config).
    if text.is_some() && matches!(db::get_embedding(conn, &item.uuid.to_string()), Ok(Some(_))) {
        crate::infrastructure::embedding::index_memory(conn, &item);
    }

    if !files.is_empty() {
        // Resolve to absolute exactly as `learn` and `recall` do. Storing the
        // raw relative path here would make the memory invisible to
        // `recall --file`, which resolves its argument before matching.
        let resolved: Vec<String> = files
            .iter()
            .map(|p| crate::infrastructure::project::resolve_file_link_here(p))
            .collect();
        db::set_item_files(conn, &item.uuid, &resolved)?;
        updated.push("files");
    }

    Ok(json!({
        "label": handle,
        "uuid": &item.uuid.to_string()[..8],
        "updated": updated,
        "title": item.title,
        "tags": item.tags,
    }))
}

/// `sara relearn <label> [--tag <t>]… [--file <f>]… [<new body text>]`
pub fn run(
    conn: &Connection,
    handle: &str,
    text: Option<&str>,
    tags: &[String],
    files: &[String],
    force: bool,
) -> Result<()> {
    let v = relearn_value(conn, handle, text, tags, files, force)?;
    render::print_relearned(&v, handle);
    Ok(())
}

/// A short title for display, taken from the start of the memory text.
/// Duplicated from the `learn` slice to keep the vertical-slice boundary
/// the architecture tests enforce.
fn summarize(text: &str) -> String {
    const MAX: usize = 80;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let truncated: String = trimmed.chars().take(MAX).collect();
        format!("{}…", truncated.trim_end())
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/relearn/mod.rs"]
mod tests;
