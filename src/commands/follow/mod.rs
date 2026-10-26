use std::time::{Duration as StdDuration, Instant};

use anyhow::Result;
use chrono::{Duration, Utc};
use ratatui::Terminal;
use ratatui::backend::Backend;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::tui::theme::Theme;
use crate::infrastructure::{db, tui};

pub mod render;
pub mod state;

use state::{App, Mode, Outcome, is_minimal};

const POLL_MS: u64 = 120;
const FRAME_MS: u128 = 120;
const TICK: StdDuration = StdDuration::from_secs(5);

pub struct FollowOptions {
    pub id: Option<String>,
    pub minimal: bool,
    pub project: Option<String>,
    pub all: bool,
    pub stall_mins: Option<i64>,
}

pub fn run(conn: &Connection, cfg: &Config, opts: FollowOptions) -> Result<()> {
    let mode = match &opts.id {
        Some(id) => Mode::Task(db::resolve_task(conn, id)?.uuid),
        None => Mode::Mission,
    };
    let stall = Duration::minutes(opts.stall_mins.unwrap_or(state::DEFAULT_STALL_MINS).max(1));
    let mut app = App::new(mode, opts.minimal, opts.project, opts.all, stall);
    app.refresh(conn, Utc::now())?;
    let theme = Theme::detect();

    loop {
        match tui::with_terminal(|t| event_loop(t, conn, &mut app, &theme))? {
            Outcome::Open(uuid) => {
                crate::commands::info::run(conn, cfg, &uuid.to_string(), false, false, false)?;
                app.refresh(conn, Utc::now())?;
            }
            _ => return Ok(()),
        }
    }
}

fn event_loop<B: Backend<Error: Send + Sync + 'static>>(
    terminal: &mut Terminal<B>,
    conn: &Connection,
    app: &mut App,
    theme: &Theme,
) -> Result<Outcome> {
    let started = Instant::now();
    let mut version = db::data_version(conn)?;
    let mut last_refresh = Instant::now();
    let mut minimal = app.minimal;
    loop {
        if !app.paused {
            app.frame = (started.elapsed().as_millis() / FRAME_MS) as usize;
        }
        terminal.draw(|f| {
            let a = f.area();
            minimal = is_minimal(app.minimal, a.width, a.height);
            render::render(f, app, theme);
        })?;
        if let Some(key) = tui::next_key(POLL_MS)? {
            let before = app.mode;
            let was_paused = app.paused;
            match app.handle_key(key, minimal) {
                Outcome::Continue => {}
                exit => return Ok(exit),
            }
            if app.mode != before || (was_paused && !app.paused) {
                app.refresh(conn, Utc::now())?;
                last_refresh = Instant::now();
            }
        }
        if app.paused {
            continue;
        }
        let now_version = db::data_version(conn)?;
        if now_version != version || last_refresh.elapsed() >= TICK {
            version = now_version;
            app.refresh(conn, Utc::now())?;
            last_refresh = Instant::now();
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/follow/tests.rs"]
mod tests;
