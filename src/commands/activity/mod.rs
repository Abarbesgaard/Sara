use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use rusqlite::Connection;

use crate::infrastructure::{db, tui};

mod render;
mod types;

use types::ActivityData;

pub fn run(conn: &Connection, project: Option<&str>) -> Result<()> {
    let counts = db::activity_counts(conn, 365, project)?;
    let (total_created, total_completed, cur_streak, longest_streak) =
        db::activity_stats(conn, project)?;

    let data = ActivityData {
        counts,
        project: project.map(str::to_owned),
        total_created,
        total_completed,
        cur_streak,
        longest_streak,
    };

    let mut terminal = tui::init_terminal()?;
    loop {
        terminal.draw(|f| render::render(f, &data))?;
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
