//! Read-only MCP tools: load and inspect tasks. Contributes `read_router`.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorData;
use rmcp::{tool, tool_router};

use crate::commands;

use super::params::*;
use super::server::{SaraServer, mcp_err, ok_json};

#[tool_router(router = read_router, vis = "pub(crate)")]
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

    #[tool(
        description = "Cross-task keyword search over descriptions, notes, and code anchors, plus exact --tag/--project lookups over learned memories."
    )]
    fn recall(&self, Parameters(p): Parameters<RecallParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp recall", |conn, cfg| {
                commands::recall::recall_value(
                    conn,
                    cfg,
                    &p.query,
                    p.tag.as_deref().unwrap_or(&[]),
                    p.project.as_deref().unwrap_or(&[]),
                    p.limit.unwrap_or(20),
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

    #[tool(description = "List all tag vocabulary with usage counts across active memories.")]
    fn tags(&self, Parameters(p): Parameters<TagsParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp tags", |conn, _cfg| {
                let counts = crate::infrastructure::db::list_tags_with_counts(conn)?;
                Ok(serde_json::json!(
                    counts
                        .into_iter()
                        .map(|(tag, count)| serde_json::json!({ "tag": tag, "count": count }))
                        .collect::<Vec<_>>()
                ))
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "List all known projects with their metadata, task counts, and last activity."
    )]
    fn projects(&self, Parameters(p): Parameters<ProjectsParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp projects", |conn, _cfg| {
                let names = crate::infrastructure::db::project_names(conn)?;
                let mut rows = Vec::with_capacity(names.len());
                for name in names {
                    let profile = crate::infrastructure::db::get_project(conn, &name)?;
                    let stats = crate::infrastructure::db::project_stats(conn, &name)?;
                    let last = crate::infrastructure::db::project_last_activity(conn, &name)?;
                    rows.push(serde_json::json!({
                        "name": name,
                        "goal": profile.as_ref().and_then(|p| p.goal.as_deref()),
                        "stack": profile.as_ref().and_then(|p| p.stack.as_deref()),
                        "pending": stats.pending,
                        "done": stats.completed_total,
                        "last_activity": last.map(|d| d.to_rfc3339()),
                    }));
                }
                Ok(serde_json::Value::Array(rows))
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }
}
