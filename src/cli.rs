use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "sara",
    about = "Sara — a personal assistant with a folder-aware task manager",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize Sara's private knowledge store (creates Sara/ with PARA folders)
    Init {
        /// Store location (defaults to ./Sara in the current directory)
        #[arg(long)]
        path: Option<PathBuf>,
    },

    /// Git project profile commands
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },

    /// Nuke a project: delete all its tasks and profile (run `sara project init` to recreate)
    Reset {
        /// Project to reset (defaults to the current project)
        #[arg(long, short)]
        project: Option<String>,
        /// Skip the confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Add a task, note, or link to Sara's store
    Add {
        /// Content: task description, note text, or URL (auto-detected)
        #[arg(trailing_var_arg = true)]
        words: Vec<String>,
        /// Add as a task (default for plain text; same as legacy `tk add`)
        #[arg(long, conflicts_with_all = ["note", "capture_link"])]
        task: bool,
        /// Add as a note in Sara's store
        #[arg(long, conflicts_with_all = ["task", "capture_link"])]
        note: bool,
        /// Add as a link in Sara's store
        #[arg(long, name = "link", conflicts_with_all = ["task", "note"])]
        capture_link: bool,
        /// Override project
        #[arg(long, short)]
        project: Option<String>,
        /// Override priority (H/M/L) — tasks only
        #[arg(long)]
        priority: Option<String>,
        /// Tag (repeatable)
        #[arg(long, short)]
        tag: Vec<String>,
        /// Accept all values without the TUI review form
        #[arg(short, long)]
        yes: bool,
        /// Skip LLM enrichment (priority/due/tags for tasks; summary/tags/PARA for notes/links)
        #[arg(long)]
        no_llm: bool,
        /// Recurrence interval: daily, weekly, monthly, 2w, 3d, 1m, etc.
        #[arg(long, visible_alias = "recur")]
        every: Option<String>,
    },

    /// Show full details of a task (id) or note/link (n1, l2)
    Info {
        /// Task id, uuid prefix, or item handle (n1, l2)
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

    /// Remove a link by its number (see `sara info`)
    Unlink {
        /// Link id (the number shown in the detail view)
        link_id: i64,
    },

    /// List tasks and optionally notes/links
    List {
        /// Show tasks for all projects (default: current project only)
        #[arg(short, long)]
        all: bool,
        /// Also list notes and links from Sara's store
        #[arg(long)]
        items: bool,
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

    /// Tie the currently active git branch to a task (snapshot on sara stop)
    Addbranch {
        /// Task id or uuid prefix
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

    /// Add a checklist item to a task
    #[clap(name = "check")]
    Check {
        /// Task ID
        id: String,
        /// Checklist item text
        text: String,
    },

    /// Show a GitHub-style activity heatmap
    #[clap(name = "activity", alias = "heat")]
    Activity {
        /// Limit to a specific project (defaults to current git project)
        #[arg(long, short)]
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

    /// Ask Sara anything — searches your store and answers with LLM
    #[command(visible_alias = "ask")]
    Search {
        /// Your question
        query: String,
    },

    /// Surface what's most relevant right now
    Brief {
        /// Skip LLM — use template brief only
        #[arg(long)]
        no_llm: bool,
    },

    /// Rebuild long-term memory from daily notes + captures (OpenClaw-style dreaming)
    Learn,

    /// Store a durable fact in long-term memory
    Remember {
        #[arg(trailing_var_arg = true, required = true)]
        words: Vec<String>,
    },

    /// Inspect and search Sara's memory (MEMORY.md + daily notes)
    Memory {
        #[command(subcommand)]
        action: Option<MemoryAction>,
        /// Query for `sara memory search`
        query: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum MemoryAction {
    /// Show long-term MEMORY.md (default)
    Show,
    /// Show today's daily notes
    Today,
    /// Search memory files by keyword
    Search,
    /// Index memory chunks for semantic search
    Index,
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
    Use {
        name: String,
    },
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
    Remove {
        name: String,
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
    use clap::CommandFactory;

    #[test]
    fn cli_name_is_sara() {
        let cmd = Cli::command();
        assert_eq!(cmd.get_name(), "sara");
    }

    #[test]
    fn cli_has_core_task_commands() {
        let cmd = Cli::command();
        for name in ["add", "list", "info", "done", "init", "undo"] {
            assert!(
                cmd.find_subcommand(name).is_some(),
                "missing subcommand: {name}"
            );
        }
    }
}
