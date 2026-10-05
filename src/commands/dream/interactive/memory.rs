use crossterm::event::KeyCode;
use ratatui::Frame;
use rusqlite::Connection;

use super::{Flow, Screen, cycle, step_direction};
use crate::commands::dream::load::{navigate_back, open};
use crate::commands::dream::render;
use crate::commands::dream::types::DreamData;

pub(in crate::commands::dream) struct MemoryScreen {
    data: DreamData,
    frame: u64,
    selected: usize,
    breadcrumb: Vec<String>,
    show_help: bool,
}

impl MemoryScreen {
    pub(in crate::commands::dream) fn new(data: DreamData) -> Self {
        Self {
            data,
            frame: 0,
            selected: 0,
            breadcrumb: vec![],
            show_help: false,
        }
    }

    fn show(&mut self, data: DreamData) {
        self.data = data;
        self.frame = 0;
        self.selected = 0;
    }

    fn back(&mut self, conn: &Connection) -> bool {
        match navigate_back(conn, &mut self.breadcrumb) {
            Some(d) => {
                self.show(d);
                true
            }
            None => false,
        }
    }
}

impl Screen for MemoryScreen {
    fn draw(&self, f: &mut Frame) {
        render::ui(
            f,
            &self.data,
            self.frame,
            self.selected,
            &self.breadcrumb,
            self.show_help,
        );
    }

    fn tick(&mut self) {
        self.frame += 1;
    }

    fn on_key(&mut self, conn: &Connection, code: KeyCode) -> Flow {
        if let Some(forward) = step_direction(code) {
            self.selected = cycle(self.selected, self.data.neighbors.len(), forward);
            return Flow::Continue;
        }
        match code {
            KeyCode::Char('q') => return Flow::Quit,
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Esc => {
                if self.show_help {
                    self.show_help = false;
                } else if self.breadcrumb.is_empty() || !self.back(conn) {
                    return Flow::Quit;
                }
            }
            KeyCode::Backspace => {
                self.back(conn);
            }
            KeyCode::Enter => {
                if let Some(label) = self.data.memory_label_at(self.selected)
                    && let Ok(d) = open(conn, label)
                {
                    self.breadcrumb.push(self.data.label.clone());
                    self.show(d);
                }
            }
            _ => {}
        }
        Flow::Continue
    }
}
