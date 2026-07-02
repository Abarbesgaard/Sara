//! Create + guide-editing MCP tools: build a task and shape its guide (steps,
//! notes, anchors, assignment/rationale). Contributes `guide_router`.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorData;
use rmcp::{tool, tool_router};

use crate::commands;

use super::params::*;
use super::server::{SaraServer, mcp_err, ok_json};

#[tool_router(router = guide_router, vis = "pub(crate)")]
impl SaraServer {
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
}
