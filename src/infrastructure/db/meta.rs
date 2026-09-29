use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    let value = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()?;
    Ok(value)
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}
