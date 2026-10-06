use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};

use crate::commands;
use crate::infrastructure::db;

use super::params::*;
use super::server::{SaraServer, mcp_err, ok_json};

#[tool_router(router = read_router, vis = "pub(crate)")]
impl SaraServer {
    #[tool(
        description = "List pending tasks, ranked by urgency, for the project at `project_path`. Pass `project` to list another project by name, or `all: true` to list every project.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn list(&self, Parameters(p): Parameters<ListParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp list", |conn, cfg| {
                commands::list::list_value(conn, cfg, p.all.unwrap_or(false), p.project.as_deref())
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Full task guide as JSON: description, steps, acceptance criteria, notes, links, freshness and open feedback. Includes a `similar_work` array when Strong memories (strength >= 2.0) match the task's description or tags; read it before starting.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn info(&self, Parameters(p): Parameters<IdParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp info", |conn, _cfg| {
                commands::info::guide_value(conn, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "The execution cursor: the first not-done step of a task (steps only, not acceptance criteria), with its 1-based `index`, `total`, `text`, `intent` and `verify_cmd`. Returns `done: true` once every step is done. Includes `relevant_memories` when Strong memories match the task. Call `step_done` without `n` to complete this step.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn next(&self, Parameters(p): Parameters<IdParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp next", |conn, _cfg| {
                commands::guide::next_value(conn, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "List a task's ordered steps (not acceptance criteria) with their 1-based `index`. `until` returns only steps 1..until.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn steps(&self, Parameters(p): Parameters<StepsParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp steps", |conn, _cfg| {
                commands::guide::steps_value(conn, &p.id, p.until)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Read-only: list a task's verify commands and acceptance criteria without running them. `step` limits it to one step. To run them and stamp the task green, use `validate`.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn verify(&self, Parameters(p): Parameters<VerifyParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp verify", |conn, _cfg| {
                commands::guide::verify_value(conn, &p.id, p.step)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Search memories and tasks for prior work. `query` is full-text keyword search (FTS5) over task descriptions, notes, steps, links and memories; `tag`, `project` and `files` filter memories exactly (a `files` entry ending in `/` matches everything under it). With no arguments it returns the most recent memories; `limit` defaults to 10. When `[recall] semantic = true` is set in config.toml, memories are also matched by meaning (embeddings). Always read `confidence` (high|medium|low|semantic|none) and `caveat`: `none` only means no keyword matched, not that no related work exists. `spread: true` also returns graph-related memories that share no keyword, in `associative` (recall also does this by itself when hits are weak). Hits may carry `stale` (an anchored file changed or was deleted: re-check, then `relearn`) and `cluster` (a canonical family collapsed to one hit; `nearest` names the best-matching hidden member). `patterns` lists recurring solutions with their prior `instances`: reuse a pattern's `text` as the task's steps rather than re-deriving it, then `learn` the outcome and `link_memory` it `derived_from` the canonical.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn recall(&self, Parameters(p): Parameters<RecallParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp recall", |conn, cfg| {
                let task = commands::recall::use_task(conn, p.task.as_deref())?;
                db::with_use_attribution(task, || {
                    commands::recall::recall_value(
                        conn,
                        cfg,
                        &p.query,
                        p.tag.as_deref().unwrap_or(&[]),
                        p.project.as_deref().unwrap_or(&[]),
                        p.files.as_deref().unwrap_or(&[]),
                        p.limit.unwrap_or(10),
                        p.spread.unwrap_or(false),
                    )
                })
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "List a task's open human feedback: annotations still awaiting a response. Pass an item's id to `resolve` as `feedback_id`.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn feedback(&self, Parameters(p): Parameters<IdParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp feedback", |conn, _cfg| {
                commands::guide::feedback_value(conn, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Dependency-ordered briefing: the full guide of the task and of every task it is blocked by, in dependency order.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn plan_show(&self, Parameters(p): Parameters<IdParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp plan_show", |conn, _cfg| {
                commands::plan::show_value(conn, &p.id)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "List every tag on active memories with how many memories use it. Check it before `learn` so you reuse existing tags instead of creating near-duplicates (`service-a` vs `serviceA`).",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn tags(&self, Parameters(p): Parameters<TagsParams>) -> Result<String, String> {
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
        description = "List every active and provisional memory, newest first, with its label (e.g. m3), body, tags, files, `strength` and `strength_label` (Strong >= 2.0, Linked >= 1.5, otherwise Weak). Use it to audit what recall trusts or to find a label for `relearn`, `promote` or `forget`.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn memories(&self, Parameters(p): Parameters<MemoriesParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp memories", |conn, _cfg| {
                let items = crate::infrastructure::db::list_memories(conn)?;
                let strengths = crate::infrastructure::db::item_strengths(conn, &items);
                let rows: Vec<serde_json::Value> = items
                    .iter()
                    .map(|m| {
                        let strength = strengths.get(&m.uuid).copied().unwrap_or(1.0);
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
        description = "List every known project with its goal, stack, pending and done task counts, and last activity.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn projects(&self, Parameters(p): Parameters<ProjectsParams>) -> Result<String, String> {
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

    #[tool(
        description = "Create a typed, directed link from memory `from` to memory `to` (labels such as m3). Relations: `supersedes` (from replaces to; recall flags to as superseded and `prune_memories` can archive it), `derived_from` (from applies or builds on to, usually a canonical pattern), `similar_to` (related content), `used_in` (from was used in to's context). `weight` defaults to 1.0. A memory cannot link to itself, and `supersedes`/`derived_from` links that would form a cycle are refused. To retire an outdated memory, prefer `supersedes` over `forget` so the history stays traceable.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn link_memory(&self, Parameters(p): Parameters<LinkMemoryParams>) -> Result<String, String> {
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

    #[tool(
        description = "Remove the `relation` link from memory `from` to memory `to`. Returns `removed` (0 if no such link existed).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn unlink_memory(
        &self,
        Parameters(p): Parameters<UnlinkMemoryParams>,
    ) -> Result<String, String> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp unlink_memory",
                |conn, _cfg| commands::link_memory::unlink_value(conn, &p.from, &p.relation, &p.to),
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Find and optionally archive low-value memories: (1) superseded, i.e. targeted by a `supersedes` link; (2) provisional and not reviewed within `provisional_days`; (3) weak (no task link) and older than `weak_days`. `dry_run` defaults to true and only previews; pass dry_run=false to archive. Archived memories are hidden from recall but not deleted.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn prune_memories(
        &self,
        Parameters(p): Parameters<PruneMemoriesParams>,
    ) -> Result<String, String> {
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
        description = "Hebbian consolidation: sweep the last `window_days` of recall history and set a `co_activated` link between every pair of memories recalled within `bucket_secs` of each other, weighted by how often they co-fired. The wiring is recomputed from the window on every run, so it is idempotent and pairs that no longer co-fire decay away. This is what makes related memories surface together, so run it periodically. Returns `reinforced`: the number of co_activated links.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn consolidate(&self, Parameters(p): Parameters<ConsolidateParams>) -> Result<String, String> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp consolidate",
                |conn, _cfg| {
                    let reinforced = crate::infrastructure::memory::graph::consolidate(
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
        description = "Find clusters of memories that keep being recalled together but are not yet linked, and nominate a canonical memory for each. Read-only by default: returns the proposal for review. Pass apply=true to create the proposed `derived_from` links (each other member -> the canonical); links that would form a cycle are skipped and reported.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn reflect(&self, Parameters(p): Parameters<ReflectParams>) -> Result<String, String> {
        let min_weight = p
            .min_weight
            .unwrap_or(crate::commands::reflect::DEFAULT_MIN_WEIGHT);
        let max_cluster = p
            .max_cluster
            .unwrap_or(crate::commands::reflect::DEFAULT_MAX_CLUSTER);
        let v = self
            .with_project(p.project_path.as_deref(), "mcp reflect", |conn, _cfg| {
                if p.apply.unwrap_or(false) {
                    commands::reflect::apply_value(conn, min_weight, max_cluster)
                } else {
                    commands::reflect::reflect_value(conn, min_weight, max_cluster)
                }
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Read-only conflict report: unlinked memory pairs that share a file or have identical tag sets AND are semantically close (cosine >= `threshold`), worst first. Archives nothing. Use it to decide what to `relearn`, `link_memory`, `forget` or `prune_memories`.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn diagnose_memories(
        &self,
        Parameters(p): Parameters<DiagnoseMemoriesParams>,
    ) -> Result<String, String> {
        let threshold = p
            .threshold
            .unwrap_or(commands::diagnose_memories::DEFAULT_CONFLICT_THRESHOLD);
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp diagnose_memories",
                |conn, _cfg| {
                    commands::diagnose_memories::diagnose_value(
                        conn,
                        threshold,
                        p.project.as_deref(),
                        p.limit,
                    )
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Read-only health report for the memory store; run it before trusting recall. Checks embedding coverage, orphaned memory links, near-duplicate pairs, superseded memories still active, the provisional review backlog, decay outliers and stale file anchors. Each check returns status ok|warn|info, a count, a summary and the `fix` command to run. `healthy` is false when any check warns.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn doctor(&self, Parameters(p): Parameters<DoctorParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp doctor", |conn, cfg| {
                let project = commands::doctor::current_project(conn, cfg);
                commands::doctor::doctor_value(conn, project.as_deref())
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Rebuild the semantic embedding index over all memories. Needed after enabling semantic recall, after bulk imports, or when semantic recall misses obviously relevant memories. Returns `embedded`: the number of memories indexed.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn reindex_embeddings(
        &self,
        Parameters(p): Parameters<ReindexEmbeddingsParams>,
    ) -> Result<String, String> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp reindex_embeddings",
                |conn, _cfg| {
                    let embedded = crate::infrastructure::memory::embedding::reindex_all(conn)?;
                    Ok(serde_json::json!({ "embedded": embedded }))
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }
}
