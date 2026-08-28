//! Task-reading MCP tools plus the memory-maintenance surface (`forget`,
//! `promote`, `link_memory`, `prune_memories`, `consolidate`, `reflect`,
//! `diagnose_memories`, `reindex_embeddings`). Contributes `read_router`.

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
        description = "Full task guide as JSON: description, steps, acceptance, notes, links, freshness, open feedback. When Strong memories (strength>=2.0) matching the task's description or tags exist, a `similar_work` array is included automatically — check it before starting."
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
        description = "Cross-task keyword search over descriptions, notes, and code anchors, plus exact --tag/--project lookups and --files filter over learned memories. Returns `confidence` (high/medium/none) and a `caveat` string — always read these: `none` with a caveat means FTS found nothing but that does NOT mean no similar work exists (literal keyword search only, no stemming or semantics). Set `spread: true` to also radiate across the memory graph and return associatively-related memories (sharing no keyword) in an `associative` array."
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
                    p.files.as_deref().unwrap_or(&[]),
                    p.limit.unwrap_or(20),
                    p.spread.unwrap_or(false),
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
        description = "Browse all saved memories newest-first with strength labels (Strong/Linked/Weak). Use to audit what recall trusts or to find a memory label for `forget`."
    )]
    fn memories(&self, Parameters(p): Parameters<MemoriesParams>) -> Result<String, ErrorData> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp memories", |conn, _cfg| {
                let items = crate::infrastructure::db::list_memories(conn)?;
                let rows: Vec<serde_json::Value> = items
                    .iter()
                    .map(|m| {
                        let strength = crate::infrastructure::db::item_strength(conn, m);
                        let label = format!(
                            "{}{}",
                            m.kind.chars().next().unwrap_or('m'),
                            m.display_id.unwrap_or(0)
                        );
                        let strength_label = if strength >= 2.0 {
                            "Strong"
                        } else if strength >= 1.5 {
                            "Linked"
                        } else {
                            "Weak"
                        };
                        let files = crate::infrastructure::db::get_item_files(conn, &m.uuid)
                            .unwrap_or_default();
                        serde_json::json!({
                            "label": label,
                            "title": m.title,
                            "body": m.body,
                            "strength": strength,
                            "strength_label": strength_label,
                            "tags": m.tags,
                            "files": files,
                            "created": m.created.to_rfc3339(),
                            "modified": m.modified.to_rfc3339(),
                        })
                    })
                    .collect();
                Ok(serde_json::json!({ "memories": rows }))
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

    #[tool(description = "Create a typed directed link between two memories. \
        Relations: `supersedes` (new replaces old; old shows ⚠ superseded-by in recall), \
        `similar_to` (bidirectional affinity), `derived_from` (this was built on top of that), \
        `used_in` (memory references a context/file). \
        The superseding memory surfaces alongside the stale one in recall output. \
        Use this to invalidate outdated memories rather than deleting them.")]
    fn link_memory(
        &self,
        Parameters(p): Parameters<LinkMemoryParams>,
    ) -> Result<String, ErrorData> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp link_memory",
                |conn, _cfg| {
                    commands::link_memory::link_memory_value(
                        conn,
                        &p.from,
                        &p.relation,
                        &p.to,
                        p.weight.unwrap_or(1.0),
                    )
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Remove a typed directed link between two memories.")]
    fn unlink_memory(
        &self,
        Parameters(p): Parameters<UnlinkMemoryParams>,
    ) -> Result<String, ErrorData> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp unlink_memory",
                |conn, _cfg| commands::link_memory::unlink_value(conn, &p.from, &p.relation, &p.to),
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(description = "Evaluate and optionally archive low-value memories. \
        Three signals: (1) superseded — has incoming `supersedes` edge, \
        (2) provisional + old — auto-generated on `done` but not reviewed within `provisional_days`, \
        (3) weak + old — no task link and older than `weak_days`. \
        Set dry_run=true (default) to preview without archiving. Set dry_run=false to apply. \
        Archived memories are NOT deleted — they can be inspected via direct DB query.")]
    fn prune_memories(
        &self,
        Parameters(p): Parameters<PruneMemoriesParams>,
    ) -> Result<String, ErrorData> {
        use crate::commands::prune_memories::{DEFAULT_PROVISIONAL_DAYS, DEFAULT_WEAK_DAYS};
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp prune_memories",
                |conn, _cfg| {
                    crate::commands::prune_memories::prune_value(
                        conn,
                        p.weak_days.unwrap_or(DEFAULT_WEAK_DAYS),
                        p.provisional_days.unwrap_or(DEFAULT_PROVISIONAL_DAYS),
                        p.dry_run.unwrap_or(true),
                    )
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Hebbian consolidation: sweep recent recall history and \
        reinforce a `co_activated` synapse between every pair of memories that fired \
        together (recalled within `bucket_secs` of each other). This is what makes \
        related memories surface together in future recalls, so run it periodically — \
        recall quality degrades without it. Returns the number of synapses reinforced."
    )]
    fn consolidate(
        &self,
        Parameters(p): Parameters<ConsolidateParams>,
    ) -> Result<String, ErrorData> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp consolidate",
                |conn, _cfg| {
                    let reinforced = crate::infrastructure::memory_graph::consolidate(
                        conn,
                        p.window_days.unwrap_or(30),
                        chrono::Duration::seconds(p.bucket_secs.unwrap_or(5)),
                        p.delta.unwrap_or(0.1),
                        p.max_bucket.unwrap_or(5),
                    )?;
                    Ok(serde_json::json!({ "reinforced": reinforced }))
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Reflect over the memory graph: cluster memories that keep \
        firing together but are not yet tidied, and nominate a canonical memory per \
        cluster. Read-only by default — returns the proposal so you can review it. \
        Pass apply=true to create the proposed `derived_from` edges (each \
        non-canonical member -> the canonical one); links that would trip the \
        cycle guard are skipped and reported."
    )]
    fn reflect(&self, Parameters(p): Parameters<ReflectParams>) -> Result<String, ErrorData> {
        let min_weight = p
            .min_weight
            .unwrap_or(crate::commands::reflect::DEFAULT_MIN_WEIGHT);
        let v = self
            .with_project(p.project_path.as_deref(), "mcp reflect", |conn, _cfg| {
                if p.apply.unwrap_or(false) {
                    commands::reflect::apply_value(conn, min_weight)
                } else {
                    commands::reflect::reflect_value(conn, min_weight)
                }
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Health report for the memory graph: surfaces orphaned, \
        contradictory, duplicated and never-recalled memories. Read-only — archives \
        nothing. Use it to decide what to `relearn`, `forget` or `prune_memories`."
    )]
    fn diagnose_memories(
        &self,
        Parameters(p): Parameters<DiagnoseMemoriesParams>,
    ) -> Result<String, ErrorData> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp diagnose_memories",
                |conn, _cfg| commands::diagnose_memories::diagnose_value(conn),
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Rebuild the semantic embedding index over all memories. \
        Needed after bulk imports, or when `recall`'s semantic pass is missing \
        memories that are obviously relevant. Returns the number embedded."
    )]
    fn reindex_embeddings(
        &self,
        Parameters(p): Parameters<ReindexEmbeddingsParams>,
    ) -> Result<String, ErrorData> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp reindex_embeddings",
                |conn, _cfg| {
                    let embedded = crate::infrastructure::embedding::reindex_all(conn)?;
                    Ok(serde_json::json!({ "embedded": embedded }))
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }
}
