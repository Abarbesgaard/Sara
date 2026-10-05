mod memory;
mod web;

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::Frame;
use rusqlite::Connection;

use crate::infrastructure::tui;

pub(super) use memory::MemoryScreen;
pub(super) use web::WebScreen;

const TICK_MS: u64 = 50;

pub(super) enum Flow {
    Continue,
    Quit,
}

pub(super) trait Screen {
    fn draw(&self, f: &mut Frame);
    fn tick(&mut self);
    fn on_key(&mut self, conn: &Connection, code: KeyCode) -> Flow;
}

pub(super) fn drive(conn: &Connection, screen: &mut impl Screen) -> Result<()> {
    let mut terminal = tui::init_terminal()?;
    let res = loop {
        if let Err(e) = terminal.draw(|f| screen.draw(f)) {
            break Err(e.into());
        }
        screen.tick();
        match next_key() {
            Err(e) => break Err(e),
            Ok(Some(code)) => {
                if let Flow::Quit = screen.on_key(conn, code) {
                    break Ok(());
                }
            }
            Ok(None) => {}
        }
    };
    tui::restore_terminal()?;
    res
}

fn next_key() -> Result<Option<KeyCode>> {
    if !event::poll(Duration::from_millis(TICK_MS))? {
        return Ok(None);
    }
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => Ok(Some(key.code)),
        _ => Ok(None),
    }
}

fn step_direction(code: KeyCode) -> Option<bool> {
    match code {
        KeyCode::Tab | KeyCode::Right | KeyCode::Down => Some(true),
        KeyCode::BackTab | KeyCode::Left | KeyCode::Up => Some(false),
        _ => None,
    }
}

fn cycle(selected: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        selected
    } else if forward {
        (selected + 1) % len
    } else {
        (selected + len - 1) % len
    }
}
