mod render;
mod state;
mod types;

pub(super) use types::{BoardAction, BoardState, Feature, GroupMode};

use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::project::detect_current_project;
use crate::infrastructure::tui;

pub fn run(conn: &Connection, cfg: &Config, project_arg: Option<&str>) -> Result<()> {
    let project = if let Some(p) = project_arg {
        p.to_string()
    } else {
        let (name, _) = detect_current_project(conn, cfg)?;
        name
    };

    let mut st = state::build_state(conn, project, GroupMode::Feature)?;
    if st.tasks.is_empty() {
        println!("No tasks for project '{}'.", st.project);
        return Ok(());
    }

    loop {
        let mut terminal = tui::init_terminal()?;
        let action = render::board_loop(&mut terminal, &mut st)?;
        tui::restore_terminal()?;

        match action {
            BoardAction::Quit => break,
            BoardAction::OpenTask(uuid) => {
                crate::commands::info::run(conn, cfg, &uuid, false, false, false)?;
                // Reload — status/dependencies may have changed in the detail view.
                let project = std::mem::take(&mut st.project);
                let sel = st.selected;
                st = state::build_state(conn, project, st.mode)?;
                if st.tasks.is_empty() {
                    break;
                }
                st.selected = sel.min(st.tasks.len() - 1);
            }
            BoardAction::ToggleGrouping => {
                // Regrouping the same task set can't make it empty (every
                // task lands in a group or the trailing "ungrouped" bucket
                // either way), so there's no emptiness check here unlike the
                // other two branches.
                let project = std::mem::take(&mut st.project);
                let mode = st.mode.toggled();
                st = state::build_state(conn, project, mode)?;
            }
        }
    }
    Ok(())
}
