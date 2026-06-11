use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "tk",
    about = "A folder-aware, LLM-assisted task manager",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize (or update) the current project profile
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

    /// Nuke a project: delete all its tasks and profile (run `tk init` to recreate)
    Reset {
        /// Project to reset (defaults to the current project)
        #[arg(long, short)]
        project: Option<String>,
        /// Skip the confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Add a new task
    Add {
        /// Task description and optional inline tokens (project:x +tag pri:H)
        #[arg(trailing_var_arg = true, required = true)]
        words: Vec<String>,
        /// Override project
        #[arg(long, short)]
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
        /// Enrich with the LLM (priority/due/tags/dependency suggestions). Off by default.
        #[arg(long, visible_alias = "ai")]
        llm: bool,
    },

    /// Show full details of a task
    Info {
        /// Task id or uuid prefix
        id: String,
    },

    /// Add a comment, note, or PR/URL link to a task
    #[command(visible_alias = "comment")]
    Annotate {
        /// Task id or uuid prefix
        id: String,
        /// The comment text or URL
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
    },

    /// Remove a comment by its number (see `tk info`)
    #[command(visible_alias = "uncomment")]
    Denotate {
        /// Comment id (the number shown in the detail view)
        annotation_id: i64,
    },

    /// Attach a file path or URL to a task (URLs become links)
    Attach {
        /// Task id or uuid prefix
        id: String,
        /// File path (relative to project) or URL
        path: String,
    },

    /// Add a link (e.g. a GitHub PR) to a task
    Link {
        /// Task id or uuid prefix
        id: String,
        /// The URL to link
        url: String,
        /// Optional display label (auto-derived for GitHub PRs/issues)
        #[arg(long)]
        label: Option<String>,
    },

    /// Remove a link by its number (see `tk info`)
    Unlink {
        /// Link id (the number shown in the detail view)
        link_id: i64,
    },

    /// List tasks
    List {
        /// Show tasks for all projects (default: current project only)
        #[arg(short, long)]
        all: bool,
        /// Filter by project name
        #[arg(long)]
        project: Option<String>,
    },

    /// Start working on a task (begins time tracking, marks it active)
    Start {
        /// Task id or uuid prefix
        id: String,
    },

    /// Stop working on a task (accumulates time spent)
    Stop {
        /// Task id or uuid prefix
        id: String,
    },

    /// Mark a task as done
    Done {
        /// Task id or uuid prefix
        id: String,
        /// Force-complete even if blocked
        #[arg(long)]
        force: bool,
    },

    /// Modify a task (opens the review form pre-filled)
    Modify {
        /// Task id or uuid prefix
        id: String,
        /// Skip LLM re-enrichment
        #[arg(long)]
        no_llm: bool,
    },

    /// Delete a task (soft-delete)
    Delete {
        /// Task id or uuid prefix
        id: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },

    /// Manage task dependencies
    Dep {
        /// Task id or uuid prefix
        id: String,
        #[command(subcommand)]
        action: DepAction,
    },

    /// Revert the most recent command
    Undo,

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
