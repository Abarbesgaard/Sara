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
        description = "Search tasks by a partial uuid and return every match (not a single task). Case-insensitive SUBSTRING match on the uuid, hyphens optional — '4c449b0e' matches '…-4c44-9b0e-…'. `%`, `_` and `\\` are literal. `fragment` must be >= 4 chars after hyphens are stripped, else an error. Searches `all` statuses by default (`status`: pending|completed|all; deleted never returned), scoped to the project at `project_path` unless `all: true`. Sorted pending-first then most recently modified, capped at `limit` (default 20). Returns `{matches: [{uuid, id, project, status, description}]}`. Read-only: it never resolves to one task, so no mutation can target it — use it to look up a task you only hold a uuid fragment for.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn find(&self, Parameters(p): Parameters<FindParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp find", |conn, cfg| {
                commands::find::find_value(
                    conn,
                    cfg,
                    &p.fragment,
                    p.status.as_deref(),
                    p.all.unwrap_or(false),
                    p.limit.unwrap_or(commands::find::DEFAULT_LIMIT),
                )
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
        description = "Pick a task back up after a context reset or in a new session, in one compact call: description, assignment and rationale, done steps with their recorded results, open steps, acceptance status, whether it is validated at HEAD, what you were last `doing`, the latest notes, links, memories it cited or that are anchored to its files, and `next_actions`. Omit `id` to resume the project's most recently active pending task. Call it first when you return to work in progress; use `info` when you need the full guide.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn resume(&self, Parameters(p): Parameters<ResumeParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp resume", |conn, cfg| {
                commands::agent::resume_value(conn, cfg, p.id.as_deref())
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
            .with_task(
                p.project_path.as_deref(),
                "mcp next",
                &p.id,
                |conn, _cfg| commands::guide::next_value(conn, &p.id),
            )
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
        description = "Search memories and tasks for prior work. `query` is full-text keyword search (FTS5) over task descriptions, notes, steps, links and memories; `tag`, `project` and `files` filter memories exactly (relative `files` resolve against `project_path`; an entry ending in `/` matches everything under it). With no arguments it returns the most recent memories; `limit` defaults to 10. When `[recall] semantic = true` is set in config.toml, memories are also matched by meaning (embeddings). Always read `confidence` (high|medium|low|semantic|none) and `caveat`: `none` only means no keyword matched, not that no related work exists. `spread: true` also returns graph-related memories that share no keyword, in `associative` (recall also does this by itself when hits are weak). Hits may carry `stale` (an anchored file changed or was deleted: re-check, then `relearn`) and `cluster` (a canonical family collapsed to one hit; `nearest` names the best-matching hidden member). `patterns` lists recurring solutions with their prior `instances`: reuse a pattern's `text` as the task's steps rather than re-deriving it, then `learn` the outcome and `link_memory` it `derived_from` the canonical.",
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
        description = "List memory tags with how many memories use each, most-used first. Scoped to the current project's memories plus global ones by default (scope=\"all\" for every project); narrow with `prefix` and cap with `limit`. Check it before `learn` so you reuse existing tags instead of creating near-duplicates (`service-a` vs `serviceA`).",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn tags(&self, Parameters(p): Parameters<TagsParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp tags", |conn, cfg| {
                let scope = commands::agent::Scope::parse(p.scope.as_deref())?;
                commands::agent::tags_value(conn, cfg, scope, p.prefix.as_deref(), p.limit)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "List active and provisional memories, newest first, with label (e.g. m3), body, tags, files, `strength` and `strength_label` (Strong >= 2.0, Linked >= 1.5, otherwise Weak). Scoped to the current project plus global memories by default (scope=\"all\" for every project) and capped at `limit` (default 50; `total` and `truncated` say what was left out). Use it to audit what recall trusts or to find a label for `relearn`, `promote` or `forget`; to search, use `recall`.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn memories(&self, Parameters(p): Parameters<MemoriesParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp memories", |conn, cfg| {
                let scope = commands::agent::Scope::parse(p.scope.as_deref())?;
                commands::agent::memories_value(conn, cfg, scope, p.limit)
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
        description = "Create (or with remove=true, delete) a typed, directed link from memory `from` to memory `to` (labels such as m3). Relations: `supersedes` (from replaces to; recall flags to as superseded and `maintain action=prune` can archive it), `derived_from` (from applies or builds on to, usually a canonical pattern), `similar_to` (related content), `used_in` (from was used in to's context). `weight` defaults to 1.0. A memory cannot link to itself, and `supersedes`/`derived_from` links that would form a cycle are refused. To retire an outdated memory, prefer `supersedes` over `forget` so the history stays traceable. Removing returns `removed` (0 if no such link existed).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
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
                    if p.remove.unwrap_or(false) {
                        commands::link_memory::unlink_value(conn, &p.from, &p.relation, &p.to)
                    } else {
                        commands::link_memory::link_memory_value(
                            conn,
                            &p.from,
                            &p.relation,
                            &p.to,
                            p.weight.unwrap_or(1.0),
                        )
                    }
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Memory-store maintenance, one `action` per call. diagnose: read-only conflict report of unlinked memory pairs that share a file or tag set AND are semantically close (cosine >= `threshold`), worst first — use it to decide what to relearn, link or forget. consolidate: Hebbian pass setting `co_activated` links between memories recalled within `bucket_secs` of each other over the last `window_days`; idempotent, run it periodically so related memories surface together. reflect: propose canonical clusters among co-recalled but unlinked memories; apply=true creates the `derived_from` links. prune: archive superseded, stale-provisional and old weak memories; dry_run defaults to true (preview), pass dry_run=false to archive (hidden from recall, not deleted). reindex: rebuild the semantic embedding index after enabling semantic recall or bulk imports. Run `doctor` first to see which is needed.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn maintain(&self, Parameters(p): Parameters<MaintainParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp maintain", |conn, _cfg| {
                commands::agent::maintain_value(
                    conn,
                    &commands::agent::MaintainRequest {
                        action: p.action.trim(),
                        dry_run: p.dry_run,
                        apply: p.apply,
                        weak_days: p.weak_days,
                        provisional_days: p.provisional_days,
                        window_days: p.window_days,
                        bucket_secs: p.bucket_secs,
                        delta: p.delta,
                        max_bucket: p.max_bucket,
                        min_weight: p.min_weight,
                        max_cluster: p.max_cluster,
                        threshold: p.threshold,
                        project: p.project.as_deref(),
                        limit: p.limit,
                    },
                )
            })
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
}
