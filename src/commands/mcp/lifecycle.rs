//! Completion / edit / lifecycle MCP tools: dependencies, links, validation,
//! field edits, feedback resolution, time tracking, and completion. Contributes
//! `lifecycle_router`.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorData;
use rmcp::{tool, tool_router};

use crate::commands;

use super::params::*;
use super::server::{SaraServer, mcp_err, ok_json};

#[tool_router(router = lifecycle_router, vis = "pub(crate)")]
impl SaraServer {
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
