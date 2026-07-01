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
sara is a folder-aware task manager. A git repo == a project. Because this server \
is long-running and has no per-call working directory, EVERY tool takes an optional \
`project_path` — set it to the absolute path of the target git repo so the tool \
resolves/creates tasks in that project. Omit it to use the directory the server was \
launched in. Target tasks by their 8-char UUID prefix (stable) rather than the \
recycled numeric display id. Typical loop: add → list/info → next → step_done → verify.";

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
    fn exposes_the_ten_agent_loop_tools() {
        let names: Vec<String> = SaraServer::tool_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert_eq!(names.len(), 10, "expected 10 tools, got {names:?}");
        for expected in [
            "list",
            "info",
            "add",
            "next",
            "steps",
            "step_done",
            "verify",
            "recall",
            "annotate",
            "plan_import",
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
}
