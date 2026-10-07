use std::time::{Duration as StdDuration, Instant};

use anyhow::Result;
use chrono::{Duration, Utc};
use rusqlite::Connection;

use crate::infrastructure::tui::theme::Theme;
use crate::infrastructure::{db, tui};

pub mod render;
pub mod state;

use state::{App, Mode, Outcome, is_minimal};

const POLL_MS: u64 = 500;
const TICK: StdDuration = StdDuration::from_secs(5);

pub struct FollowOptions {
    pub id: Option<String>,
    pub minimal: bool,
    pub project: Option<String>,
    pub all: bool,
    pub stall_mins: Option<i64>,
}

pub fn run(conn: &Connection, opts: FollowOptions) -> Result<()> {
    let mode = match &opts.id {
        Some(id) => Mode::Task(db::resolve_task(conn, id)?.uuid),
        None => Mode::Mission,
    };
    let stall = Duration::minutes(opts.stall_mins.unwrap_or(state::DEFAULT_STALL_MINS).max(1));
    let mut app = App::new(mode, opts.minimal, opts.project, opts.all, stall);
    app.refresh(conn, Utc::now())?;
    let theme = Theme::detect();

    tui::with_terminal(|terminal| {
        let mut version = db::data_version(conn)?;
        let mut last_refresh = Instant::now();
        let mut minimal = app.minimal;
        loop {
            terminal.draw(|f| {
                let a = f.area();
                minimal = is_minimal(app.minimal, a.width, a.height);
                render::render(f, &app, &theme);
            })?;
            if let Some(key) = tui::next_key(POLL_MS)? {
                let before = app.mode;
                if app.handle_key(key, minimal) == Outcome::Quit {
                    return Ok(());
                }
                if app.mode != before {
                    app.refresh(conn, Utc::now())?;
                    last_refresh = Instant::now();
                }
            }
            let now_version = db::data_version(conn)?;
            if now_version != version || last_refresh.elapsed() >= TICK {
                version = now_version;
                app.refresh(conn, Utc::now())?;
                last_refresh = Instant::now();
            }
        }
    })
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/follow/tests.rs"]
mod tests;
