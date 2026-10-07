use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::tui::keymap::{Action, KeyDispatcher, Mode};
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
    let mut keys = KeyDispatcher::new();
    loop {
        terminal.draw(|f| render::render(f, &data))?;
        let Some(key) = tui::next_key(200)? else {
            continue;
        };
        if closes(keys.dispatch(key, Mode::Normal)) {
            break;
        }
    }
    tui::restore_terminal()?;
    Ok(())
}

fn closes(action: Action) -> bool {
    matches!(action, Action::Quit | Action::Confirm)
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/activity/mod.rs"]
mod tests;
