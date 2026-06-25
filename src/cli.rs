use clap::{Parser, Subcommand};
use clap_complete::engine::ArgValueCandidates;

use crate::completion::{projects, task_ids};

#[derive(Debug, Parser)]
#[command(
    name = "sara",
    about = "Sara — folder-aware task manager (successor to tk)",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize (or update) the project profile for the current folder
    Init {
        /// Override the project name
        #[arg(long)]
        name: Option<String>,
        /// Set the project goal directly (skips prompt)
        #[arg(long)]
        goal: Option<String>,
        /// Accept all detected/LLM values non-interactively
        #[arg(short, long)]
        yes: bool,
        /// Skip LLM task seeding
        #[arg(long)]
        no_llm: bool,
    },

    /// Git project profile commands (deprecated: use `sara init`)
    #[command(hide = true)]
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },

    /// Nuke a project: delete all its tasks and profile (run `sara init` to recreate)
    Reset {
        /// Project to reset (defaults to the current project)
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
        /// Skip the confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Add a task
    Add {
        #[arg(trailing_var_arg = true)]
        words: Vec<String>,
        /// Override project
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
        /// Override priority (H/M/L)
        #[arg(long)]
        priority: Option<String>,
        /// Tag (repeatable)
        #[arg(long, short)]
        tag: Vec<String>,
        /// Accept all values without the TUI review form
        #[arg(short, long)]
        yes: bool,
        /// Skip LLM enrichment
        #[arg(long)]
        no_llm: bool,
        /// Recurrence interval: daily, weekly, monthly, 2w, 3d, 1m, etc.
        #[arg(long, visible_alias = "recur")]
        every: Option<String>,
    },

    /// Show full details of a task
    Info {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// Emit the full guide as JSON (for agents/scripts)
        #[arg(long)]
        json: bool,
    },

    /// Add a comment, note, or anchored feedback to a task
    #[command(visible_alias = "comment")]
    Annotate {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// The comment / note text or URL
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
        /// Note kind: comment|finding|thought|constraint|assumption|open_question|non_goal|decision|risk|pattern
        #[arg(long)]
        kind: Option<String>,
        /// Author of the note: human|ai
        #[arg(long)]
        author: Option<String>,
        /// Anchor the comment to a guide element: step:N, acceptance:N, anchor:ID, note:ID
        #[arg(long)]
        on: Option<String>,
        /// Flag the targeted element for the LLM to reconsider
        #[arg(long)]
        reconsider: bool,
    },

    /// Remove a comment by its number (see `sara info`)
    #[command(visible_alias = "uncomment")]
    Denotate {
        /// Comment id (the number shown in the detail view)
        annotation_id: i64,
    },

    /// Attach a file path or URL to a task (URLs become links)
    #[command(visible_alias = "pr")]
    Attach {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// File path (relative to project) or URL
        path: String,
        /// Why this file matters (turns it into a code anchor)
        #[arg(long)]
        reason: Option<String>,
        /// Specific symbol (function/type) to change
        #[arg(long)]
        symbol: Option<String>,
        /// Line range, e.g. 10:57
        #[arg(long)]
        lines: Option<String>,
        /// Provenance: human (default) or ai (records as a suggestion)
        #[arg(long)]
        source: Option<String>,
    },

    /// Add a link (e.g. a GitHub PR) to a task
    Link {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// The URL to link
        url: String,
        /// Optional display label (auto-derived for GitHub PRs/issues)
        #[arg(long)]
        label: Option<String>,
    },

    /// Remove a link by its number (see `sara info`)
    Unlink {
        /// Link id (the number shown in the detail view)
        link_id: i64,
    },

    /// Show all tasks for a project as a board (pending + completed with strikethrough)
    Board {
        /// Project name (defaults to current git project)
        #[arg(long, short)]
        project: Option<String>,
    },

    /// List pending tasks
    List {
        /// Show tasks for all projects (default: current project only)
        #[arg(short, long)]
        all: bool,
        /// Filter by project name
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
        /// Emit the list as JSON
        #[arg(long)]
        json: bool,
    },

    /// Start working on a task (begins time tracking, marks it active)
    Start {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
    },

    /// Stop working on a task (accumulates time spent)
    Stop {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
    },

    /// Mark a task as done
    Done {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// Force-complete even if blocked
        #[arg(long)]
        force: bool,
    },

    /// Modify a task (opens the review form pre-filled)
    Modify {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// Skip LLM re-enrichment
        #[arg(long)]
        no_llm: bool,
    },

    /// Move a task to another project (non-interactive)
    #[command(visible_alias = "mv")]
    Move {
        /// Task id or uuid prefix
        id: String,
        /// Target project name
        project: String,
    },

    /// Export a task (and its dependency closure) to a portable copy-paste blob
    Export {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// Write the blob to a file instead of stdout
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
    },

    /// Import a task bundle from a portable blob (file, argument, or stdin)
    Import {
        /// Path to a blob file, or the blob string itself; omit to read stdin
        source: Option<String>,
        /// Reassign every imported task to this project
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
    },

    /// Delete a task (soft-delete)
    Delete {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },

    /// Manage task dependencies
    Dep {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        #[command(subcommand)]
        action: DepAction,
    },

    /// Tie the currently active git branch to a task (snapshot on sara stop)
    Addbranch {
        /// Task id or uuid prefix
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// Remove the tied branch
        #[arg(long)]
        clear: bool,
    },

    /// Revert the most recent command
    Undo,

    /// Manage LLM provider profiles (switch on the fly without editing config)
    Provider {
        #[command(subcommand)]
        action: ProviderAction,
    },

    /// Add a checklist item / step / acceptance criterion to a task
    #[clap(name = "check")]
    Check {
        /// Task ID
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        /// Step / criterion text
        text: String,
        /// Fuller "what this step does" intent
        #[arg(long)]
        intent: Option<String>,
        /// Item kind: step (default) or acceptance
        #[arg(long)]
        kind: Option<String>,
        /// Provenance: human (default) or ai
        #[arg(long)]
        source: Option<String>,
        /// Command that verifies this step / criterion
        #[arg(long)]
        verify: Option<String>,
    },

    /// Show the next not-done step (the execution cursor)
    Next {
        /// Task id or uuid prefix
        id: String,
        /// Emit as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show ordered steps, optionally up to checkmark N ("implement until N")
    Steps {
        /// Task id or uuid prefix
        id: String,
        /// Only show steps 1..=N
        #[arg(long)]
        until: Option<usize>,
        /// Emit as JSON
        #[arg(long)]
        json: bool,
    },

    /// Mark steps done/undone with an execution record
    Step {
        #[command(subcommand)]
        action: StepAction,
    },

    /// Print (and optionally run) verification commands / acceptance criteria
    Verify {
        /// Task id or uuid prefix
        id: String,
        /// Only verify step N
        #[arg(long)]
        step: Option<usize>,
        /// Actually run the verification command(s)
        #[arg(long)]
        run: bool,
    },

    /// Cross-task memory: keyword search across tasks/findings/anchors
    Recall {
        /// Search query
        #[arg(trailing_var_arg = true, required = true)]
        query: Vec<String>,
        /// Max results
        #[arg(long, default_value_t = 20)]
        limit: i64,
        /// Emit as JSON
        #[arg(long)]
        json: bool,
    },

    /// Hand a task back to Sara to improve the guide with her LLM
    Refine {
        /// Task id or uuid prefix
        id: String,
        /// Only address flagged-for-reconsider feedback
        #[arg(long)]
        only_flagged: bool,
    },

    /// Set the originating assignment/prompt for a task
    Assignment {
        /// Task id or uuid prefix
        id: String,
        /// The assignment text
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
    },

    /// Set the rationale (why this task exists)
    Rationale {
        /// Task id or uuid prefix
        id: String,
        /// The rationale text
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
    },

    /// Stamp the guide as validated against the current git HEAD
    Validate {
        /// Task id or uuid prefix
        id: String,
    },

    /// List open feedback (human comments) for a task
    Feedback {
        /// Task id or uuid prefix
        id: String,
        /// Emit as JSON
        #[arg(long)]
        json: bool,
    },

    /// Resolve a piece of feedback by its id
    Resolve {
        /// Feedback (annotation) id
        feedback_id: i64,
    },

    /// Atomic plan ingestion and dependency-ordered briefings
    Plan {
        #[command(subcommand)]
        action: PlanAction,
    },

    /// Show a GitHub-style activity heatmap
    #[clap(name = "activity", alias = "heat")]
    Activity {
        /// Limit to a specific project (defaults to current git project)
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
        /// Show activity across all projects
        #[arg(long, short)]
        all: bool,
    },

    /// Print config and data directory paths
    Paths,

    /// Generate shell completions
    Completions {
        /// Shell type
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProjectAction {
    /// Initialize (or update) the current git project profile
    Init {
        /// Override the project name
        #[arg(long)]
        name: Option<String>,
        /// Set the project goal directly (skips prompt)
        #[arg(long)]
        goal: Option<String>,
        /// Accept all detected/LLM values non-interactively
        #[arg(short, long)]
        yes: bool,
        /// Skip LLM task seeding
        #[arg(long)]
        no_llm: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProviderAction {
    /// List configured provider profiles and show the active one
    List,
    /// Switch to a named profile (or "default" to revert to [llm] block)
    Use { name: String },
    /// Add a new named provider profile (and activate it)
    Add {
        /// Profile name (e.g. "azure", "mlx", "gpt4o")
        name: String,
        /// Provider type: azure | openai | mlx | ollama | anthropic
        #[arg(long = "type", short = 't')]
        provider_type: String,
        /// Model name
        #[arg(long, short)]
        model: String,
        /// Base URL (required for azure/mlx/ollama)
        #[arg(long, short)]
        url: Option<String>,
        /// API key
        #[arg(long, short)]
        key: Option<String>,
    },
    /// Remove a named profile
    Remove { name: String },
}

#[derive(Debug, Subcommand)]
pub enum StepAction {
    /// Mark step N done, recording an execution result + commit
    Done {
        /// Task id or uuid prefix
        id: String,
        /// 1-based step number
        n: usize,
        /// Execution result / note
        #[arg(long)]
        result: Option<String>,
        /// Item kind: step (default) or acceptance
        #[arg(long)]
        kind: Option<String>,
    },
    /// Reopen step N
    Undone {
        /// Task id or uuid prefix
        id: String,
        /// 1-based step number
        n: usize,
        /// Item kind: step (default) or acceptance
        #[arg(long)]
        kind: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlanAction {
    /// Ingest a whole task graph from JSON (file path, or '-' for stdin)
    Import {
        /// Path to the plan JSON file, or '-' to read stdin
        source: String,
    },
    /// Emit a dependency-ordered briefing for a task and its blockers
    Show {
        /// Task id or uuid prefix
        id: String,
        /// Emit as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum DepAction {
    /// Add a dependency: task depends ON another task
    On {
        /// The id or uuid prefix of the task this task depends on
        other: String,
    },
    /// Remove a dependency
    Off {
        /// The id or uuid prefix to remove as a dependency
        other: String,
    },
    /// List dependencies of this task
    List,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

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
}
