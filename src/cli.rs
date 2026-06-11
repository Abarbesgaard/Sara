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
        /// Accept all LLM proposals without the TUI review form
        #[arg(short, long)]
        yes: bool,
        /// Skip LLM enrichment
        #[arg(long)]
        no_llm: bool,
    },

    /// Show full details of a task
    Info {
        /// Task id or uuid prefix
        id: String,
    },

    /// Add a comment, note, or PR/URL link to a task
    Annotate {
        /// Task id or uuid prefix
        id: String,
        /// The comment text or URL
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
    },

    /// Remove an annotation by its number (see `tk info`)
    Denotate {
        /// Annotation id (the number shown in the detail view)
        annotation_id: i64,
    },

    /// Attach a file path or URL to a task
    Attach {
        /// Task id or uuid prefix
        id: String,
        /// File path (relative to project) or URL
        path: String,
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
