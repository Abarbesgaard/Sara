use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

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
        db::set_item_files(conn, &item.uuid, files)?;
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
    let updated: Vec<&str> = v["updated"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    println!(
        "Relearned {} (updated: {}): {}",
        v["label"].as_str().unwrap_or(handle),
        updated.join(", "),
        v["title"].as_str().unwrap_or(""),
    );
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
mod tests {
    use super::*;
    use crate::infrastructure::model::Item;

    fn seed(conn: &Connection) -> Item {
        let mut item = Item::new_memory(
            "old title".to_string(),
            "old body about frobnicators".to_string(),
            None,
        );
        item.tags = vec!["oldtag".to_string()];
        item.path = Some(String::new());
        db::insert_item(conn, &mut item).unwrap();
        item
    }

    #[test]
    fn relearn_replaces_body_and_reindexes_fts() {
        let conn = db::open_in_memory_for_test();
        let item = seed(&conn);
        let label = format!("m{}", item.display_id.unwrap());

        relearn_value(
            &conn,
            &label,
            Some("new body about widgets"),
            &[],
            &[],
            false,
        )
        .unwrap();

        let loaded = db::get_item_by_uuid(&conn, &item.uuid.to_string()).unwrap();
        assert_eq!(loaded.body, "new body about widgets");
        assert_eq!(loaded.title, "new body about widgets");
        // FTS reflects the new body, not the old one.
        assert_eq!(db::search_fts(&conn, "widgets", 10).unwrap().len(), 1);
        assert!(
            db::search_fts(&conn, "frobnicators", 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn relearn_replaces_tags_and_preserves_body_and_created() {
        let conn = db::open_in_memory_for_test();
        let item = seed(&conn);
        let label = format!("m{}", item.display_id.unwrap());

        relearn_value(&conn, &label, None, &["newtag".to_string()], &[], false).unwrap();

        let loaded = db::get_item_by_uuid(&conn, &item.uuid.to_string()).unwrap();
        assert_eq!(loaded.body, "old body about frobnicators");
        assert_eq!(loaded.tags, vec!["newtag".to_string()]);
        assert_eq!(loaded.created, item.created);
        assert_eq!(db::find_items_by_tag(&conn, "newtag").unwrap().len(), 1);
        assert!(db::find_items_by_tag(&conn, "oldtag").unwrap().is_empty());
    }

    #[test]
    fn relearn_requires_at_least_one_change() {
        let conn = db::open_in_memory_for_test();
        let item = seed(&conn);
        let label = format!("m{}", item.display_id.unwrap());

        assert!(relearn_value(&conn, &label, None, &[], &[], false).is_err());
    }

    #[test]
    fn relearn_refreshes_a_stale_semantic_embedding() {
        use crate::infrastructure::embedding;

        let conn = db::open_in_memory_for_test();
        let item = seed(&conn);
        let label = format!("m{}", item.display_id.unwrap());

        // Index the memory as `learn` would, then capture the stored vector.
        embedding::index_memory(&conn, &item);
        let before = db::get_embedding(&conn, &item.uuid.to_string())
            .unwrap()
            .expect("memory should be indexed");

        // Edit the body to something semantically unrelated.
        relearn_value(
            &conn,
            &label,
            Some("kubernetes pod eviction under memory pressure"),
            &[],
            &[],
            false,
        )
        .unwrap();

        let after = db::get_embedding(&conn, &item.uuid.to_string())
            .unwrap()
            .expect("embedding must still exist after relearn");
        assert_ne!(
            before, after,
            "relearn must refresh the embedding so semantic recall matches the new body"
        );
        // And it must equal a fresh embedding of the new text.
        let loaded = db::get_item_by_uuid(&conn, &item.uuid.to_string()).unwrap();
        let recomputed = {
            embedding::index_memory(&conn, &loaded);
            db::get_embedding(&conn, &item.uuid.to_string())
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            after, recomputed,
            "embedding must reflect the new body text"
        );
    }

    #[test]
    fn relearn_does_not_index_an_unembedded_memory() {
        let conn = db::open_in_memory_for_test();
        let item = seed(&conn);
        let label = format!("m{}", item.display_id.unwrap());

        // Never indexed (semantic recall was off at learn time) → relearn must
        // not fabricate an embedding.
        relearn_value(&conn, &label, Some("a brand new body"), &[], &[], false).unwrap();
        assert!(
            db::get_embedding(&conn, &item.uuid.to_string())
                .unwrap()
                .is_none(),
            "relearn must not create an embedding for a memory that had none"
        );
    }

    #[test]
    fn relearn_enforces_safety_guardrails_on_new_body() {
        let conn = db::open_in_memory_for_test();
        let item = seed(&conn);
        let label = format!("m{}", item.display_id.unwrap());

        assert!(relearn_value(&conn, &label, Some("api_key=verysecret"), &[], &[], false).is_err());
        // --force bypasses
        assert!(relearn_value(&conn, &label, Some("api_key=verysecret"), &[], &[], true).is_ok());
    }
}
