use crossterm::event::KeyCode;
use ratatui::Frame;
use rusqlite::Connection;

use super::{Flow, Screen, cycle, step_direction};
use crate::commands::dream::layout::nearest_in_direction;
use crate::commands::dream::load::open;
use crate::commands::dream::render;
use crate::commands::dream::types::{Dir, DreamData, Star, WebData};

const MAX_ZOOM: f64 = 6.0;
const ZOOM_STEP: f64 = 1.25;

pub(in crate::commands::dream) struct WebScreen {
    web: WebData,
    frame: u64,
    selected: usize,
    show_help: bool,
    dream: Option<DreamData>,
    dream_frame: u64,
    dream_selected: usize,
    zoom: f64,
    search_input: Option<String>,
    query: String,
}

impl WebScreen {
    pub(in crate::commands::dream) fn new(web: WebData) -> Self {
        Self {
            web,
            frame: 0,
            selected: 0,
            show_help: false,
            dream: None,
            dream_frame: 0,
            dream_selected: 0,
            zoom: 1.0,
            search_input: None,
            query: String::new(),
        }
    }

    fn jump(&self, back: bool) -> usize {
        jump_to_match(self.selected, &self.query, &self.web.stars, back)
    }

    fn on_search_key(&mut self, code: KeyCode) {
        let Some(buf) = self.search_input.as_mut() else {
            return;
        };
        match code {
            KeyCode::Esc => {
                self.search_input = None;
                self.query.clear();
            }
            KeyCode::Enter => {
                self.query = buf.trim().to_lowercase();
                self.search_input = None;
                self.selected = self.jump(false);
            }
            KeyCode::Backspace => {
                if buf.pop().is_none() {
                    self.search_input = None;
                }
            }
            KeyCode::Char(c) => buf.push(c),
            _ => {}
        }
    }

    fn navigate(&mut self, code: KeyCode, forward: bool) {
        if let Some(d) = &self.dream {
            self.dream_selected = cycle(self.dream_selected, d.neighbors.len(), forward);
            return;
        }
        let stars = &self.web.stars;
        self.selected = match code {
            KeyCode::Right => nearest_in_direction(stars, self.selected, Dir::Right),
            KeyCode::Down => nearest_in_direction(stars, self.selected, Dir::Down),
            KeyCode::Left => nearest_in_direction(stars, self.selected, Dir::Left),
            KeyCode::Up => nearest_in_direction(stars, self.selected, Dir::Up),
            _ => cycle(self.selected, stars.len(), forward),
        };
    }

    fn on_web_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('/') => self.search_input = Some(String::new()),
            KeyCode::Char('n') => self.selected = self.jump(false),
            KeyCode::Char('N') => self.selected = self.jump(true),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.zoom = (self.zoom * ZOOM_STEP).min(MAX_ZOOM)
            }
            KeyCode::Char('-') => self.zoom = (self.zoom / ZOOM_STEP).max(1.0),
            KeyCode::Char('0') => self.zoom = 1.0,
            _ => return false,
        }
        true
    }

    fn back(&mut self) -> Flow {
        if self.show_help {
            self.show_help = false;
        } else if self.dream.is_some() {
            self.dream = None;
        } else if !self.query.is_empty() {
            self.query.clear();
        } else if self.zoom > 1.0 {
            self.zoom = 1.0;
        } else {
            return Flow::Quit;
        }
        Flow::Continue
    }

    fn dive(&mut self, conn: &Connection) {
        let target = match &self.dream {
            Some(d) => d.memory_label_at(self.dream_selected),
            None => Some(self.web.stars[self.selected].label.as_str()),
        };
        if let Some(t) = target
            && let Ok(d) = open(conn, t)
        {
            self.dream = Some(d);
            self.dream_frame = 0;
            self.dream_selected = 0;
        }
    }
}

impl Screen for WebScreen {
    fn draw(&self, f: &mut Frame) {
        match &self.dream {
            Some(d) => render::ui(
                f,
                d,
                self.dream_frame,
                self.dream_selected,
                &[self.web.stars[self.selected].label.clone()],
                self.show_help,
            ),
            None => render::ui_web(
                f,
                &self.web,
                self.frame,
                self.selected,
                self.show_help,
                self.zoom,
                self.search_input.as_deref(),
                &self.query,
            ),
        }
    }

    fn tick(&mut self) {
        self.frame += 1;
        self.dream_frame += 1;
    }

    fn on_key(&mut self, conn: &Connection, code: KeyCode) -> Flow {
        if self.dream.is_none() && self.search_input.is_some() {
            self.on_search_key(code);
            return Flow::Continue;
        }
        if self.dream.is_none() && self.on_web_key(code) {
            return Flow::Continue;
        }
        if let Some(forward) = step_direction(code) {
            self.navigate(code, forward);
            return Flow::Continue;
        }
        match code {
            KeyCode::Char('q') => return Flow::Quit,
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Esc | KeyCode::Backspace => return self.back(),
            KeyCode::Enter => self.dive(conn),
            _ => {}
        }
        Flow::Continue
    }
}

fn jump_to_match(selected: usize, query: &str, stars: &[Star], back: bool) -> usize {
    if query.is_empty() {
        return selected;
    }
    let n = stars.len();
    (1..=n)
        .map(|step| cycle_by(selected, n, step, back))
        .find(|&i| stars[i].matches(query))
        .unwrap_or(selected)
}

fn cycle_by(selected: usize, n: usize, step: usize, back: bool) -> usize {
    if back {
        (selected + n - step) % n
    } else {
        (selected + step) % n
    }
}
