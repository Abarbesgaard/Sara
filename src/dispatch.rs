use anyhow::Result;
use clap::CommandFactory;
use rusqlite::Connection;
use std::io;

use crate::cli::{self, Cli, Command, DepAction, ProjectAction};
use crate::commands;
use crate::infrastructure::{self, config};
use crate::mcp;

pub fn dispatch(command: Command, mut conn: Connection, cfg: config::Config) -> Result<()> {
    match command {
        Command::Init {
            name,
            goal,
            stack,
            conventions,
            notes,
            setup_cmd,
            test_cmd,
            lint_cmd,
            run_cmd,
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
                setup_cmd.as_deref(),
                test_cmd.as_deref(),
                lint_cmd.as_deref(),
                run_cmd.as_deref(),
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
                    None,
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
            let req = commands::add::AddRequest {
                words: &words,
                project: project.as_deref(),
                priority: priority.as_deref(),
                tags: &tag,
                recur: every.as_deref(),
                annotations: &annotation,
                links: &link,
                checks: &check,
                depends_on: &depends_on,
            };
            commands::add::run(&conn, &cfg, &req, yes)?;
        }

        Command::Begin {
            words,
            project,
            priority,
            tag,
            file,
            assignment,
            why,
            check,
            verify,
            json,
        } => {
            if words.is_empty() {
                anyhow::bail!("Task description cannot be empty");
            }
            commands::begin::run(
                &conn,
                &cfg,
                &words.join(" "),
                &tag,
                &file,
                project.as_deref(),
                priority.as_deref(),
                assignment.as_deref(),
                why.as_deref(),
                check.as_deref(),
                verify.as_deref(),
                json,
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

        Command::Board { project, finished } => {
            commands::board::run(&conn, &cfg, project.as_deref(), finished)?;
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
            estimate,
            clear_estimate,
            every,
            clear_recur,
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
                estimate.as_deref(),
                clear_estimate,
                every.as_deref(),
                clear_recur,
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
            if let Some(w) = v["warning"].as_str() {
                eprintln!("warning: {w}");
            }
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
                json,
            } => {
                commands::guide::step_done(
                    &conn,
                    &cfg,
                    &id,
                    n,
                    result.as_deref(),
                    kind.as_deref(),
                    json,
                )?;
            }
            cli::StepAction::Undone { id, n, kind, json } => {
                commands::guide::step_undone(&conn, &cfg, &id, n, kind.as_deref(), json)?;
            }
            cli::StepAction::Remove { id, n, kind, json } => {
                commands::guide::step_remove(&conn, &cfg, &id, n, kind.as_deref(), json)?;
            }
        },

        Command::Verify {
            id,
            step,
            run,
            tick_on_pass,
        } => {
            commands::guide::verify(&conn, &cfg, &id, step, run, tick_on_pass)?;
        }

        Command::Learn {
            text,
            tag,
            project,
            task,
            file,
            auto_files,
            force,
            supersedes,
            derived_from,
            similar_to,
        } => {
            commands::learn::run(
                &conn,
                &cfg,
                &commands::learn::LearnRequest {
                    text: &text.join(" "),
                    tags: &tag,
                    projects: &project,
                    tasks: &task,
                    files: &file,
                    auto_files,
                    force,
                    supersedes: &supersedes,
                    derived_from: &derived_from,
                    similar_to: &similar_to,
                },
            )?;
        }

        Command::Recall {
            query,
            tag,
            project,
            file,
            top,
            limit,
            spread,
            semantic,
            task,
            json,
        } => {
            let effective_limit = top.unwrap_or(limit);
            let _ = semantic;
            let cfg = cfg.clone();
            commands::recall::run(
                &conn,
                &cfg,
                &query.join(" "),
                &tag,
                &project,
                &file,
                effective_limit,
                spread,
                task.as_deref(),
                json,
            )?;
        }

        Command::ReindexEmbeddings => {
            let n = infrastructure::memory::embedding::reindex_all(&conn)?;
            println!("Embedded {n} memories into the semantic index.");
        }

        Command::Consolidate {
            window_days,
            bucket_secs,
            delta,
            max_bucket,
        } => {
            let n = infrastructure::memory::graph::consolidate(
                &conn,
                window_days,
                chrono::Duration::seconds(bucket_secs),
                delta,
                max_bucket,
            )?;
            if n == 0 {
                println!("Nothing to consolidate — no co-firing recalls in the window.");
            } else {
                println!("Consolidated {n} co-activation synapse(s) from recent recalls.");
            }
        }

        Command::Forget {
            handle, cascade, ..
        } => {
            commands::forget::run(&conn, &handle, cascade)?;
        }

        Command::Promote { handle } => {
            commands::promote::run(&conn, &handle)?;
        }

        Command::Dream { handle } => match handle {
            Some(h) => commands::dream::run(&conn, &h)?,
            None => commands::dream::run_web(&conn)?,
        },

        Command::Relearn {
            handle,
            text,
            tag,
            file,
            force,
        } => {
            let joined = text.join(" ");
            let body = if joined.trim().is_empty() {
                None
            } else {
                Some(joined.as_str())
            };
            commands::relearn::run(&conn, &handle, body, &tag, &file, force)?;
        }

        Command::Tags { json } => {
            commands::tags::run(&conn, json)?;
        }

        Command::Memories { json } => {
            commands::memories::run(&conn, json)?;
        }

        Command::LinkMemory {
            from,
            relation,
            to,
            weight,
        } => {
            commands::link_memory::run(&conn, &from, &relation, &to, weight)?;
        }

        Command::UnlinkMemory { from, relation, to } => {
            commands::link_memory::unlink(&conn, &from, &relation, &to)?;
        }

        Command::PruneMemories {
            dry_run,
            apply,
            weak_days,
            provisional_days,
        } => {
            let actual_dry_run = !apply && dry_run;
            commands::prune_memories::run(&conn, weak_days, provisional_days, actual_dry_run)?;
            let scope = infrastructure::project::detect_current_project(&conn, &cfg)
                .ok()
                .map(|(name, _)| name);
            if let Ok(diag) = commands::diagnose_memories::diagnose_value(
                &conn,
                commands::diagnose_memories::DEFAULT_CONFLICT_THRESHOLD,
                scope.as_deref(),
                None,
            ) {
                let n = diag["count"].as_u64().unwrap_or(0);
                if n > 0 {
                    println!(
                        "\n{n} memory pair(s) may be duplicates or contradictions — run `sara diagnose-memories` to review."
                    );
                }
            }
        }

        Command::Doctor { json, strict } => {
            commands::doctor::run(&conn, json, strict)?;
        }

        Command::DiagnoseMemories {
            threshold,
            project,
            limit,
            json,
        } => {
            commands::diagnose_memories::run(&conn, json, threshold, project.as_deref(), limit)?;
        }

        Command::Reflect {
            min_weight,
            max_cluster,
            apply,
            json,
        } => {
            commands::reflect::run(&conn, min_weight, max_cluster, json, apply)?;
        }

        Command::Assignment { id, text } => {
            commands::guide::assignment(&conn, &id, &text.join(" "))?;
        }

        Command::Rationale { id, text } => {
            commands::guide::rationale(&conn, &id, &text.join(" "))?;
        }

        Command::Validate { id, no_run, fresh } => {
            commands::guide::validate(&conn, &id, no_run, fresh)?;
        }

        Command::Feedback { id, json } => {
            commands::guide::feedback(&conn, &id, json)?;
        }

        Command::Resolve { feedback_id, run } => {
            commands::guide::resolve(&conn, feedback_id, run)?;
        }

        Command::RecordRun {
            id,
            kind,
            model,
            provider,
            prompt,
            response,
        } => {
            commands::guide::record_run(
                &conn,
                &id,
                &kind,
                model.as_deref(),
                provider.as_deref(),
                prompt.as_deref(),
                response.as_deref(),
            )?;
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
                crate::infrastructure::project::find_git_root(&cwd)
                    .map(|root| crate::infrastructure::project::project_name_from_root(&root))
            };
            commands::activity::run(&conn, proj.as_deref())?;
        }

        Command::Sync => {
            commands::sync::run(&conn, &cfg)?;
        }

        Command::Mcp => {
            mcp::run(conn, &cfg)?;
        }

        Command::Paths => {
            let cfg_path = config::config_path()?;
            let db_path = config::db_path()?;
            println!("Config: {}", cfg_path.display());
            println!("Database: {}", db_path.display());
        }

        #[cfg(feature = "telemetry")]
        Command::Telemetry { action, show, json } => {
            if show {
                let records = infrastructure::telemetry::read_queue()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&records)?);
                } else if records.is_empty() {
                    println!("No telemetry records queued yet.");
                } else {
                    println!(
                        "{} queued telemetry record(s) — this is exactly what would be sent:",
                        records.len()
                    );
                    for r in &records {
                        println!("{}", serde_json::to_string(r)?);
                    }
                }
            } else {
                match action.as_deref().unwrap_or("status") {
                    "on" | "off" => {
                        let on = action.as_deref() == Some("on");
                        let mut c = config::load()?;
                        c.telemetry.enabled = on;
                        config::save(&c)?;
                        println!("Telemetry {}.", if on { "enabled" } else { "disabled" });
                    }
                    "status" => {
                        let env_off = std::env::var("SARA_NO_TELEMETRY")
                            .map(|v| !v.is_empty())
                            .unwrap_or(false);
                        let active = infrastructure::telemetry::enabled(&cfg);
                        println!(
                            "Telemetry: {} (config.enabled={}, SARA_NO_TELEMETRY {})",
                            if active { "on" } else { "off" },
                            cfg.telemetry.enabled,
                            if env_off { "set" } else { "unset" },
                        );
                        println!(
                            "Queue: {}",
                            infrastructure::telemetry::queue_path()?.display()
                        );
                    }
                    other => {
                        anyhow::bail!(
                            "unknown telemetry action '{other}' (expected on|off|status, or --show)"
                        );
                    }
                }
            }
        }

        #[cfg(not(feature = "telemetry"))]
        Command::Telemetry { .. } => {
            println!("{}", infrastructure::telemetry::NOT_COMPILED);
        }

        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut io::stdout());
        }

        Command::TelemetryFlush => {
            #[cfg(feature = "telemetry")]
            let _ = infrastructure::telemetry::flush(&cfg);
        }
    }
    Ok(())
}
