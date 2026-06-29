use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use rusqlite::Connection;

use crate::infrastructure::{db, tui};

mod render;

pub fn run(conn: &Connection, project: Option<&str>) -> Result<()> {
    let counts = db::activity_counts(conn, 365, project)?;
    let stats = db::activity_stats(conn, project)?;
    let (total_created, total_completed, cur_streak, longest_streak) = stats;

    let mut terminal = tui::init_terminal()?;
    loop {
        terminal.draw(|f| {
            render::render(
                f,
                &counts,
                project,
                total_created,
                total_completed,
                cur_streak,
                longest_streak,
            )
        })?;
        if event::poll(std::time::Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => break,
                _ => {}
            }
        }
    }
    tui::restore_terminal()?;
    Ok(())
}

pub fn month_abbr(m: u32) -> &'static str {
    match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}
