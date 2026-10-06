use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use rusqlite::Connection;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{ErrorData, Implementation, ServerCapabilities, ServerInfo};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::transport::stdio;
use rmcp::{ServerHandler, ServiceExt, tool_handler};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub(crate) const INSTRUCTIONS: &str = "\
sara is a folder-aware task manager with a long-term memory: a git repo == a \
project, and each task carries a rich guide (ordered steps, acceptance criteria, \
notes, links, dependencies) meant for an agent to execute. This server exposes the \
whole non-interactive task lifecycle and the memory store as typed tools — begin, \
plan, guide, track, verify, remember, and complete; nothing opens a TUI or blocks \
on stdin.\n\n\
Because the server is long-running and has no per-call working directory, EVERY \
tool takes an optional `project_path` — set it to the absolute path of the target \
git repo so the tool resolves/creates tasks there; omit it to use the directory the \
server was launched in. Target tasks by their 8-char UUID prefix (stable), not the \
recycled numeric display id. Never read the sara SQLite DB directly.\n\n\
Execution loop: start new work with `begin` (or `list`/`info` to resume a task). \
`begin` seeds a first step to recall prior art — decide what knowledge bears on the \
task and call `recall` yourself, passing the task as `task`. Lay out the work with `check` (steps, or \
acceptance criteria with kind=\"acceptance\" and a `verify` command), then repeat: \
`next` for the current step → do the work → `step_done` with a result (and `used`: the memories that helped). Record \
findings and decisions with `annotate`. `validate` runs every acceptance \
criterion's verify command and stamps the guide green at git HEAD.\n\n\
Memory: `recall` before solving, with `task` set when it serves a task (heed its `patterns`, `confidence`, and `stale` \
flags); `learn` one distilled insight when you finish, tagged and bound to its \
files; `relearn`/`forget` to correct or retire memories, `reflect` and `doctor` to \
keep the store healthy.\n\n\
A failed tool call returns a result with isError set and the reason as text — read \
it and correct the call. To finish, link the PR (`link`) and call `done` only once \
that PR has merged — opening a PR is not completion.";

pub(crate) struct CwdGuard {
    prev: Option<PathBuf>,
}

impl CwdGuard {
    pub(crate) fn enter(project_path: Option<&str>) -> anyhow::Result<Self> {
        match project_path {
            Some(p) if !p.trim().is_empty() => {
                let p = p.trim();
                let path = Path::new(p);
                if !path.is_absolute() {
                    anyhow::bail!(
                        "project_path must be an absolute path to the target repo, got {p:?}"
                    );
                }
                let prev = std::env::current_dir().ok();
                std::env::set_current_dir(path)
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

    pub(crate) fn all_router() -> ToolRouter<Self> {
        Self::read_router() + Self::guide_router() + Self::lifecycle_router()
    }

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

    /// Route a tool call exactly as `#[tool_handler]`'s generated default does.
    #[cfg(not(feature = "telemetry"))]
    async fn route_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    /// Route a tool call and record it: tool name, argument NAMES (values
    /// stripped), calling client, duration, and outcome. Wrapping the router
    /// also captures failures before a handler body runs — parameter
    /// deserialization errors (`is_error` results) and unknown tools (`Err`).
    #[cfg(feature = "telemetry")]
    async fn route_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        use crate::infrastructure::telemetry;

        let tool_name = request.name.to_string();
        let params = telemetry::extract_mcp_params(request.arguments.as_ref());
        let client = context.peer.peer_info().map(|info| telemetry::McpClient {
            name: info.client_info.name.clone(),
            version: info.client_info.version.clone(),
        });

        let started = std::time::Instant::now();
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        let outcome = self.tool_router.call(tcc).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        let telem: anyhow::Result<()> = match &outcome {
            Ok(r) if r.is_error == Some(true) => {
                Err(anyhow::anyhow!("mcp tool returned an error result"))
            }
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow::anyhow!(e.to_string())),
        };
        telemetry::capture(
            &self.cfg,
            telemetry::Source::Mcp,
            &format!("mcp {tool_name}"),
            &params,
            elapsed_ms,
            &telem,
            client.as_ref(),
        );
        telemetry::spawn_flush(&self.cfg);
        outcome
    }
}

/// Tool failures are returned as `Err(String)`, which rmcp turns into a
/// `CallToolResult` with `isError: true` so the model can see and recover from
/// them; `ErrorData` would surface as a JSON-RPC protocol error instead.
pub(crate) fn mcp_err(e: anyhow::Error) -> String {
    e.to_string()
}

pub(crate) fn ok_json(v: serde_json::Value) -> Result<String, String> {
    serde_json::to_string_pretty(&v).map_err(|e| mcp_err(e.into()))
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SaraServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(INSTRUCTIONS.to_string());
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = Implementation::new("sara", env!("CARGO_PKG_VERSION"));
        info
    }

    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        self.route_tool(request, context).await
    }
}

pub fn run(conn: Connection, cfg: &Config) -> anyhow::Result<()> {
    let _ = db::prune_old_events(&conn, 90);
    let server = SaraServer::new(conn, cfg.clone());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let (stdin, stdout) = stdio();
        let transport =
            super::transport::TolerantInit::new(AsyncRwTransport::new_server(stdin, stdout));
        let service = server.serve(transport).await?;
        service.waiting().await?;
        Ok::<(), anyhow::Error>(())
    })
}
