use clap::{Parser, Subcommand};
use clap_complete::engine::ArgValueCandidates;

use crate::completion::{memory_labels, projects, task_ids};

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
    Init {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        #[arg(long)]
        stack: Option<String>,
        #[arg(long)]
        conventions: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        setup_cmd: Option<String>,
        #[arg(long)]
        test_cmd: Option<String>,
        #[arg(long)]
        lint_cmd: Option<String>,
        #[arg(long)]
        run_cmd: Option<String>,
        #[arg(short, long)]
        yes: bool,
    },

    #[command(hide = true)]
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },

    Reset {
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
        #[arg(short, long)]
        yes: bool,
    },

    Add {
        words: Vec<String>,
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long, short)]
        tag: Vec<String>,
        #[arg(short, long)]
        yes: bool,
        #[arg(long, visible_alias = "recur")]
        every: Option<String>,
        #[arg(long)]
        annotation: Vec<String>,
        #[arg(long)]
        link: Vec<String>,
        #[arg(long)]
        check: Vec<String>,
        #[arg(long, add = ArgValueCandidates::new(task_ids))]
        depends_on: Vec<String>,
    },

    Begin {
        words: Vec<String>,
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long, short)]
        tag: Vec<String>,
        #[arg(long)]
        file: Vec<String>,
        #[arg(long)]
        assignment: Option<String>,
        #[arg(long, visible_alias = "rationale")]
        why: Option<String>,
        #[arg(long)]
        check: Option<String>,
        #[arg(long)]
        verify: Option<String>,
        #[arg(long)]
        json: bool,
    },

    Info {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        plain: bool,
        #[arg(long)]
        md: bool,
        #[arg(long)]
        history: bool,
    },

    #[command(visible_alias = "comment")]
    Annotate {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        #[arg(required = true)]
        text: Vec<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        author: Option<String>,
        #[arg(long)]
        on: Option<String>,
        #[arg(long)]
        reconsider: bool,
    },

    #[command(visible_alias = "uncomment")]
    Denotate {
        annotation_id: i64,
    },

    #[command(visible_alias = "pr")]
    Attach {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        path: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        lines: Option<String>,
        #[arg(long)]
        source: Option<String>,
    },

    Link {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        url: String,
        #[arg(long)]
        label: Option<String>,
    },

    Unlink {
        link_id: i64,
    },

    Board {
        #[arg(long, short)]
        project: Option<String>,
        #[arg(long)]
        finished: bool,
    },

    Projects,

    List {
        #[arg(short, long)]
        all: bool,
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        by_issue: bool,
    },

    Start {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
    },

    Stop {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
    },

    Done {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        #[arg(long)]
        force: bool,
    },

    Modify {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        clear_due: bool,
        #[arg(long, short)]
        tag: Vec<String>,
        #[arg(long)]
        clear_tags: bool,
        #[arg(long)]
        estimate: Option<String>,
        #[arg(long)]
        clear_estimate: bool,
        #[arg(long, visible_alias = "recur")]
        every: Option<String>,
        #[arg(long)]
        clear_recur: bool,
    },

    #[command(visible_alias = "mv")]
    Move {
        id: String,
        project: String,
    },

    Export {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
    },

    Import {
        source: Option<String>,
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
    },

    Delete {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        #[arg(short, long)]
        yes: bool,
    },

    Dep {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: Option<String>,
        #[command(subcommand)]
        action: DepAction,
    },

    Addbranch {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        #[arg(long)]
        clear: bool,
    },

    Undo,

    #[clap(name = "check")]
    Check {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        text: String,
        #[arg(long)]
        intent: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        verify: Option<String>,
    },

    Next {
        id: String,
        #[arg(long)]
        json: bool,
    },

    Steps {
        id: String,
        #[arg(long)]
        until: Option<usize>,
        #[arg(long)]
        json: bool,
    },

    Step {
        #[command(subcommand)]
        action: StepAction,
    },

    Verify {
        id: String,
        #[arg(long)]
        step: Option<usize>,
        #[arg(long)]
        run: bool,
        #[arg(long)]
        tick_on_pass: bool,
    },

    Learn {
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
        #[arg(long, short)]
        tag: Vec<String>,
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Vec<String>,
        #[arg(long, add = ArgValueCandidates::new(task_ids))]
        task: Vec<String>,
        #[arg(long)]
        file: Vec<String>,
        #[arg(long)]
        auto_files: bool,
        #[arg(long)]
        force: bool,
        #[arg(long, add = ArgValueCandidates::new(memory_labels))]
        supersedes: Vec<String>,
        #[arg(long = "derived-from", add = ArgValueCandidates::new(memory_labels))]
        derived_from: Vec<String>,
        #[arg(long = "similar-to", add = ArgValueCandidates::new(memory_labels))]
        similar_to: Vec<String>,
    },

    Recall {
        query: Vec<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Vec<String>,
        #[arg(long)]
        file: Vec<String>,
        #[arg(long)]
        top: Option<i64>,
        #[arg(long, default_value_t = 10)]
        limit: i64,
        #[arg(long)]
        spread: bool,
        #[arg(long)]
        semantic: bool,
        #[arg(long)]
        json: bool,
    },

    ReindexEmbeddings,
    Consolidate {
        #[arg(long, default_value_t = 30)]
        window_days: i64,
        #[arg(long, default_value_t = 5)]
        bucket_secs: i64,
        #[arg(long, default_value_t = 0.1)]
        delta: f64,
        #[arg(long, default_value_t = 5)]
        max_bucket: usize,
    },

    Forget {
        handle: String,
        #[arg(long, short)]
        yes: bool,
        #[arg(long)]
        cascade: bool,
    },

    Promote {
        handle: String,
    },

    Dream {
        handle: Option<String>,
    },

    Relearn {
        handle: String,
        #[arg(trailing_var_arg = true)]
        text: Vec<String>,
        #[arg(long, short)]
        tag: Vec<String>,
        #[arg(long)]
        file: Vec<String>,
        #[arg(long)]
        force: bool,
    },

    Tags {
        #[arg(long)]
        json: bool,
    },

    Memories {
        #[arg(long)]
        json: bool,
    },

    Telemetry {
        action: Option<String>,
        #[arg(long)]
        show: bool,
        #[arg(long)]
        json: bool,
    },

    #[command(name = "__telemetry_flush", hide = true)]
    TelemetryFlush,

    #[command(name = "link-memory")]
    LinkMemory {
        from: String,
        relation: String,
        to: String,
        #[arg(long, default_value = "1.0")]
        weight: f64,
    },

    #[command(name = "unlink-memory")]
    UnlinkMemory {
        from: String,
        relation: String,
        to: String,
    },

    #[command(name = "prune-memories")]
    PruneMemories {
        #[arg(long, default_value = "true")]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        apply: bool,
        #[arg(long, default_value = "90")]
        weak_days: i64,
        #[arg(long, default_value = "30")]
        provisional_days: i64,
    },

    #[command(name = "diagnose-memories", alias = "conflicts")]
    DiagnoseMemories {
        #[arg(long, default_value_t = crate::commands::diagnose_memories::DEFAULT_CONFLICT_THRESHOLD)]
        threshold: f32,
        #[arg(long, short)]
        project: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },

    Reflect {
        #[arg(long, default_value_t = crate::commands::reflect::DEFAULT_MIN_WEIGHT)]
        min_weight: f64,
        #[arg(long, default_value_t = crate::commands::reflect::DEFAULT_MAX_CLUSTER)]
        max_cluster: usize,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },

    Assignment {
        id: String,
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
    },

    Rationale {
        id: String,
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
    },

    Validate {
        id: String,
        #[arg(long)]
        no_run: bool,
        #[arg(long)]
        fresh: bool,
    },

    Feedback {
        id: String,
        #[arg(long)]
        json: bool,
    },

    Resolve {
        feedback_id: i64,
        #[arg(long)]
        run: Option<i64>,
    },

    RecordRun {
        #[arg(add = ArgValueCandidates::new(task_ids))]
        id: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        response: Option<String>,
    },

    Plan {
        #[command(subcommand)]
        action: PlanAction,
    },

    #[clap(name = "activity", alias = "heat")]
    Activity {
        #[arg(long, short, add = ArgValueCandidates::new(projects))]
        project: Option<String>,
        #[arg(long, short)]
        all: bool,
    },

    Sync,

    Mcp,

    Paths,

    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProjectAction {
    Init {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum StepAction {
    Done {
        id: String,
        n: usize,
        #[arg(long)]
        result: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Undone {
        id: String,
        n: usize,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(visible_alias = "rm")]
    Remove {
        id: String,
        n: usize,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlanAction {
    Import {
        source: String,
    },
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum DepAction {
    On {
        other: String,
    },
    Off {
        other: String,
    },
    List,
    Chain {
        #[arg(required = true, num_args = 2.., add = ArgValueCandidates::new(task_ids))]
        ids: Vec<String>,
    },
}

#[cfg(test)]
#[path = "../tests/unit/cli.rs"]
mod tests;
