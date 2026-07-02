//! `sara mcp` — a stdio JSON-RPC MCP server exposing sara's agent loop as typed
//! tools, so any MCP client (Claude, Codex, Copilot, …) can drive sara without
//! the CLI's flag-ordering / UUID / TUI footguns.
//!
//! Design (see task 13024f4f):
//! - A THIN adapter: every tool calls the same `commands::*` value functions the
//!   CLI uses, so there is one source of truth. Nothing here re-implements DB logic.
//! - Folder-awareness: sara derives "the project" from the current git folder, but
//!   a long-running server has no per-call cwd. Every tool therefore takes an
//!   optional `project_path`; [`SaraServer::with_project`] sets the process cwd to
//!   it (guarded by the connection mutex, so concurrent calls can't race on cwd or
//!   the thread-local undo batch) before calling the underlying function.
//! - stdout is the JSON-RPC channel: tools MUST never print. That is why the
//!   adapter calls the print-free `*_value` cores, not the CLI's `run` functions.
//! - The async runtime is built ONLY here; the rest of the CLI stays synchronous.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::Deserialize;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ErrorData, Implementation, ServerCapabilities, ServerInfo};
use rmcp::transport::stdio;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};

use crate::commands;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

const INSTRUCTIONS: &str = "\
sara is a folder-aware task manager: a git repo == a project, and each task carries \
a rich guide (ordered steps, acceptance criteria, notes, links, dependencies) meant \
for an agent to execute. This server exposes the whole non-interactive task \
lifecycle as typed tools — read, plan, guide, edit, track, and complete; nothing \
opens a TUI or blocks on stdin.\n\n\
Because the server is long-running and has no per-call working directory, EVERY \
tool takes an optional `project_path` — set it to the absolute path of the target \
git repo so the tool resolves/creates tasks there; omit it to use the directory the \
server was launched in. Target tasks by their 8-char UUID prefix (stable), not the \
recycled numeric display id. Never read the sara SQLite DB directly.\n\n\
Typical execution loop: list/info to load a task → next for the current step → do \
the work → step_done (with a result) → verify. To finish, link the PR (link) and \
call done only once that PR has merged — opening a PR is not completion.";

/// Restores the process working directory on drop. Only changes cwd when a
/// non-empty `project_path` is supplied.
struct CwdGuard {
    prev: Option<PathBuf>,
}

impl CwdGuard {
    fn enter(project_path: Option<&str>) -> anyhow::Result<Self> {
        match project_path {
            Some(p) if !p.trim().is_empty() => {
                let prev = std::env::current_dir().ok();
                std::env::set_current_dir(p)
                    .with_context(|| format!("project_path is not an accessible directory: {p}"))?;
                Ok(Self { prev })
            }
            _ => Ok(Self { prev: None }),
        }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        if let Some(prev) = &self.prev {
            let _ = std::env::set_current_dir(prev);
        }
    }
}

#[derive(Clone)]
pub struct SaraServer {
    conn: Arc<Mutex<Connection>>,
    cfg: Config,
    tool_router: ToolRouter<Self>,
}

impl SaraServer {
    fn new(conn: Connection, cfg: Config) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            cfg,
            tool_router: Self::tool_router(),
        }
    }

    /// Run `f` against the DB in the context of `project_path`: locks the single
    /// connection (serializing all tool calls), sets the process cwd to the
    /// project, and opens an undo batch — all on one thread, so both the cwd and
    /// the thread-local undo context are coherent for the enclosed call.
    fn with_project<T>(
        &self,
        project_path: Option<&str>,
        label: &str,
        f: impl FnOnce(&Connection, &Config) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("sara database mutex was poisoned"))?;
        let _cwd = CwdGuard::enter(project_path)?;
        db::begin_undo_batch(label);
        f(&conn, &self.cfg)
    }
}

/// anyhow → MCP error. Tool-level failures surface as client-visible errors.
fn mcp_err(e: anyhow::Error) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// Render a value as a pretty JSON string — the tool result is returned as a text
/// content block (mirroring the CLI's `--json` output). Tools return dynamic JSON
/// objects, so text avoids MCP's requirement that structured `outputSchema` be a
/// statically-typed object.
fn ok_json(v: serde_json::Value) -> Result<String, ErrorData> {
    serde_json::to_string_pretty(&v).map_err(|e| mcp_err(e.into()))
}

// ── Tool parameter schemas ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct ListParams {
    /// Absolute path of the target git repo/project. Omit to use the launch dir.
    project_path: Option<String>,
    /// List tasks across all projects instead of just the current one.
    all: Option<bool>,
    /// Explicit project name filter (overrides project_path detection).
    project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct IdParams {
    project_path: Option<String>,
    /// Task id or (preferred) 8-char uuid prefix.
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AddParams {
    project_path: Option<String>,
    /// The task description / title.
    description: String,
    project: Option<String>,
    /// Priority: H, M, or L.
    priority: Option<String>,
    tags: Option<Vec<String>>,
    /// Recurrence interval: daily, weekly, 2w, 3d, 1m, …
    recur: Option<String>,
    /// Notes to attach at creation.
    annotations: Option<Vec<String>>,
    /// URLs to link at creation.
    links: Option<Vec<String>>,
    /// Checklist steps to add at creation.
    checks: Option<Vec<String>>,
    /// UUID prefixes of tasks this new task depends on.
    depends_on: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StepsParams {
    project_path: Option<String>,
    id: String,
    /// Only return steps 1..=until.
    until: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StepDoneParams {
    project_path: Option<String>,
    id: String,
    /// 1-based step number.
    n: usize,
    /// Execution result / evidence recorded with the step.
    result: Option<String>,
    /// Item kind: "step" (default) or "acceptance".
    kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct VerifyParams {
    project_path: Option<String>,
    id: String,
    /// Only return the verify command for step N.
    step: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecallParams {
    project_path: Option<String>,
    /// Search query (keyword / FTS).
    query: String,
    /// Max results (default 20).
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AnnotateParams {
    project_path: Option<String>,
    id: String,
    /// The note / comment text.
    text: String,
    /// Note kind: comment (default), finding, decision, constraint, risk, …
    kind: Option<String>,
    /// Author: human (default) or ai.
    author: Option<String>,
    /// Anchor to a guide element: step:N, acceptance:N, anchor:ID, note:ID.
    on: Option<String>,
    /// Flag the anchored element for reconsideration.
    reconsider: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PlanImportParams {
    project_path: Option<String>,
    /// The plan graph as a JSON string ({"project"?, "tasks":[…]}). Passed inline
    /// (never via stdin, which is the MCP transport channel).
    plan_json: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DoneParams {
    project_path: Option<String>,
    id: String,
    /// Complete the task even if it is blocked by unfinished dependencies.
    force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LinkParams {
    project_path: Option<String>,
    id: String,
    /// The URL to attach (e.g. a PR or issue link).
    url: String,
    /// Optional human-readable label for the link.
    label: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DepParams {
    project_path: Option<String>,
    /// The dependent task (the one that gets blocked).
    id: String,
    /// "on" (add: `id` depends on `other`), "off" (remove), or "list".
    action: String,
    /// The blocker task; required for "on"/"off", ignored for "list".
    other: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CheckParams {
    project_path: Option<String>,
    id: String,
    /// The step / acceptance-criterion text.
    text: String,
    /// Item kind: "step" (default) or "acceptance".
    kind: Option<String>,
    /// Optional intent / why-note for the step.
    intent: Option<String>,
    /// Optional shell command that verifies this step.
    verify: Option<String>,
    /// Author/source of the item: "human" (default) or "ai".
    source: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ModifyParams {
    project_path: Option<String>,
    id: String,
    /// New description / title.
    description: Option<String>,
    /// Priority: H, M, or L.
    priority: Option<String>,
    /// Due date (same formats as `add`, e.g. "friday", "2026-07-15").
    due: Option<String>,
    /// Clear the due date.
    clear_due: Option<bool>,
    /// Tags — REPLACES the whole tag set (not additive).
    tags: Option<Vec<String>>,
    /// Clear all tags.
    clear_tags: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ResolveParams {
    project_path: Option<String>,
    /// The feedback (annotation) id to resolve — NOT a task id/uuid.
    feedback_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StepEditParams {
    project_path: Option<String>,
    id: String,
    /// 1-based step number.
    n: usize,
    /// Item kind: "step" (default) or "acceptance".
    kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GuideTextParams {
    project_path: Option<String>,
    id: String,
    /// The text to set (assignment / rationale).
    text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AttachParams {
    project_path: Option<String>,
    id: String,
    /// File path or URL to attach. URLs are stored as links.
    path: String,
    /// Why this file/anchor matters.
    reason: Option<String>,
    /// Symbol (function/type) the anchor points at.
    symbol: Option<String>,
    /// Line range, e.g. "10:57" or "10-57".
    lines: Option<String>,
    /// Provenance: "ai" marks it suggested; anything else is manual.
    source: Option<String>,
}

// ── Tools ────────────────────────────────────────────────────────────────────

#[tool_router]
impl SaraServer {
    #[tool(description = "List pending tasks for a project (or all projects).")]
    fn list(&self, Parameters(p): Parameters<ListParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp list", |conn, cfg| {
                commands::list::list_value(conn, cfg, p.all.unwrap_or(false), p.project.as_deref())
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Full task guide as JSON: description, steps, acceptance, notes, links, freshness, open feedback."
    )]
    fn info(&self, Parameters(p): Parameters<IdParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp info", |conn, _cfg| {
                commands::info::guide_value(conn, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Create a task (never opens the TUI). Returns the new task's id/uuid.")]
    fn add(&self, Parameters(p): Parameters<AddParams>) -> Result<String, ErrorData> {
        let words = vec![p.description.clone()];
        let tags = p.tags.clone().unwrap_or_default();
        let annotations = p.annotations.clone().unwrap_or_default();
        let links = p.links.clone().unwrap_or_default();
        let checks = p.checks.clone().unwrap_or_default();
        let depends_on = p.depends_on.clone().unwrap_or_default();
        let v = self
            .with_project(p.project_path.as_deref(), "mcp add", |conn, cfg| {
                commands::add::run_value(
                    conn,
                    cfg,
                    &words,
                    p.project.as_deref(),
                    p.priority.as_deref(),
                    &tags,
                    p.recur.as_deref(),
                    &annotations,
                    &links,
                    &checks,
                    &depends_on,
                )
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "The execution cursor: the first not-done step of a task.")]
    fn next(&self, Parameters(p): Parameters<IdParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp next", |conn, _cfg| {
                commands::guide::next_value(conn, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Ordered steps of a task (optionally only up to step `until`).")]
    fn steps(&self, Parameters(p): Parameters<StepsParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp steps", |conn, _cfg| {
                commands::guide::steps_value(conn, &p.id, p.until)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Mark step N of a task done, recording a result and the current git commit."
    )]
    fn step_done(&self, Parameters(p): Parameters<StepDoneParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp step_done", |conn, _cfg| {
                commands::guide::step_done_value(
                    conn,
                    &p.id,
                    p.n,
                    p.result.as_deref(),
                    p.kind.as_deref(),
                )
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Read-only: the verification commands + acceptance criteria for a task (does NOT run them)."
    )]
    fn verify(&self, Parameters(p): Parameters<VerifyParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp verify", |conn, _cfg| {
                commands::guide::verify_value(conn, &p.id, p.step)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Cross-task keyword search over descriptions, notes, and code anchors.")]
    fn recall(&self, Parameters(p): Parameters<RecallParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp recall", |conn, cfg| {
                commands::recall::recall_value(conn, cfg, &p.query, p.limit.unwrap_or(20))
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Add a comment / note to a task (optionally anchored, or an ai finding/decision)."
    )]
    fn annotate(&self, Parameters(p): Parameters<AnnotateParams>) -> Result<String, ErrorData> {
        let words = vec![p.text.clone()];
        let v = self
            .with_project(p.project_path.as_deref(), "mcp annotate", |conn, _cfg| {
                commands::annotate::annotate_value(
                    conn,
                    &p.id,
                    &words,
                    p.kind.as_deref(),
                    p.author.as_deref(),
                    p.on.as_deref(),
                    p.reconsider.unwrap_or(false),
                )
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Bulk-ingest a task graph from an inline JSON plan; wires dependencies by plan-local key."
    )]
    fn plan_import(
        &self,
        Parameters(p): Parameters<PlanImportParams>,
    ) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp plan_import", |conn, cfg| {
                commands::plan::import_raw(conn, cfg, &p.plan_json)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Mark a task complete (finalizes its timer, repacks ids, spawns the next recurrence). Errors if the task is blocked unless force=true. A task is done only when its PR is merged — do not call this just because a PR was opened."
    )]
    fn done(&self, Parameters(p): Parameters<DoneParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp done", |conn, cfg| {
                commands::done::done_value(conn, cfg, &p.id, p.force.unwrap_or(false))
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Attach a URL (e.g. a PR or issue link) to a task.")]
    fn link(&self, Parameters(p): Parameters<LinkParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp link", |conn, _cfg| {
                commands::annotate::link_value(conn, &p.id, &p.url, p.label.as_deref())
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Manage task dependencies. action=\"on\": `id` becomes blocked by `other`; \"off\": remove that edge; \"list\": show the task's blockers and what it blocks. `other` is required for on/off."
    )]
    fn dep(&self, Parameters(p): Parameters<DepParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp dep", |conn, cfg| {
                match p.action.as_str() {
                    "on" => {
                        let other = p
                            .other
                            .as_deref()
                            .ok_or_else(|| anyhow::anyhow!("dep action \"on\" requires `other`"))?;
                        commands::dep::dep_on_value(conn, cfg, &p.id, other)
                    }
                    "off" => {
                        let other = p.other.as_deref().ok_or_else(|| {
                            anyhow::anyhow!("dep action \"off\" requires `other`")
                        })?;
                        commands::dep::dep_off_value(conn, cfg, &p.id, other)
                    }
                    "list" => commands::dep::dep_list_value(conn, &p.id),
                    other => {
                        anyhow::bail!(
                            "unknown dep action `{other}` (expected \"on\", \"off\", or \"list\")"
                        )
                    }
                }
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Add a checklist step (or an acceptance criterion with kind=\"acceptance\") to a task's guide, optionally with an intent note and a verify command."
    )]
    fn check(&self, Parameters(p): Parameters<CheckParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp check", |conn, _cfg| {
                commands::guide::check_value(
                    conn,
                    &p.id,
                    &p.text,
                    p.intent.as_deref(),
                    p.kind.as_deref(),
                    p.source.as_deref(),
                    p.verify.as_deref(),
                )
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Stamp a task's guide as validated against the project's current git HEAD. Errors if the project is not a git repo."
    )]
    fn validate(&self, Parameters(p): Parameters<IdParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp validate", |conn, _cfg| {
                commands::guide::validate_value(conn, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Set task fields non-interactively (never opens the TUI). At least one field is required. `tags` REPLACES the whole tag set."
    )]
    fn modify(&self, Parameters(p): Parameters<ModifyParams>) -> Result<String, ErrorData> {
        let tags = p.tags.clone().unwrap_or_default();
        let v = self
            .with_project(p.project_path.as_deref(), "mcp modify", |conn, cfg| {
                commands::modify::modify_value(
                    conn,
                    cfg,
                    &p.id,
                    p.description.as_deref(),
                    p.priority.as_deref(),
                    p.due.as_deref(),
                    p.clear_due.unwrap_or(false),
                    &tags,
                    p.clear_tags.unwrap_or(false),
                )
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "List a task's open human feedback (items awaiting a response).")]
    fn feedback(&self, Parameters(p): Parameters<IdParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp feedback", |conn, _cfg| {
                commands::guide::feedback_value(conn, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Resolve a feedback item by its `feedback_id` (the annotation id from the `feedback`/`info` output — NOT a task id)."
    )]
    fn resolve(&self, Parameters(p): Parameters<ResolveParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp resolve", |conn, _cfg| {
                commands::guide::resolve_value(conn, p.feedback_id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Dependency-ordered briefing for a task: each task's full guide in dependency order (the task plus everything it is blocked by)."
    )]
    fn plan_show(&self, Parameters(p): Parameters<IdParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp plan_show", |conn, _cfg| {
                commands::plan::show_value(conn, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Reopen a previously-completed step (or acceptance criterion) of a task.")]
    fn step_undone(&self, Parameters(p): Parameters<StepEditParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp step_undone",
                |conn, _cfg| {
                    commands::guide::step_undone_value(conn, &p.id, p.n, p.kind.as_deref())
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Delete step N (or acceptance criterion N) from a task's guide; remaining items renumber."
    )]
    fn step_remove(&self, Parameters(p): Parameters<StepEditParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp step_remove",
                |conn, _cfg| {
                    commands::guide::step_remove_value(conn, &p.id, p.n, p.kind.as_deref())
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Set a task's assignment (the originating prompt / what to build).")]
    fn assignment(&self, Parameters(p): Parameters<GuideTextParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp assignment", |conn, _cfg| {
                commands::guide::assignment_value(conn, &p.id, &p.text)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Set a task's rationale (why it exists / the reasoning behind it).")]
    fn rationale(&self, Parameters(p): Parameters<GuideTextParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp rationale", |conn, _cfg| {
                commands::guide::rationale_value(conn, &p.id, &p.text)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Attach a file or code anchor to a task (a URL is stored as a link). Anchor metadata: `reason`, `symbol`, `lines` (\"10:57\"), `source`."
    )]
    fn attach(&self, Parameters(p): Parameters<AttachParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp attach", |conn, _cfg| {
                commands::annotate::attach_value(
                    conn,
                    &p.id,
                    &p.path,
                    p.reason.as_deref(),
                    p.symbol.as_deref(),
                    p.lines.as_deref(),
                    p.source.as_deref(),
                )
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Start the time tracker for a task (no-op if it is already active).")]
    fn start(&self, Parameters(p): Parameters<IdParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp start", |conn, cfg| {
                commands::timer::start_value(conn, cfg, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Stop the time tracker for a task, recording the session; snapshots a tied branch's changed files if one is set."
    )]
    fn stop(&self, Parameters(p): Parameters<IdParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp stop", |conn, cfg| {
                commands::timer::stop_value(conn, cfg, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }
}

#[tool_handler]
impl ServerHandler for SaraServer {
    fn get_info(&self) -> ServerInfo {
        // ServerInfo / Implementation are #[non_exhaustive]: build from Default and
        // assign public fields rather than using a struct literal.
        let mut info = ServerInfo::default();
        info.instructions = Some(INSTRUCTIONS.to_string());
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = Implementation::new("sara", env!("CARGO_PKG_VERSION"));
        info
    }
}

/// `sara mcp` entry point: serve the MCP tool set over stdio until the client
/// disconnects. Builds the tokio runtime here so the rest of the CLI stays sync.
pub fn run(conn: Connection, cfg: &Config) -> anyhow::Result<()> {
    let server = SaraServer::new(conn, cfg.clone());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let service = server.serve(stdio()).await?;
        service.waiting().await?;
        Ok::<(), anyhow::Error>(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::model::Task;
    use std::sync::Mutex;

    // cwd is process-global; serialize the tests that mutate it.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    fn server_with(conn: Connection) -> SaraServer {
        SaraServer::new(conn, Config::default())
    }

    fn seed_task(conn: &Connection, project: &str, desc: &str) {
        let mut t = Task::new(desc.to_string(), project.to_string());
        db::insert_task(conn, &mut t).expect("insert task");
    }

    #[test]
    fn exposes_the_agent_loop_tools() {
        let names: Vec<String> = SaraServer::tool_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert_eq!(names.len(), 26, "expected 26 tools, got {names:?}");
        for expected in [
            // read
            "list",
            "info",
            "next",
            "steps",
            "verify",
            "recall",
            "feedback",
            "plan_show",
            // mutate (create / guide)
            "add",
            "step_done",
            "annotate",
            "plan_import",
            "check",
            "step_undone",
            "step_remove",
            "assignment",
            "rationale",
            "attach",
            // completion / edit / lifecycle
            "done",
            "link",
            "dep",
            "validate",
            "modify",
            "resolve",
            "start",
            "stop",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing tool `{expected}` in {names:?}"
            );
        }
    }

    #[test]
    fn get_info_advertises_sara_tools_and_instructions() {
        let server = server_with(db::open_in_memory_for_test());
        let info = server.get_info();
        assert_eq!(info.server_info.name, "sara");
        assert!(
            info.capabilities.tools.is_some(),
            "tools capability missing"
        );
        assert!(
            info.instructions
                .as_deref()
                .unwrap_or_default()
                .contains("project_path"),
            "instructions should mention project_path"
        );
    }

    #[test]
    fn cwd_guard_sets_and_restores_working_dir() {
        let _lock = CWD_LOCK.lock().unwrap();
        let start = std::env::current_dir().unwrap();
        let tmp = std::env::temp_dir().canonicalize().unwrap();
        {
            let _g = CwdGuard::enter(Some(tmp.to_str().unwrap())).unwrap();
            assert_eq!(
                std::env::current_dir().unwrap().canonicalize().unwrap(),
                tmp
            );
        }
        assert_eq!(std::env::current_dir().unwrap(), start, "cwd not restored");

        // None / empty leaves cwd untouched.
        {
            let _g = CwdGuard::enter(None).unwrap();
            assert_eq!(std::env::current_dir().unwrap(), start);
        }
    }

    #[test]
    fn cwd_guard_errors_on_missing_project_path() {
        let _lock = CWD_LOCK.lock().unwrap();
        let start = std::env::current_dir().unwrap();
        assert!(CwdGuard::enter(Some("/no/such/sara/dir/xyz")).is_err());
        assert_eq!(
            std::env::current_dir().unwrap(),
            start,
            "cwd changed on error"
        );
    }

    #[test]
    fn with_project_runs_closure_against_the_connection() {
        let server = server_with(db::open_in_memory_for_test());
        seed_task_via(&server, "alpha", "first");
        seed_task_via(&server, "alpha", "second");

        // project_path=None avoids touching process cwd; filter by explicit project.
        let v = server
            .with_project(None, "test", |conn, cfg| {
                commands::list::list_value(conn, cfg, false, Some("alpha"))
            })
            .expect("with_project");
        let tasks = v["tasks"].as_array().expect("tasks array");
        assert_eq!(tasks.len(), 2);
    }

    fn seed_task_via(server: &SaraServer, project: &str, desc: &str) {
        server
            .with_project(None, "seed", |conn, _cfg| {
                seed_task(conn, project, desc);
                Ok(())
            })
            .expect("seed");
    }

    /// Seed a task and return its uuid string (for targeting mutate tools).
    fn seed_returning(server: &SaraServer, project: &str, desc: &str) -> String {
        server
            .with_project(None, "seed", |conn, _cfg| {
                let mut t = Task::new(desc.to_string(), project.to_string());
                db::insert_task(conn, &mut t)?;
                Ok(t.uuid.to_string())
            })
            .expect("seed")
    }

    #[test]
    fn done_value_marks_task_completed() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "finish me");
        let v = server
            .with_project(None, "done", |conn, cfg| {
                commands::done::done_value(conn, cfg, &uuid, false)
            })
            .expect("done");
        assert_eq!(v["status"], "completed");
        assert_eq!(v["recurrence"], serde_json::Value::Null);
    }

    #[test]
    fn link_value_attaches_a_url() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "task");
        let v = server
            .with_project(None, "link", |conn, _cfg| {
                commands::annotate::link_value(conn, &uuid, "https://example/pr/1", Some("PR"))
            })
            .expect("link");
        assert_eq!(v["url"], "https://example/pr/1");
    }

    #[test]
    fn dep_on_then_list_reports_the_blocker() {
        let server = server_with(db::open_in_memory_for_test());
        let a = seed_returning(&server, "p", "dependent");
        let b = seed_returning(&server, "p", "blocker");
        server
            .with_project(None, "dep on", |conn, cfg| {
                commands::dep::dep_on_value(conn, cfg, &a, &b)
            })
            .expect("dep on");
        let v = server
            .with_project(None, "dep list", |conn, _cfg| {
                commands::dep::dep_list_value(conn, &a)
            })
            .expect("dep list");
        assert_eq!(v["blocked_by"].as_array().map(|a| a.len()), Some(1));
    }

    #[test]
    fn check_value_adds_a_step() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "task");
        let v = server
            .with_project(None, "check", |conn, _cfg| {
                commands::guide::check_value(conn, &uuid, "do the thing", None, None, None, None)
            })
            .expect("check");
        assert_eq!(v["kind"], db::STEP_KIND_STEP);
        let steps = server
            .with_project(None, "steps", |conn, _cfg| {
                commands::guide::steps_value(conn, &uuid, None)
            })
            .expect("steps");
        assert_eq!(steps["steps"].as_array().map(|a| a.len()), Some(1));
    }

    #[test]
    fn modify_value_sets_priority_and_requires_a_field() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "task");
        let v = server
            .with_project(None, "modify", |conn, cfg| {
                commands::modify::modify_value(
                    conn,
                    cfg,
                    &uuid,
                    None,
                    Some("H"),
                    None,
                    false,
                    &[],
                    false,
                )
            })
            .expect("modify");
        assert_eq!(v["priority"], "H");

        // No field flags → must error, never open the TUI.
        let empty = server.with_project(None, "modify-empty", |conn, cfg| {
            commands::modify::modify_value(conn, cfg, &uuid, None, None, None, false, &[], false)
        });
        assert!(empty.is_err(), "modify with no fields should error");
    }

    #[test]
    fn step_undone_then_remove_edit_the_checklist() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "task");
        server
            .with_project(None, "check", |conn, _cfg| {
                commands::guide::check_value(conn, &uuid, "step one", None, None, None, None)
            })
            .expect("check");
        // done → undone flips it back to not-done.
        server
            .with_project(None, "done", |conn, _cfg| {
                commands::guide::step_done_value(conn, &uuid, 1, None, None)
            })
            .expect("step_done");
        let undone = server
            .with_project(None, "undone", |conn, _cfg| {
                commands::guide::step_undone_value(conn, &uuid, 1, None)
            })
            .expect("step_undone");
        assert_eq!(undone["done"], false);
        // remove drops the item.
        let removed = server
            .with_project(None, "remove", |conn, _cfg| {
                commands::guide::step_remove_value(conn, &uuid, 1, None)
            })
            .expect("step_remove");
        assert_eq!(removed["removed"], "step one");
        let steps = server
            .with_project(None, "steps", |conn, _cfg| {
                commands::guide::steps_value(conn, &uuid, None)
            })
            .expect("steps");
        assert_eq!(steps["steps"].as_array().map(|a| a.len()), Some(0));
    }

    #[test]
    fn assignment_and_rationale_set_guide_text() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "task");
        let a = server
            .with_project(None, "assignment", |conn, _cfg| {
                commands::guide::assignment_value(conn, &uuid, "build the thing")
            })
            .expect("assignment");
        assert_eq!(a["assignment"], "build the thing");
        let r = server
            .with_project(None, "rationale", |conn, _cfg| {
                commands::guide::rationale_value(conn, &uuid, "because reasons")
            })
            .expect("rationale");
        assert_eq!(r["rationale"], "because reasons");
    }

    #[test]
    fn attach_value_records_a_file_and_an_anchor() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "task");
        let file = server
            .with_project(None, "attach", |conn, _cfg| {
                commands::annotate::attach_value(conn, &uuid, "src/main.rs", None, None, None, None)
            })
            .expect("attach file");
        assert_eq!(file["kind"], "file");
        let anchor = server
            .with_project(None, "attach anchor", |conn, _cfg| {
                commands::annotate::attach_value(
                    conn,
                    &uuid,
                    "src/lib.rs",
                    Some("core logic"),
                    None,
                    Some("10:20"),
                    None,
                )
            })
            .expect("attach anchor");
        assert_eq!(anchor["kind"], "anchor");
        assert_eq!(anchor["line_start"], 10);
        assert_eq!(anchor["line_end"], 20);
    }

    #[test]
    fn start_then_stop_tracks_a_session() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "task");
        let started = server
            .with_project(None, "start", |conn, cfg| {
                commands::timer::start_value(conn, cfg, &uuid)
            })
            .expect("start");
        assert_eq!(started["started"], true);
        let stopped = server
            .with_project(None, "stop", |conn, cfg| {
                commands::timer::stop_value(conn, cfg, &uuid)
            })
            .expect("stop");
        assert_eq!(stopped["stopped"], true);
        assert!(stopped["session_seconds"].as_i64().is_some());
    }

    #[test]
    fn feedback_and_resolve_round_trip() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "task");
        // Seed an open feedback item via a human annotation flagged for revision.
        server
            .with_project(None, "annotate", |conn, _cfg| {
                commands::annotate::annotate_value(
                    conn,
                    &uuid,
                    &["please fix".to_string()],
                    None,
                    Some("human"),
                    None,
                    true,
                )
            })
            .expect("annotate");
        let fb = server
            .with_project(None, "feedback", |conn, _cfg| {
                commands::guide::feedback_value(conn, &uuid)
            })
            .expect("feedback");
        let items = fb["open_feedback"].as_array().expect("open_feedback array");
        assert_eq!(items.len(), 1);
        let fb_id = items[0]["id"].as_i64().expect("feedback id");
        let resolved = server
            .with_project(None, "resolve", |conn, _cfg| {
                commands::guide::resolve_value(conn, fb_id)
            })
            .expect("resolve");
        assert_eq!(resolved["resolved"], true);
    }

    #[test]
    fn plan_show_value_returns_a_briefing() {
        let server = server_with(db::open_in_memory_for_test());
        let uuid = seed_returning(&server, "p", "solo task");
        let v = server
            .with_project(None, "plan_show", |conn, _cfg| {
                commands::plan::show_value(conn, &uuid)
            })
            .expect("plan_show");
        assert_eq!(v["briefing"].as_array().map(|a| a.len()), Some(1));
    }
}
