use anyhow::Result;
use clap::CommandFactory;
use clap::Parser;
use std::io;
use std::process::ExitCode;

use sara_tasks::cli::{self, Cli, Command, DepAction, ProjectAction};
use sara_tasks::commands;
use sara_tasks::infrastructure::{self, config, db};

fn run() -> Result<()> {
    // Dynamic shell completion: when invoked as `COMPLETE=<shell> sara …`
    // (the registration installed via `source <(COMPLETE=zsh sara)`), emit
    // completions and exit. A no-op during normal invocation.
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();

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
            "move",
            "export",
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

    let cfg = config::load()?;
    let mut conn = db::open()?;

    if !matches!(cli.command, Command::Undo) {
        db::begin_undo_batch(&command_label);
    }

    match cli.command {
        Command::Init {
            name,
            goal,
            stack,
            conventions,
            notes,
            yes,
        } => {
            commands::init::run(
                &conn,
                &cfg,
                name.as_deref(),
                goal.as_deref(),
                stack.as_deref(),
                conventions.as_deref(),
                notes.as_deref(),
                yes,
            )?;
        }

        Command::Project { action } => match action {
            ProjectAction::Init { name, goal, yes } => {
                eprintln!("note: `sara project init` is deprecated — use `sara init` instead.");
                commands::init::run(
                    &conn,
                    &cfg,
                    name.as_deref(),
                    goal.as_deref(),
                    None,
                    None,
                    None,
                    yes,
                )?;
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
            every,
            annotation,
            link,
            check,
            depends_on,
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
                every.as_deref(),
                &annotation,
                &link,
                &check,
                &depends_on,
            )?;
        }

        Command::Info {
            id,
            json,
            plain,
            md,
            history,
        } => {
            if json {
                commands::info::run_json(&conn, &cfg, &id)?;
            } else {
                commands::info::run(&conn, &cfg, &id, plain, md, history)?;
            }
        }

        Command::Annotate {
            id,
            text,
            kind,
            author,
            on,
            reconsider,
        } => {
            commands::annotate::annotate(
                &conn,
                &id,
                &text,
                kind.as_deref(),
                author.as_deref(),
                on.as_deref(),
                reconsider,
            )?;
        }

        Command::Denotate { annotation_id } => {
            commands::annotate::denotate(&conn, annotation_id)?;
        }

        Command::Attach {
            id,
            path,
            reason,
            symbol,
            lines,
            source,
        } => {
            commands::annotate::attach(
                &conn,
                &id,
                &path,
                reason.as_deref(),
                symbol.as_deref(),
                lines.as_deref(),
                source.as_deref(),
            )?;
        }

        Command::Link { id, url, label } => {
            commands::annotate::link(&conn, &id, &url, label.as_deref())?;
        }

        Command::Unlink { link_id } => {
            commands::annotate::unlink(&conn, link_id)?;
        }

        Command::Board { project } => {
            commands::board::run(&conn, &cfg, project.as_deref())?;
        }

        Command::Projects => {
            commands::projects::run(&conn, &cfg)?;
        }

        Command::List {
            all,
            project,
            json,
            by_issue,
        } => {
            commands::list::run(&conn, &cfg, all, project.as_deref(), json, by_issue)?;
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

        Command::Modify {
            id,
            description,
            priority,
            due,
            clear_due,
            tag,
            clear_tags,
        } => {
            commands::modify::run(
                &conn,
                &cfg,
                &id,
                description.as_deref(),
                priority.as_deref(),
                due.as_deref(),
                clear_due,
                &tag,
                clear_tags,
            )?;
        }

        Command::Move { id, project } => {
            commands::move_task::run(&conn, &cfg, &id, &project)?;
        }

        Command::Export { id, output } => {
            commands::export::run(&conn, &id, output.as_deref())?;
        }

        Command::Import { source, project } => {
            commands::import::run(&mut conn, &cfg, source.as_deref(), project.as_deref())?;
        }

        Command::Delete { id, yes } => {
            commands::delete::run(&conn, &id, yes)?;
        }

        Command::Dep { id, action } => match action {
            DepAction::On { other } => {
                let id = id.ok_or_else(|| anyhow::anyhow!("task id required for `dep on`"))?;
                commands::dep::run_on(&conn, &cfg, &id, &other)?;
            }
            DepAction::Off { other } => {
                let id = id.ok_or_else(|| anyhow::anyhow!("task id required for `dep off`"))?;
                commands::dep::run_off(&conn, &cfg, &id, &other)?;
            }
            DepAction::List => {
                let id = id.ok_or_else(|| anyhow::anyhow!("task id required for `dep list`"))?;
                commands::dep::run_list(&conn, &id)?;
            }
            DepAction::Chain { ids } => {
                commands::dep::run_chain(&conn, &cfg, &ids)?;
            }
        },

        Command::Addbranch { id, clear } => {
            commands::branch::run(&conn, &id, clear)?;
        }

        Command::Undo => {
            commands::undo::run(&conn)?;
        }

        Command::Check {
            id,
            text,
            intent,
            kind,
            source,
            verify,
        } => {
            let v = commands::guide::check_value(
                &conn,
                &id,
                &text,
                intent.as_deref(),
                kind.as_deref(),
                source.as_deref(),
                verify.as_deref(),
            )?;
            println!(
                "Added {} to task {}",
                v["kind"].as_str().unwrap_or("step"),
                v["task"].as_i64().unwrap_or(0)
            );
        }

        Command::Next { id, json } => {
            commands::guide::next(&conn, &cfg, &id, json)?;
        }

        Command::Steps { id, until, json } => {
            commands::guide::steps(&conn, &cfg, &id, until, json)?;
        }

        Command::Step { action } => match action {
            cli::StepAction::Done {
                id,
                n,
                result,
                kind,
            } => {
                commands::guide::step_done(
                    &conn,
                    &cfg,
                    &id,
                    n,
                    result.as_deref(),
                    kind.as_deref(),
                )?;
            }
            cli::StepAction::Undone { id, n, kind } => {
                commands::guide::step_undone(&conn, &cfg, &id, n, kind.as_deref())?;
            }
            cli::StepAction::Remove { id, n, kind } => {
                commands::guide::step_remove(&conn, &cfg, &id, n, kind.as_deref())?;
            }
        },

        Command::Verify { id, step, run } => {
            commands::guide::verify(&conn, &cfg, &id, step, run)?;
        }

        Command::Recall { query, limit, json } => {
            commands::recall::run(&conn, &cfg, &query.join(" "), limit, json)?;
        }

        Command::Assignment { id, text } => {
            commands::guide::assignment(&conn, &id, &text.join(" "))?;
        }

        Command::Rationale { id, text } => {
            commands::guide::rationale(&conn, &id, &text.join(" "))?;
        }

        Command::Validate { id } => {
            commands::guide::validate(&conn, &id)?;
        }

        Command::Feedback { id, json } => {
            commands::guide::feedback(&conn, &id, json)?;
        }

        Command::Resolve { feedback_id } => {
            commands::guide::resolve(&conn, feedback_id)?;
        }

        Command::Plan { action } => match action {
            cli::PlanAction::Import { source } => {
                commands::plan::import(&conn, &cfg, &source)?;
            }
            cli::PlanAction::Show { id, json } => {
                commands::plan::show(&conn, &cfg, &id, json)?;
            }
        },

        Command::Activity { project, all } => {
            let proj = if all {
                None
            } else if let Some(p) = project {
                Some(p)
            } else {
                let cwd = std::env::current_dir().unwrap_or_default();
                infrastructure::project::find_git_root(&cwd)
                    .map(|root| infrastructure::project::project_name_from_root(&root))
            };
            commands::activity::run(&conn, proj.as_deref())?;
        }

        Command::Sync => {
            commands::sync::run(&conn, &cfg)?;
        }

        Command::Mcp => {
            // This terminal arm moves `conn` (the server owns it for its lifetime).
            // That's fine even though other arms borrow `&conn`/`&mut conn`: match
            // arms are mutually exclusive, and `conn` is not used after the match.
            commands::mcp::run(conn, &cfg)?;
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
