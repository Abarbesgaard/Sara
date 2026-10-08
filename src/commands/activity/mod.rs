use anyhow::Result;
use chrono::{Local, NaiveDate};
use crossterm::event::KeyCode;
use rusqlite::Connection;

use crate::infrastructure::db::DayTask;
use crate::infrastructure::tui::keymap::{Action, KeyDispatcher, Mode};
use crate::infrastructure::{db, tui};

mod render;
mod types;

use types::{ActivityData, ActivityState, Drill};

type DayLoader<'a> = Box<dyn FnMut(NaiveDate) -> Vec<DayTask> + 'a>;

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
    let mut st = ActivityState::new(data, Local::now().date_naive());
    let mut load: DayLoader =
        Box::new(|day| db::tasks_touched_on(conn, day, project).unwrap_or_default());

    tui::with_terminal(|terminal| {
        let mut keys = KeyDispatcher::new();
        loop {
            terminal.draw(|f| render::render(f, &st))?;
            let Some(key) = tui::next_key(200)? else {
                continue;
            };
            if apply(&mut st, keys.dispatch(key, Mode::Normal), &mut load) {
                return Ok(());
            }
        }
    })
}

fn apply(st: &mut ActivityState, action: Action, load: &mut DayLoader) -> bool {
    let before = st.cursor;
    match action {
        Action::Quit | Action::Cancel => {
            if st.drill.take().is_none() {
                return true;
            }
        }
        Action::Confirm => {
            st.drill = match st.drill {
                Some(_) => None,
                None => Some(Drill {
                    day: st.cursor,
                    tasks: load(st.cursor),
                }),
            }
        }
        Action::Down => st.move_by(1),
        Action::Up => st.move_by(-1),
        Action::Top | Action::Bottom => st.cursor = st.today,
        Action::Raw(k) => match k.code {
            KeyCode::Char('h') | KeyCode::Left => st.move_by(-7),
            KeyCode::Char('l') | KeyCode::Right => st.move_by(7),
            _ => {}
        },
        _ => {}
    }
    if st.cursor != before && st.drill.is_some() {
        st.drill = Some(Drill {
            day: st.cursor,
            tasks: load(st.cursor),
        });
    }
    false
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/activity/mod.rs"]
mod tests;
