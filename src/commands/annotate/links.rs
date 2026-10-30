use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

pub fn link_value(
    conn: &Connection,
    id_or_uuid: &str,
    url: &str,
    label: Option<&str>,
) -> Result<Value> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    let already_linked = db::get_links(conn, &task.uuid)?
        .iter()
        .any(|l| l.url == url);
    if !already_linked {
        db::add_link(conn, &task.uuid, url, label)?;
    }
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "url": url,
        "label": label,
        "already_linked": already_linked,
    }))
}

pub fn link(conn: &Connection, id_or_uuid: &str, url: &str, label: Option<&str>) -> Result<()> {
    let v = link_value(conn, id_or_uuid, url, label)?;
    let task = v["task"].as_i64().unwrap_or(0);
    if v["already_linked"].as_bool() == Some(true) {
        println!("Task {task} is already linked to {url}");
    } else {
        println!("Linked task {task}: {url}");
    }
    Ok(())
}

pub fn unlink_value(conn: &Connection, link_id: i64) -> Result<Value> {
    if db::delete_link(conn, link_id)? {
        Ok(json!({ "link_id": link_id, "removed": true }))
    } else {
        anyhow::bail!("No link with id {link_id}")
    }
}

pub fn unlink_url_value(conn: &Connection, id_or_uuid: &str, url: &str) -> Result<Value> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    let Some(link) = db::get_links(conn, &task.uuid)?
        .into_iter()
        .find(|l| l.url == url)
    else {
        anyhow::bail!("Task {id_or_uuid} has no link to {url}")
    };
    db::delete_link(conn, link.id)?;
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "url": url,
        "link_id": link.id,
        "removed": true,
    }))
}

pub fn unlink(conn: &Connection, link_id: i64) -> Result<()> {
    unlink_value(conn, link_id)?;
    println!("Removed link {link_id}.");
    Ok(())
}
