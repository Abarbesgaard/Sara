#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]
// Robustness gate (GH #174): under `panic = "abort"` any user-reachable
// unwrap/expect is a hard crash with no message. Deny them in production code;
// test builds may still use unwrap/expect freely.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod cli;
mod commands;
mod completion;
mod dispatch;
mod infrastructure;
mod mcp;

#[cfg(test)]
#[path = "../tests/unit/support/mod.rs"]
mod test_support;

use anyhow::Result;
use clap::CommandFactory;
use clap::Parser;
use std::process::ExitCode;

use cli::{Cli, Command};
use infrastructure::{config, db};

fn run() -> Result<()> {
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();

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
            "doing",
            "follow",
        ];
        if ACTIONS.contains(&args[2].as_str()) {
            let id = args.remove(1);
            let action = args.remove(1);
            args.insert(1, action);
            args.insert(2, id);
        }
    }
    let command_label = args[1..].join(" ");
    let command_flags = infrastructure::telemetry::extract_cli_flags(&args[1..]);
    let command_name = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            #[cfg(feature = "telemetry")]
            capture_clap_exit(&e, &command_name, &command_flags);
            e.exit();
        }
    };

    let cfg = config::load()?;
    infrastructure::tui::theme::init(Some(&cfg.tui.theme));
    let conn = db::open()?;

    let _ = infrastructure::memory::embedding::ensure_index_current(&conn);

    if !matches!(cli.command, Command::Undo) {
        db::begin_undo_batch(&command_label);
    }

    infrastructure::telemetry::maybe_show_notice(&cfg);

    let command = cli.command;
    let started = std::time::Instant::now();
    let result = dispatch::dispatch(command, conn, cfg.clone());
    let elapsed_ms = started.elapsed().as_millis() as u64;
    // Commands that only inspect or ship telemetry must not record themselves.
    let is_telemetry_meta = command_name == "__telemetry_flush" || command_name == "telemetry";
    if !is_telemetry_meta {
        infrastructure::telemetry::capture(
            &cfg,
            infrastructure::telemetry::Source::Cli,
            &command_name,
            &command_flags,
            elapsed_ms,
            &result,
            None,
        );
        infrastructure::telemetry::spawn_flush(&cfg);
    }
    result
}

/// Record an invocation clap ends itself (usage error, `--help`, `--version`),
/// which would otherwise leave no telemetry.
#[cfg(feature = "telemetry")]
fn capture_clap_exit(e: &clap::Error, command_name: &str, command_flags: &[String]) {
    use clap::error::ErrorKind;
    let Ok(cfg) = config::load() else {
        return;
    };
    let ok = matches!(
        e.kind(),
        ErrorKind::DisplayHelp
            | ErrorKind::DisplayVersion
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
    let res: Result<()> = if ok {
        Ok(())
    } else {
        Err(anyhow::anyhow!("cli usage error"))
    };
    infrastructure::telemetry::capture(
        &cfg,
        infrastructure::telemetry::Source::Cli,
        command_name,
        command_flags,
        0,
        &res,
        None,
    );
    infrastructure::telemetry::spawn_flush(&cfg);
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
