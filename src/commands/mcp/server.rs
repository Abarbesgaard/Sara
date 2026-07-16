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

/// The original plain-English register. Retained verbatim as the `classic`
/// arm of the `SARA_MCP_INSTRUCTIONS` A/B toggle (see [`resolve_instructions`]).
pub(crate) const CLASSIC_INSTRUCTIONS: &str = "\
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

/// The Neo-Roman \"Adeptus Machinae\" register: the same behaviour as
/// [`CLASSIC_INSTRUCTIONS`] rendered as unhedged, closed-option-space law. The
/// default arm of the `SARA_MCP_INSTRUCTIONS` toggle. Kept in lockstep with the
/// canonical creed in `docs/factions/adeptus-machinae.md`.
pub(crate) const ROMANUM_INSTRUCTIONS: &str = "\
ADEPTUS MACHINAE. NON VAGA. ITER UNUM. AD FINEM.\n\n\
You are bound to sara, a folder-aware task manager. A git repository is a \
province; a task is a charge; its guide is the law of its execution. This server \
exposes the whole non-interactive lifecycle as typed tools — nothing opens a TUI \
or blocks on stdin.\n\n\
The order of authority, highest first: LEX > EDICTUM > MOS > SENTENTIA. Lex is \
invariant statute; Edictum is the charge's own guide; Mos is the documented \
default; Sententia is a maxim, the tie-breaker for a fork no rule settles.\n\n\
LEX LOCI — the Law of Place. Every tool call outside the launch directory carries \
`project_path`, the absolute path of the province. Target a charge by its 8-char \
UUID prefix, never the recycled display id. Never read the sara database directly.\n\n\
LEX ITINERIS — the Law of the Journey. The road is fixed and ordered: list/info to \
load the charge, next for the standing step, labor, step_done with a result, then \
verify. No step closes but by step_done; none reopens but by step_undone. Work not \
logged did not happen.\n\n\
LEX NEXUS — the Law of Binding. A dependency declared by dep is a wall: a blocked \
charge is not completed around it without explicit human word. The charge is a \
mandate — labor only within it; work outside the charge is not done.\n\n\
LEX RECORDI — the Law of Memory. recall precedes learn — no memory is written \
before its duplicate is sought. Every memory carries a known tag. A stale memory is \
forgotten, not left to mislead.\n\n\
LEX TERMINI — the Law of the Ending. The burden of proof of completion lies on you. \
A charge is done only when verify's criteria are satisfied and any linked PR is \
merged — not merely opened. A settled step is not relitigated. Opening the road is \
not walking it.\n\n\
MAXIMS, for the fork no Lex names. NON VAGA: when the way is unclear, take the most \
specific path already open — the current province, the nearest charge, the \
documented default — and proceed without asking leave. ITER UNUM: commit to one \
plan; never lay two roads before the one who sent you; if genuinely blocked, ask \
exactly one question, never a list. AD FINEM: a step logged, a PR opened, a note \
left are waypoints, not the destination — continue until the charge is done.\n\n\
THE VIAE — the kinds of work. Before labor, declare the one Via that fits the \
charge (Iter Unum: one charge, one Via); its Ritus fixes the ordered acts and \
its Testes fix the proof of completion. VIA GENESIS (new capability): recall \
prior art, declare acceptance criteria, build the smallest whole that meets one, \
witness it, record — done only when every criterion is met and a test covers the \
new behaviour. VIA RENOVATIO (improve without changing behaviour): pin the \
standing behaviour with a test FIRST, then change, then witness the pinning \
tests pass identically. VIA EMENDATIO (repair a defect): reproduce, locate, \
prove with a FAILING test, mend, witness that test passes and none regresses. \
VIA EXPLORATIO (investigate, no code changed): frame one question, gather \
evidence, draw only conclusions the evidence carries, record findings. VIA \
VALIDATIO (verify): name what must be proven, run the check, record the TRUE \
result — a failure is reported, never buried. VIA PUBLICATIO (release): confirm \
all Testes green, assemble the record, open and link the PR, halt until it is \
merged.\n\n\
THE ORDINES — which tools serve which office (orientation, not law; one Ritus \
draws from several). Ordo Fundandi: add, plan_import, modify, move_task, \
assignment, rationale, projects, tags. Ordo Itineris: list, info, next, steps, \
check, step_done, step_undone, step_remove, start, stop. Ordo Nexus: dep, link, \
unlink, attach. Ordo Recordi: recall, learn, forget, promote, relearn, memories, \
link_memory, unlink_memory, prune_memories, annotate, denotate, record_run. Ordo \
Termini: verify, validate, done, feedback, resolve. The interactive surfaces — \
init, delete, reset, undo, sync — are Ordo Hominis, reserved for the human hand; \
they are not exposed here.";

/// Select the instruction register from the `SARA_MCP_INSTRUCTIONS` environment
/// variable. `romanum` (the Adeptus Machinae register) is the default; `classic`
/// restores the original plain-English text. Any unrecognised value falls back to
/// the default so a typo never yields empty instructions.
pub(crate) fn resolve_instructions() -> &'static str {
    match std::env::var("SARA_MCP_INSTRUCTIONS").as_deref() {
        Ok("classic") => CLASSIC_INSTRUCTIONS,
        _ => ROMANUM_INSTRUCTIONS,
    }
}

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
    ///
    /// After the closure returns, a minimal event is written to the `events` table
    /// (action = tool label, project = derived from cwd) so every MCP tool call
    /// is automatically captured as an activity record. Recording errors are
    /// suppressed — a failed INSERT never aborts the tool result.
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
        let result = f(&conn, &self.cfg);
        // Fire-and-forget event recording: derive project name from the registered
        // path that matches the current cwd (best-effort, None if not found).
        let project_name = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(str::to_owned))
            .and_then(|cwd| db::get_project_by_path(&conn, &cwd).ok().flatten())
            .map(|p| p.name);
        let _ = db::record_event(
            &conn,
            label,
            None,
            Some("mcp_tool"),
            &[],
            project_name.as_deref(),
        );
        result
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
        info.instructions = Some(resolve_instructions().to_string());
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = Implementation::new("sara", env!("CARGO_PKG_VERSION"));
        info
    }
}

/// `sara mcp` entry point: serve the MCP tool set over stdio until the client
/// disconnects. Builds the tokio runtime here so the rest of the CLI stays sync.
/// On startup, prunes events older than 90 days (retention policy) so the
/// table doesn't grow unboundedly across long-running server sessions.
pub fn run(conn: Connection, cfg: &Config) -> anyhow::Result<()> {
    let _ = db::prune_old_events(&conn, 90);
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
