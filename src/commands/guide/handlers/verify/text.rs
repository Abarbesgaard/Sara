use anyhow::Result;
use rusqlite::Connection;

use super::commands::{all_commands, nth_step};
use super::tick;
use crate::commands::guide::support::{shell, working_dir};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub fn verify(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    step: Option<usize>,
    run: bool,
    tick_on_pass: bool,
) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    let dir = working_dir(conn, &task.project);

    if tick_on_pass {
        return tick::run(conn, &task, &steps, &acceptance, step, dir.as_deref());
    }

    let cmds: Vec<String> = match step {
        Some(n) => {
            let s = nth_step(&steps, n)?;
            if s.verify_cmd.is_none() {
                println!("Step {n} has no verify command.");
            }
            s.verify_cmd.iter().cloned().collect()
        }
        None => all_commands(conn, &task, &steps, &acceptance)?,
    };

    if !acceptance.is_empty() && step.is_none() {
        println!("Acceptance criteria:");
        for (i, a) in acceptance.iter().enumerate() {
            let mark = if a.done { "[x]" } else { "[ ]" };
            println!("  {} {}. {}", mark, i + 1, a.text);
        }
    }

    if cmds.is_empty() {
        println!("No verification commands found.");
        return Ok(());
    }

    if run && db::ensure_started(conn, &task.uuid)? {
        println!("Task {} is now active.", task.id.unwrap_or(0));
    }

    for cmd in &cmds {
        if !run {
            println!("{cmd}");
            continue;
        }
        println!("$ {cmd}");
        match shell(cmd, dir.as_deref()).status() {
            Ok(s) if s.success() => println!("  ok: passed"),
            Ok(s) => println!("  exited with {}", s.code().unwrap_or(-1)),
            Err(e) => println!("  failed to run: {e}"),
        }
    }
    Ok(())
}
