use std::sync::{Mutex, MutexGuard, OnceLock};

use ratatui::{Frame, Terminal, backend::TestBackend};
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Item, Task};

pub fn conn() -> Connection {
    db::open_in_memory_for_test()
}

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

pub fn link(conn: &Connection, from: &Item, relation: &str, to: &Item) {
    db::insert_memory_link(
        conn,
        &from.uuid.to_string(),
        &to.uuid.to_string(),
        relation,
        1.0,
    )
    .unwrap();
}

pub fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub fn render_to_string(w: u16, h: u16, draw: impl FnOnce(&mut Frame)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(draw).unwrap();
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let mut out = String::new();
    for y in 0..area.height {
        let line: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}
