mod render;
mod state;
mod types;

pub(super) use types::{BoardAction, BoardState, Feature};

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

    let mut st = state::build_state(conn, project)?;
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
                st = state::build_state(conn, project)?;
                if st.tasks.is_empty() {
                    break;
                }
                st.selected = sel.min(st.tasks.len() - 1);
            }
        }
    }
    Ok(())
}
