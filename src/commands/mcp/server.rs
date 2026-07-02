//! The MCP server runtime: the [`SaraServer`] type, its per-call project/cwd
//! guard, the JSON helpers the tool slices share, the `ServerHandler` (get_info +
//! dispatch), and the `sara mcp` entry point. The `#[tool]` methods themselves
//! live in the `read` / `guide` / `lifecycle` slices; `new` composes their named
//! routers into one.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use rusqlite::Connection;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{ErrorData, Implementation, ServerCapabilities, ServerInfo};
use rmcp::transport::stdio;
use rmcp::{ServerHandler, ServiceExt, tool_handler};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub(crate) const INSTRUCTIONS: &str = "\
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
pub(crate) struct CwdGuard {
    prev: Option<PathBuf>,
}

impl CwdGuard {
    pub(crate) fn enter(project_path: Option<&str>) -> anyhow::Result<Self> {
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
    pub(crate) fn new(conn: Connection, cfg: Config) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            cfg,
            tool_router: Self::all_router(),
        }
    }

    /// The full tool set: the three capability routers combined. Shared by `new`
    /// (to populate the dispatch field) and the tool-count test.
    pub(crate) fn all_router() -> ToolRouter<Self> {
        Self::read_router() + Self::guide_router() + Self::lifecycle_router()
    }

    /// Run `f` against the DB in the context of `project_path`: locks the single
    /// connection (serializing all tool calls), sets the process cwd to the
    /// project, and opens an undo batch — all on one thread, so both the cwd and
    /// the thread-local undo context are coherent for the enclosed call.
    pub(crate) fn with_project<T>(
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
pub(crate) fn mcp_err(e: anyhow::Error) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// Render a value as a pretty JSON string — the tool result is returned as a text
/// content block (mirroring the CLI's `--json` output). Tools return dynamic JSON
/// objects, so text avoids MCP's requirement that structured `outputSchema` be a
/// statically-typed object.
pub(crate) fn ok_json(v: serde_json::Value) -> Result<String, ErrorData> {
    serde_json::to_string_pretty(&v).map_err(|e| mcp_err(e.into()))
}

#[tool_handler(router = self.tool_router)]
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
