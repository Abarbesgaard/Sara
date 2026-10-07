use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rusqlite::Connection;
use uuid::Uuid;

use crate::infrastructure::db::{self, FlowEvent, STEP_KIND_STEP};
use crate::infrastructure::model::Task;

pub const LIVE_WINDOW_SECS: i64 = 120;
pub const DEFAULT_STALL_MINS: i64 = 10;
pub const MINIMAL_MAX_WIDTH: u16 = 40;
pub const MINIMAL_MAX_HEIGHT: u16 = 12;
pub const FEED_PER_TASK: usize = 50;
pub const FEED_MAX: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pulse {
    Live,
    Idle,
    Stalled,
}

pub fn pulse(
    last: DateTime<Utc>,
    now: DateTime<Utc>,
    open_steps: usize,
    stall_after: Duration,
) -> Pulse {
    let quiet = now - last;
    if quiet <= Duration::seconds(LIVE_WINDOW_SECS) {
        Pulse::Live
    } else if open_steps > 0 && quiet >= stall_after {
        Pulse::Stalled
    } else {
        Pulse::Idle
    }
}

pub fn is_minimal(forced: bool, width: u16, height: u16) -> bool {
    forced || width < MINIMAL_MAX_WIDTH || height < MINIMAL_MAX_HEIGHT
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub uuid: Uuid,
    pub id: Option<i64>,
    pub description: String,
    pub project: String,
    pub steps: Vec<Step>,
    pub doing: Option<String>,
    pub last: DateTime<Utc>,
    pub pulse: Pulse,
}

impl Card {
    pub fn label(&self) -> String {
        match self.id {
            Some(id) => id.to_string(),
            None => self.uuid.to_string()[..8].to_string(),
        }
    }

    pub fn done_count(&self) -> usize {
        self.steps.iter().filter(|s| s.done).count()
    }

    pub fn current(&self) -> Option<usize> {
        self.steps.iter().position(|s| !s.done)
    }
}

pub fn card(
    conn: &Connection,
    task: &Task,
    last: DateTime<Utc>,
    now: DateTime<Utc>,
    stall_after: Duration,
) -> Result<Card> {
    let steps: Vec<Step> = db::get_checklist(conn, &task.uuid)?
        .into_iter()
        .filter(|i| i.kind == STEP_KIND_STEP)
        .map(|i| Step {
            text: i.text,
            done: i.done,
        })
        .collect();
    let open = steps.iter().filter(|s| !s.done).count();
    Ok(Card {
        uuid: task.uuid,
        id: task.id,
        description: task.description.clone(),
        project: task.project.clone(),
        doing: db::latest_doing(conn, &task.uuid)?.map(|a| a.text),
        last,
        pulse: pulse(last, now, open, stall_after),
        steps,
    })
}

pub fn mission_cards(
    conn: &Connection,
    now: DateTime<Utc>,
    stall_after: Duration,
) -> Result<Vec<Card>> {
    db::open_tasks_by_activity(conn, None)?
        .iter()
        .map(|(t, at)| card(conn, t, *at, now, stall_after))
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedItem {
    pub uuid: Uuid,
    pub project: String,
    pub label: String,
    pub event: FlowEvent,
}

pub fn in_feed(e: &FlowEvent) -> bool {
    !matches!(e.kind, db::FlowKind::Change(_))
}

pub fn feed(conn: &Connection, cards: &[Card]) -> Result<Vec<FeedItem>> {
    let mut items = Vec::new();
    for c in cards {
        let events: Vec<FlowEvent> = db::flow_events(conn, &c.uuid)?
            .into_iter()
            .filter(in_feed)
            .collect();
        let start = events.len().saturating_sub(FEED_PER_TASK);
        items.extend(events.into_iter().skip(start).map(|event| FeedItem {
            uuid: c.uuid,
            project: c.project.clone(),
            label: c.label(),
            event,
        }));
    }
    items.sort_by_key(|i| i.event.at);
    let start = items.len().saturating_sub(FEED_MAX);
    Ok(items.split_off(start))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Focus {
    pub card: Card,
    pub events: Vec<FlowEvent>,
}

pub fn focus(
    conn: &Connection,
    uuid: &Uuid,
    now: DateTime<Utc>,
    stall_after: Duration,
) -> Result<Focus> {
    let task = db::get_task_by_uuid_prefix(conn, &uuid.to_string())?
        .ok_or_else(|| anyhow::anyhow!("task {uuid} no longer exists"))?;
    let events = db::flow_events(conn, uuid)?;
    let last = events
        .last()
        .map_or(task.modified, |e| e.at.max(task.entry));
    Ok(Focus {
        card: card(conn, &task, last, now, stall_after)?,
        events,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Mission,
    Task(Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Quit,
}

#[derive(Debug, Clone)]
pub struct App {
    pub mode: Mode,
    pub opened_from_mission: bool,
    pub minimal: bool,
    pub project: Option<String>,
    pub all: bool,
    pub selected: usize,
    pub stall_after: Duration,
    pub now: DateTime<Utc>,
    pub cards: Vec<Card>,
    pub focus: Option<Focus>,
    pub feed: Vec<FeedItem>,
}

impl App {
    pub fn new(
        mode: Mode,
        minimal: bool,
        project: Option<String>,
        all: bool,
        stall_after: Duration,
    ) -> Self {
        Self {
            mode,
            opened_from_mission: false,
            minimal,
            project,
            all,
            selected: 0,
            stall_after,
            now: Utc::now(),
            cards: Vec::new(),
            focus: None,
            feed: Vec::new(),
        }
    }

    pub fn shows_all_projects(&self) -> bool {
        self.project.is_none() || self.all
    }

    pub fn visible_cards(&self, minimal: bool) -> Vec<&Card> {
        let all = self.shows_all_projects();
        let mut cards: Vec<&Card> = self
            .cards
            .iter()
            .filter(|c| all || Some(&c.project) == self.project.as_ref())
            .collect();
        if minimal {
            let mut order: Vec<&str> = Vec::new();
            for c in &cards {
                if !order.contains(&c.project.as_str()) {
                    order.push(&c.project);
                }
            }
            let rank = |p: &str| order.iter().position(|o| *o == p).unwrap_or(0);
            cards.sort_by_key(|c| (rank(&c.project), c.pulse == Pulse::Idle));
        }
        cards
    }

    pub fn visible_feed(&self, minimal: bool) -> Vec<&FeedItem> {
        let visible: Vec<Uuid> = self.visible_cards(minimal).iter().map(|c| c.uuid).collect();
        self.feed
            .iter()
            .filter(|i| visible.contains(&i.uuid))
            .collect()
    }

    pub fn refresh(&mut self, conn: &Connection, now: DateTime<Utc>) -> Result<()> {
        self.now = now;
        match self.mode {
            Mode::Mission => {
                self.cards = mission_cards(conn, now, self.stall_after)?;
                self.feed = feed(conn, &self.cards)?;
                self.focus = None;
                let n = self.visible_cards(self.minimal).len();
                self.selected = self.selected.min(n.saturating_sub(1));
            }
            Mode::Task(uuid) => {
                self.focus = Some(focus(conn, &uuid, now, self.stall_after)?);
            }
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyEvent, minimal: bool) -> Outcome {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Outcome::Quit;
        }
        match key.code {
            KeyCode::Char('q') => return Outcome::Quit,
            KeyCode::Esc => {
                if matches!(self.mode, Mode::Task(_)) && self.opened_from_mission {
                    self.mode = Mode::Mission;
                    self.opened_from_mission = false;
                } else {
                    return Outcome::Quit;
                }
            }
            KeyCode::Char('m') => self.minimal = !self.minimal,
            KeyCode::Char('p') if self.mode == Mode::Mission => {
                self.all = !self.shows_all_projects();
                self.selected = 0;
            }
            KeyCode::Down | KeyCode::Char('j') if self.mode == Mode::Mission => {
                let n = self.visible_cards(minimal).len();
                if self.selected + 1 < n {
                    self.selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.mode == Mode::Mission => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Enter if self.mode == Mode::Mission => {
                if let Some(c) = self.visible_cards(minimal).get(self.selected) {
                    self.mode = Mode::Task(c.uuid);
                    self.opened_from_mission = true;
                }
            }
            _ => {}
        }
        Outcome::Continue
    }
}
