//! A tiny key/value store (`meta` table) for singleton facts about the
//! database — e.g. the embedding scheme version that lets semantic recall
//! self-heal when the bundled model or embed-text scheme changes.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

/// Read a `meta` value, or `None` if the key was never set.
pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    let value = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()?;
    Ok(value)
}

/// Set (insert or replace) a `meta` value.
pub fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}
