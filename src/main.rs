#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

mod cli;
mod commands;
mod config;
mod dates;
mod db;
mod enrich;
mod files;
mod git;
mod llm;
mod model;
mod project;
mod tui;

use anyhow::Result;
use clap::CommandFactory;
use clap::Parser;
use std::io;
use std::process::ExitCode;

use cli::{Cli, Command, DepAction, ProjectAction};

fn run() -> Result<()> {
    // Taskwarrior-style shorthands:
    //   `sara <id>`          -> `sara info <id>`
    //   `sara <id> <action>` -> `sara <action> <id>`
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && args[1].parse::<i64>().is_ok() {
        args.insert(1, "info".to_string());
    } else if args.len() >= 3 && args[1].parse::<i64>().is_ok() {
        const ACTIONS: &[&str] = &[
            "start",
            "stop",
            "done",
            "delete",
            "modify",
            "info",
            "dep",
            "annotate",
            "comment",
            "attach",
            "pr",
            "link",
            "addbranch",
        ];
        if ACTIONS.contains(&args[2].as_str()) {
            let id = args.remove(1);
            let action = args.remove(1);
            args.insert(1, action);
            args.insert(2, id);
        }
    }
    let command_label = args[1..].join(" ");
    let cli = Cli::parse_from(args);

    if let Command::Provider { ref action } = cli.command {
        return commands::provider::run(action);
    }

    let cfg = config::load()?;
    let mut conn = db::open()?;

    if !matches!(cli.command, Command::Undo) {
        db::begin_undo_batch(&command_label);
    }

    match cli.command {
        Command::Init {
            name,
            goal,
            yes,
            no_llm,
        } => {
            commands::init::run(&conn, &cfg, name.as_deref(), goal.as_deref(), yes, no_llm)?;
        }

        Command::Project { action } => match action {
            ProjectAction::Init {
                name,
                goal,
                yes,
                no_llm,
            } => {
                eprintln!("note: `sara project init` is deprecated — use `sara init` instead.");
                commands::init::run(&conn, &cfg, name.as_deref(), goal.as_deref(), yes, no_llm)?;
            }
        },

        Command::Reset { project, yes } => {
            commands::reset::run(&mut conn, &cfg, project.as_deref(), yes)?;
        }

        Command::Add {
            words,
            project,
            priority,
            tag,
            yes,
            no_llm,
            every,
        } => {
            if words.is_empty() {
                anyhow::bail!("Task description cannot be empty");
            }
            commands::add::run(
                &conn,
                &cfg,
                &words,
                project.as_deref(),
                priority.as_deref(),
                &tag,
                yes,
                no_llm,
                every.as_deref(),
            )?;
        }

        Command::Info { id } => {
            commands::info::run(&conn, &cfg, &id)?;
        }

        Command::Annotate { id, text } => {
            commands::annotate::annotate(&conn, &id, &text)?;
        }

        Command::Denotate { annotation_id } => {
            commands::annotate::denotate(&conn, annotation_id)?;
        }

        Command::Attach { id, path } => {
            commands::annotate::attach(&conn, &id, &path)?;
        }

        Command::Link { id, url, label } => {
            commands::annotate::link(&conn, &id, &url, label.as_deref())?;
        }

        Command::Unlink { link_id } => {
            commands::annotate::unlink(&conn, link_id)?;
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

        Command::Addbranch { id, clear } => {
            commands::branch::run(&conn, &id, clear)?;
        }

        Command::Undo => {
            commands::undo::run(&conn)?;
        }

        Command::Provider { action } => {
            commands::provider::run(&action)?;
        }

        Command::Check { id, text } => {
            let task = db::resolve_task(&conn, &id)?;
            db::add_checklist_item(&conn, &task.uuid, &text)?;
            println!("Added checklist item to task {}", task.id.unwrap_or(0));
        }

        Command::Activity { project, all } => {
            let proj = if all {
                None
            } else if let Some(p) = project {
                Some(p)
            } else {
                let cwd = std::env::current_dir().unwrap_or_default();
                crate::project::find_git_root(&cwd)
                    .map(|root| crate::project::project_name_from_root(&root))
            };
            commands::activity::run(&conn, proj.as_deref())?;
        }

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
