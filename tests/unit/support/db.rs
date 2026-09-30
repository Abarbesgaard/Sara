use rusqlite::Connection;
use uuid::Uuid;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Item, Task};

pub fn cfg() -> Config {
    Config::default()
}

pub fn seed_task(conn: &Connection, desc: &str, project: &str) -> Task {
    let mut task = Task::new(desc.to_string(), project.to_string());
    db::insert_task(conn, &mut task).unwrap();
    task
}

pub fn memory(title: &str, body: &str) -> Item {
    let mut item = Item::new_memory(title.to_string(), body.to_string(), None);
    item.path = Some(String::new());
    item
}

pub fn insert_memory(conn: &Connection, mut item: Item) -> Item {
    db::insert_item(conn, &mut item).unwrap();
    item
}

pub fn seed_memory(conn: &Connection, title: &str, body: &str, tags: &[&str]) -> Item {
    let mut item = memory(title, body);
    item.tags = tags.iter().map(|t| t.to_string()).collect();
    insert_memory(conn, item)
}

pub fn link(conn: &Connection, from: Uuid, relation: &str, to: Uuid) {
    db::insert_memory_link(conn, &from.to_string(), &to.to_string(), relation, 1.0).unwrap();
}
