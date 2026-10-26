use crate::infrastructure::tui::keymap::{self, Action, KeyDispatcher};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rusqlite::Connection;
use std::cell::Cell;
use uuid::Uuid;

use crate::infrastructure::db::{self, FlowEvent, STEP_KIND_STEP};
use crate::infrastructure::model::Task;

pub const LIVE_WINDOW_SECS: i64 = 120;
pub const DEFAULT_STALL_MINS: i64 = 10;
pub const MINIMAL_MAX_WIDTH: u16 = 40;
pub const MINIMAL_MAX_HEIGHT: u16 = 12;
pub const SPLIT_MIN_WIDTH: u16 = 110;
pub const FEED_PER_TASK: usize = 50;
pub const FEED_MAX: usize = 200;
pub const SPARK_BUCKETS: usize = 12;
pub const SPARK_BUCKET_MINS: i64 = 5;
pub const FRESH_SECS: i64 = 15;
pub const SCROLL_PAGE: usize = 10;

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

pub fn is_split(width: u16) -> bool {
    width >= SPLIT_MIN_WIDTH
}

pub fn is_fresh(at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    (now - at).num_seconds() <= FRESH_SECS
}

/// Feed activity per `SPARK_BUCKET_MINS` bucket over the last hour, oldest first.
pub fn spark(events: &[FlowEvent], now: DateTime<Utc>) -> Vec<u32> {
    let mut buckets = vec![0; SPARK_BUCKETS];
    let width = SPARK_BUCKET_MINS * 60;
    for e in events.iter().filter(|e| in_feed(e)) {
        let ago = (now - e.at).num_seconds().max(0);
        let idx = usize::try_from(ago / width).unwrap_or(usize::MAX);
        if idx < SPARK_BUCKETS {
            buckets[SPARK_BUCKETS - 1 - idx] += 1;
        }
    }
    buckets
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub text: String,
    pub done: bool,
    pub done_at: Option<DateTime<Utc>>,
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub uuid: Uuid,
    pub id: Option<i64>,
    pub description: String,
    pub project: String,
    pub steps: Vec<Step>,
    pub doing: Option<String>,
    pub entry: DateTime<Utc>,
    pub last: DateTime<Utc>,
    pub pulse: Pulse,
    pub spark: Vec<u32>,
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

    /// How long each step took: done steps from the previous completion (or
    /// the task's creation) to their own, the current step up to `now`.
    pub fn step_times(&self, now: DateTime<Utc>) -> Vec<Option<Duration>> {
        let current = self.current();
        let mut since = self.entry;
        self.steps
            .iter()
            .enumerate()
            .map(|(i, s)| match (s.done, s.done_at) {
                (true, Some(at)) => {
                    let took = (at - since).max(Duration::zero());
                    since = since.max(at);
                    Some(took)
                }
                (false, _) if Some(i) == current => Some((now - since).max(Duration::zero())),
                _ => None,
            })
            .collect()
    }

    /// Case-insensitive match against everything the card shows; `query` must be lowercase.
    pub fn matches(&self, query: &str) -> bool {
        let current = self.current().map(|i| self.steps[i].text.as_str());
        [
            Some(self.description.as_str()),
            Some(self.project.as_str()),
            Some(self.label().as_str()),
            self.doing.as_deref(),
            current,
        ]
        .into_iter()
        .flatten()
        .any(|f| f.to_lowercase().contains(query))
    }
}

pub fn card(
    conn: &Connection,
    task: &Task,
    last: DateTime<Utc>,
    now: DateTime<Utc>,
    stall_after: Duration,
    events: &[FlowEvent],
) -> Result<Card> {
    let steps: Vec<Step> = db::get_checklist(conn, &task.uuid)?
        .into_iter()
        .filter(|i| i.kind == STEP_KIND_STEP)
        .map(|i| Step {
            done_at: i.done_at.as_deref().filter(|_| i.done).map(db::parse_at),
            result: i.result.filter(|r| !r.trim().is_empty()),
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
        entry: task.entry,
        last,
        pulse: pulse(last, now, open, stall_after),
        spark: spark(events, now),
        steps,
    })
}

/// Every open task as a card, plus the merged feed of their events.
pub fn mission(
    conn: &Connection,
    now: DateTime<Utc>,
    stall_after: Duration,
) -> Result<(Vec<Card>, Vec<FeedItem>)> {
    let mut cards = Vec::new();
    let mut items = Vec::new();
    for (t, at) in db::open_tasks_by_activity(conn, None)? {
        let events = db::flow_events(conn, &t.uuid)?;
        let c = card(conn, &t, at, now, stall_after, &events)?;
        push_feed(&mut items, &c, events);
        cards.push(c);
    }
    Ok((cards, merge_feed(items)))
}

pub fn mission_cards(
    conn: &Connection,
    now: DateTime<Utc>,
    stall_after: Duration,
) -> Result<Vec<Card>> {
    Ok(mission(conn, now, stall_after)?.0)
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

fn push_feed(items: &mut Vec<FeedItem>, c: &Card, events: Vec<FlowEvent>) {
    let events: Vec<FlowEvent> = events.into_iter().filter(in_feed).collect();
    let start = events.len().saturating_sub(FEED_PER_TASK);
    items.extend(events.into_iter().skip(start).map(|event| FeedItem {
        uuid: c.uuid,
        project: c.project.clone(),
        label: c.label(),
        event,
    }));
}

fn merge_feed(mut items: Vec<FeedItem>) -> Vec<FeedItem> {
    items.sort_by_key(|i| i.event.at);
    let start = items.len().saturating_sub(FEED_MAX);
    items.split_off(start)
}

pub fn feed(conn: &Connection, cards: &[Card]) -> Result<Vec<FeedItem>> {
    let mut items = Vec::new();
    for c in cards {
        push_feed(&mut items, c, db::flow_events(conn, &c.uuid)?);
    }
    Ok(merge_feed(items))
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
        card: card(conn, &task, last, now, stall_after, &events)?,
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
    Open(Uuid),
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
    pub paused: bool,
    pub hide_idle: bool,
    pub filter: String,
    pub filtering: bool,
    pub help: bool,
    /// Events scrolled back from the newest in the task view; 0 follows the tail.
    pub scroll: usize,
    pub frame: usize,
    /// Flow rows drawn by the last task-view render; bounds scrolling so the
    /// oldest event lands at the top of the pane instead of scrolling past it.
    pub flow_room: Cell<Option<usize>>,
    keys: KeyDispatcher,
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
            keys: KeyDispatcher::new(),
            minimal,
            project,
            all,
            selected: 0,
            stall_after,
            now: Utc::now(),
            cards: Vec::new(),
            focus: None,
            feed: Vec::new(),
            paused: false,
            hide_idle: false,
            filter: String::new(),
            filtering: false,
            help: false,
            scroll: 0,
            frame: 0,
            flow_room: Cell::new(None),
        }
    }

    pub fn shows_all_projects(&self) -> bool {
        self.project.is_none() || self.all
    }

    pub fn is_narrowed(&self) -> bool {
        self.hide_idle || !self.filter.is_empty()
    }

    pub fn visible_cards(&self, minimal: bool) -> Vec<&Card> {
        let all = self.shows_all_projects();
        let query = self.filter.to_lowercase();
        let mut cards: Vec<&Card> = self
            .cards
            .iter()
            .filter(|c| all || Some(&c.project) == self.project.as_ref())
            .filter(|c| !self.hide_idle || c.pulse != Pulse::Idle)
            .filter(|c| query.is_empty() || c.matches(&query))
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
                (self.cards, self.feed) = mission(conn, now, self.stall_after)?;
                self.focus = None;
                self.clamp_selection(self.minimal);
            }
            Mode::Task(uuid) => {
                let before = self.focus.as_ref().map(|f| f.events.len());
                let next = focus(conn, &uuid, now, self.stall_after)?;
                if let Some(before) = before
                    && self.scroll > 0
                {
                    self.scroll += next.events.len().saturating_sub(before);
                }
                self.focus = Some(next);
            }
        }
        Ok(())
    }

    fn clamp_selection(&mut self, minimal: bool) {
        let n = self.visible_cards(minimal).len();
        self.selected = self.selected.min(n.saturating_sub(1));
    }

    fn max_scroll(&self) -> usize {
        let len = self.focus.as_ref().map_or(0, |f| f.events.len());
        let room = self.flow_room.get().unwrap_or(1).max(1);
        len.saturating_sub(room)
    }

    fn scroll_by(&mut self, delta: isize) {
        let next = self.scroll.saturating_add_signed(delta);
        self.scroll = next.min(self.max_scroll());
    }

    fn target(&self, minimal: bool) -> Option<Uuid> {
        match self.mode {
            Mode::Task(uuid) => Some(uuid),
            Mode::Mission => self
                .visible_cards(minimal)
                .get(self.selected)
                .map(|c| c.uuid),
        }
    }

    fn filter_key(&mut self, key: KeyEvent, minimal: bool) {
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.filtering = false;
            }
            KeyCode::Enter => self.filtering = false,
            KeyCode::Backspace => {
                self.filter.pop();
            }
            KeyCode::Char(c) => self.filter.push(c),
            _ => {}
        }
        self.selected = 0;
        self.clamp_selection(minimal);
    }

    pub fn handle_key(&mut self, key: KeyEvent, minimal: bool) -> Outcome {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Outcome::Quit;
        }
        if self.help {
            self.help = false;
            return Outcome::Continue;
        }
        if self.filtering {
            self.filter_key(key, minimal);
            return Outcome::Continue;
        }
        let mission = self.mode == Mode::Mission;
        let page = SCROLL_PAGE as isize;
        match self.keys.dispatch(key, keymap::Mode::Normal) {
            Action::Quit if key.code == KeyCode::Esc => {
                if mission && !self.filter.is_empty() {
                    self.filter.clear();
                    self.clamp_selection(minimal);
                } else if !mission && self.opened_from_mission {
                    self.mode = Mode::Mission;
                    self.opened_from_mission = false;
                    self.scroll = 0;
                } else {
                    return Outcome::Quit;
                }
            }
            Action::Quit => return Outcome::Quit,
            Action::ToggleMark => self.paused = !self.paused,
            Action::Raw(k) if k.code == KeyCode::Char('?') => self.help = true,
            Action::Raw(k) if k.code == KeyCode::Char('m') => self.minimal = !self.minimal,
            Action::Raw(k) if k.code == KeyCode::Char('o') => {
                if let Some(uuid) = self.target(minimal) {
                    return Outcome::Open(uuid);
                }
            }
            Action::Raw(k) if k.code == KeyCode::Char('p') && mission => {
                self.all = !self.shows_all_projects();
                self.selected = 0;
            }
            Action::Raw(k) if k.code == KeyCode::Char('i') && mission => {
                self.hide_idle = !self.hide_idle;
                self.selected = 0;
            }
            Action::Raw(k) if k.code == KeyCode::Char('/') && mission => {
                self.filtering = true;
                self.selected = 0;
            }
            Action::Down | Action::PageDown if mission => {
                let step = if key.code == KeyCode::PageDown { 5 } else { 1 };
                let n = self.visible_cards(minimal).len();
                self.selected = (self.selected + step).min(n.saturating_sub(1));
            }
            Action::Up | Action::PageUp if mission => {
                let step = if key.code == KeyCode::PageUp { 5 } else { 1 };
                self.selected = self.selected.saturating_sub(step);
            }
            Action::Top if mission => self.selected = 0,
            Action::Bottom if mission => {
                self.selected = self.visible_cards(minimal).len().saturating_sub(1);
            }
            Action::Confirm if mission => {
                if let Some(c) = self.visible_cards(minimal).get(self.selected) {
                    self.mode = Mode::Task(c.uuid);
                    self.opened_from_mission = true;
                    self.scroll = 0;
                }
            }
            Action::Up => self.scroll_by(1),
            Action::Down => self.scroll_by(-1),
            Action::PageUp => self.scroll_by(page),
            Action::PageDown => self.scroll_by(-page),
            Action::Top => self.scroll = self.max_scroll(),
            Action::Bottom => self.scroll = 0,
            _ => {}
        }
        Outcome::Continue
    }
}
