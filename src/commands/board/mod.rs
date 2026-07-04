mod render;
mod state;
mod types;

pub(super) use types::{BoardAction, BoardState, IssueNode};

use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::project::detect_current_project;
use crate::infrastructure::tui;

fn is_empty(st: &BoardState) -> bool {
    st.issues.is_empty() && st.standalone.is_empty()
}

pub fn run(conn: &Connection, cfg: &Config, project_arg: Option<&str>, finished: bool) -> Result<()> {
    let project = if let Some(p) = project_arg {
        p.to_string()
    } else {
        let (name, _) = detect_current_project(conn, cfg)?;
        name
    };

    let mut st = state::build_state(conn, project, finished, None)?;
    if is_empty(&st) {
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
                let project = st.project.clone();
                let sel = st.selected;
                st = state::build_state(conn, project, st.show_finished, Some(&st))?;
                if is_empty(&st) {
                    break;
                }
                let row_count = render::visible_rows(&st).len();
                st.selected = sel.min(row_count.saturating_sub(1));
            }
        }
    }
    Ok(())
}
