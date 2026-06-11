mod cli;
mod commands;
mod config;
mod dates;
mod db;
mod enrich;
mod files;
mod llm;
mod model;
mod project;
mod tui;

use anyhow::Result;
use clap::CommandFactory;
use clap::Parser;
use std::io;
use std::process::ExitCode;

use cli::{Cli, Command, DepAction};

fn run() -> Result<()> {
    // Taskwarrior-style shorthands:
    //   `tk <id>`          -> `tk info <id>`
    //   `tk <id> <action>` -> `tk <action> <id>`   (start/stop/done/delete/modify/info/dep)
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && args[1].parse::<i64>().is_ok() {
        args.insert(1, "info".to_string());
    } else if args.len() >= 3 && args[1].parse::<i64>().is_ok() {
        const ACTIONS: &[&str] =
            &["start", "stop", "done", "delete", "modify", "info", "dep"];
        if ACTIONS.contains(&args[2].as_str()) {
            let id = args.remove(1); // remove id
            let action = args.remove(1); // remove action (now at idx 1)
            args.insert(1, action);
            args.insert(2, id);
        }
    }
    let cli = Cli::parse_from(args);
    let cfg = config::load()?;
    let conn = db::open()?;

    match cli.command {
        Command::Init { name, goal, yes, no_llm } => {
            commands::init::run(
                &conn,
                &cfg,
                name.as_deref(),
                goal.as_deref(),
                yes,
                no_llm,
            )?;
        }

        Command::Add { words, project, priority, tag, yes, no_llm } => {
            commands::add::run(
                &conn,
                &cfg,
                &words,
                project.as_deref(),
                priority.as_deref(),
                &tag,
                yes,
                no_llm,
            )?;
        }

        Command::Info { id } => {
            commands::info::run(&conn, &id)?;
        }

        Command::List { all, project } => {
            commands::list::run(&conn, &cfg, all, project.as_deref())?;
        }

        Command::Start { id } => {
            commands::timer::start(&conn, &cfg, &id)?;
        }

        Command::Stop { id } => {
            commands::timer::stop(&conn, &cfg, &id)?;
        }

        Command::Done { id, force } => {
            commands::done::run(&conn, &cfg, &id, force)?;
        }

        Command::Modify { id, no_llm } => {
            commands::modify::run(&conn, &cfg, &id, no_llm)?;
        }

        Command::Delete { id, yes } => {
            commands::delete::run(&conn, &id, yes)?;
        }

        Command::Dep { id, action } => match action {
            DepAction::On { other } => {
                commands::dep::run_on(&conn, &cfg, &id, &other)?;
            }
            DepAction::Off { other } => {
                commands::dep::run_off(&conn, &cfg, &id, &other)?;
            }
            DepAction::List => {
                commands::dep::run_list(&conn, &id)?;
            }
        },

        Command::Paths => {
            let cfg_path = config::config_path()?;
            let db_path = config::db_path()?;
            println!("Config: {}", cfg_path.display());
            println!("Database: {}", db_path.display());
        }

        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut io::stdout());
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let use_color = std::env::var("NO_COLOR").is_err();
            if use_color {
                eprintln!("\x1b[31merror\x1b[0m: {e}");
            } else {
                eprintln!("error: {e}");
            }
            for cause in e.chain().skip(1) {
                if use_color {
                    eprintln!("  \x1b[33mcaused by\x1b[0m: {cause}");
                } else {
                    eprintln!("  caused by: {cause}");
                }
            }
            ExitCode::FAILURE
        }
    }
}
