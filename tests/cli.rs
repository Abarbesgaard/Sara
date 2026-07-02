//! Integration tests for `sara_tasks::cli`.
//! Moved out of an inline mod tests block in src/cli.rs.

use clap::{CommandFactory, Parser};
use sara_tasks::cli::*;

#[test]
fn cli_name_is_sara() {
    let cmd = Cli::command();
    assert_eq!(cmd.get_name(), "sara");
}

#[test]
fn cli_has_core_task_commands() {
    let cmd = Cli::command();
    for name in ["add", "list", "info", "done", "undo"] {
        assert!(
            cmd.find_subcommand(name).is_some(),
            "missing subcommand: {name}"
        );
    }
}

#[test]
fn mcp_subcommand_parses() {
    let cli = Cli::try_parse_from(["sara", "mcp"]).expect("mcp should parse");
    assert!(matches!(cli.command, Command::Mcp));
}

#[test]
fn list_accepts_short_and_long_project_flag() {
    for args in [
        ["sara", "list", "-p", "web"],
        ["sara", "list", "--project", "web"],
    ] {
        let cli = Cli::try_parse_from(args).expect("list should parse a project flag");
        match cli.command {
            Command::List { project, .. } => {
                assert_eq!(project.as_deref(), Some("web"), "{args:?}");
            }
            other => panic!("expected List, got {other:?}"),
        }
    }
}

#[test]
fn project_filter_short_flag_is_consistent_across_commands() {
    // `-p` must mean `--project` on every command exposing a project filter.
    assert!(Cli::try_parse_from(["sara", "list", "-p", "x"]).is_ok());
    assert!(Cli::try_parse_from(["sara", "reset", "-p", "x"]).is_ok());
    assert!(Cli::try_parse_from(["sara", "activity", "-p", "x"]).is_ok());
    // `add` takes `-p` before the trailing description.
    let cli = Cli::try_parse_from(["sara", "add", "-p", "x", "do", "thing"]).unwrap();
    match cli.command {
        Command::Add { project, words, .. } => {
            assert_eq!(project.as_deref(), Some("x"));
            assert_eq!(words, vec!["do".to_string(), "thing".to_string()]);
        }
        other => panic!("expected Add, got {other:?}"),
    }
}
