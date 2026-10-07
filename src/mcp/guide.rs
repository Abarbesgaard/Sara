use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};

use crate::commands;

use super::params::*;
use super::server::{SaraServer, mcp_err, ok_json};

#[tool_router(router = guide_router, vis = "pub(crate)")]
impl SaraServer {
    #[tool(
        description = "Start a task in one call: create it, set its assignment/why, register an optional acceptance criterion, and seed the FIRST step — an explicit, agent-run recall of prior art. Returns the task, its criteria, the seeded recall step, and the next cursor (which points AT that recall step). Use this to BEGIN work. begin does NOT recall for you: it directs you to decide what prior knowledge bears on the task and call `recall` yourself as your first step, so prior art is early yet purposeful. An acceptance criterion is optional (a warning is returned if omitted).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn begin(&self, Parameters(p): Parameters<BeginParams>) -> Result<String, String> {
        let tags = p.tags.clone().unwrap_or_default();
        let files = p.files.clone().unwrap_or_default();
        let v = self
            .with_project(p.project_path.as_deref(), "mcp begin", |conn, cfg| {
                commands::begin::begin_value(
                    conn,
                    cfg,
                    crate::infrastructure::telemetry::Source::Mcp,
                    &p.description,
                    &tags,
                    &files,
                    p.project.as_deref(),
                    p.priority.as_deref(),
                    p.assignment.as_deref(),
                    p.rationale.as_deref(),
                    p.check.as_deref(),
                    p.verify.as_deref(),
                )
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Create a task without opening the TUI. Before saving, it searches for similar tasks and memories and returns them in `similar` (memory hits carry the full body: read them first, they often hold the answer), plus `duplicate` when an open task with the same description already exists; the task is created either way. The task is tied to the current git branch (`branch`). Returns its `id` and `uuid`. To start work you are about to do, prefer `begin`.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn add(&self, Parameters(p): Parameters<AddParams>) -> Result<String, String> {
        let words = vec![p.description.clone()];
        let tags = p.tags.clone().unwrap_or_default();
        let annotations = p.annotations.clone().unwrap_or_default();
        let links = p.links.clone().unwrap_or_default();
        let checks = p.checks.clone().unwrap_or_default();
        let depends_on = p.depends_on.clone().unwrap_or_default();
        let v = self
            .with_project(p.project_path.as_deref(), "mcp add", |conn, cfg| {
                let req = commands::add::AddRequest {
                    words: &words,
                    project: p.project.as_deref(),
                    priority: p.priority.as_deref(),
                    tags: &tags,
                    recur: p.recur.as_deref(),
                    annotations: &annotations,
                    links: &links,
                    checks: &checks,
                    depends_on: &depends_on,
                };
                commands::add::run_value(conn, cfg, &req)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Mark a step DONE (ticks the box), recording a result and the current git commit. With neither `n` nor `step_id`, it completes the current step — the first not-done one that `next` returns — so a `next` -> `step_done` round-trip needs no position tracking. Address a specific item by `n` (the `index` returned by check/steps) OR by `step_id` (the rowid check returns). This is how you satisfy an acceptance criterion: pass kind=\"acceptance\" (with its 1-based `n`, or none to tick the first outstanding one). Do NOT call `check` to tick — check only adds. Pass `used` with the memory labels (e.g. [\"m12\"]) that actually helped, to cite them; an unknown label fails the whole call and nothing changes.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn step_done(&self, Parameters(p): Parameters<StepDoneParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp step_done", |conn, _cfg| {
                let used = p.used.as_deref().unwrap_or(&[]);
                commands::shared::with_citation(conn, &p.id, used, || {
                    if let Some(step_id) = p.step_id {
                        commands::guide::step_done_by_id_value(conn, step_id, p.result.as_deref())
                    } else if let Some(n) = p.n {
                        commands::guide::step_done_value(
                            conn,
                            &p.id,
                            n,
                            p.result.as_deref(),
                            p.kind.as_deref(),
                        )
                    } else {
                        commands::guide::step_done_current_value(
                            conn,
                            &p.id,
                            p.result.as_deref(),
                            p.kind.as_deref(),
                        )
                    }
                })
                .and_then(|v| self.report_doing(conn, &p.id, p.doing.as_deref(), v))
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Report what you are doing RIGHT NOW on a task, as one short line (e.g. \"running the parser tests\"). It is shown live to the human watching `sara follow`; it is not a note and is not kept as evidence. Call it whenever you switch to a new activity; `step_done` and `annotate` also take an optional `doing` to report in the same call.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn doing(&self, Parameters(p): Parameters<DoingParams>) -> Result<String, String> {
        let client = self.client_name();
        let v = self
            .with_project(p.project_path.as_deref(), "mcp doing", |conn, _cfg| {
                commands::doing::doing_value(conn, &p.id, &p.text, client.as_deref())
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Add a note to a task. `kind` labels it (default `comment`; e.g. finding, decision, constraint, risk, assumption, open_question). `on` anchors it to `step:N`, `acceptance:N`, `anchor:ID` or `note:ID`. `author` defaults to `human`. `reconsider: true` flags it for reconsideration. A `finding` also returns `related_findings`: similar earlier findings on the task; correct any your new note contradicts.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn annotate(&self, Parameters(p): Parameters<AnnotateParams>) -> Result<String, String> {
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
                .and_then(|v| self.report_doing(conn, &p.id, p.doing.as_deref(), v))
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Create a task graph in one call from `plan_json`: {\"project\"?, \"tasks\": [{\"key\", \"description\", \"assignment\", \"rationale\", \"priority\", \"tags\", \"steps\", \"acceptance\", \"findings\", \"constraints\", \"files\": [{\"path\", \"reason\", \"symbol\", \"line_start\", \"line_end\"}], \"depends_on\"}]}. Only `description` is required. `depends_on` entries are other tasks' `key` in the same plan or existing task ids/UUID prefixes. Runs in one transaction.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn plan_import(&self, Parameters(p): Parameters<PlanImportParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp plan_import", |conn, cfg| {
                commands::plan::import_raw(conn, cfg, &p.plan_json)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "ADD a new checklist step (or an acceptance criterion with kind=\"acceptance\") to a task's guide, optionally with an `intent` note and a `verify` command. This APPENDS an item; it does NOT tick one off (use `step_done`). Give acceptance criteria a `verify` command, or `validate` will refuse (a `warning` is returned). Returns the new item's `step_id` and 1-based `index`; pass either to step_done/step_undone/step_remove.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn check(&self, Parameters(p): Parameters<CheckParams>) -> Result<String, String> {
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

    #[tool(
        description = "Reopen a completed step (or acceptance criterion with kind=\"acceptance\"). Address it by `n` (1-based `index` from steps/check) OR by `step_id` (from check).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn step_undone(&self, Parameters(p): Parameters<StepEditParams>) -> Result<String, String> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp step_undone",
                |conn, _cfg| {
                    if let Some(step_id) = p.step_id {
                        commands::guide::step_undone_by_id_value(conn, step_id)
                    } else {
                        let n = p.n.ok_or_else(|| {
                            anyhow::anyhow!("provide `n` (1-based position) or `step_id`")
                        })?;
                        commands::guide::step_undone_value(conn, &p.id, n, p.kind.as_deref())
                    }
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Delete a step (or acceptance criterion with kind=\"acceptance\") from a task's guide; later items are renumbered. Address it by `n` (1-based `index` from steps/check) OR by `step_id` (from check).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn step_remove(&self, Parameters(p): Parameters<StepEditParams>) -> Result<String, String> {
        let v = self
            .with_project(
                p.project_path.as_deref(),
                "mcp step_remove",
                |conn, _cfg| {
                    if let Some(step_id) = p.step_id {
                        commands::guide::step_remove_by_id_value(conn, step_id)
                    } else {
                        let n = p.n.ok_or_else(|| {
                            anyhow::anyhow!("provide `n` (1-based position) or `step_id`")
                        })?;
                        commands::guide::step_remove_value(conn, &p.id, n, p.kind.as_deref())
                    }
                },
            )
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Set (replace) a task's assignment: the originating request, in the requester's words.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn assignment(&self, Parameters(p): Parameters<GuideTextParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp assignment", |conn, _cfg| {
                commands::guide::assignment_value(conn, &p.id, &p.text)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Set (replace) a task's rationale: why the task exists.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn rationale(&self, Parameters(p): Parameters<GuideTextParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp rationale", |conn, _cfg| {
                commands::guide::rationale_value(conn, &p.id, &p.text)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Attach a file or code anchor to a task; a URL is stored as a link instead. Optional anchor metadata: `reason`, `symbol`, `lines` as \"start:end\" (e.g. \"10:57\") and `source` (`ai` marks it as suggested; default human).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn attach(&self, Parameters(p): Parameters<AttachParams>) -> Result<String, String> {
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

    #[tool(
        description = "Save one distilled insight as a memory. Bodies over 2000 characters or that look like secrets are refused; force=true skips those checks and the canonical auto-link below. Run `recall` first and check `tags` to reuse existing tag names. `files` binds the memory to files (relative paths resolve against `project_path`) so later edits can flag it stale; `tasks` links tasks by id or UUID prefix; `projects` scopes it. If its tags place it inside an established canonical pattern (a same-project near-duplicate, or >= 50% tag overlap with a canonical), it is auto-linked `derived_from` that canonical and reported in `auto_derived_from`. Returns the new memory's `label`. To mark it as superseding or related to an existing memory, call `link_memory` afterwards.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn learn(&self, Parameters(p): Parameters<LearnParams>) -> Result<String, String> {
        let tags = p.tags.unwrap_or_default();
        let projects = p.projects.unwrap_or_default();
        let tasks = p.tasks.unwrap_or_default();
        let files = p.files.unwrap_or_default();
        let v = self
            .with_project(p.project_path.as_deref(), "mcp learn", |conn, cfg| {
                commands::learn::learn_value(
                    conn,
                    cfg,
                    &commands::learn::LearnRequest {
                        text: &p.text,
                        tags: &tags,
                        projects: &projects,
                        tasks: &tasks,
                        files: &files,
                        force: p.force.unwrap_or(false),
                        ..Default::default()
                    },
                )
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Archive (forget) a memory by its label, e.g. \"m3\", when it is stale or wrong; it is hidden from recall but not deleted. If it is a canonical with derived_from children, they are listed for review, and archived too when cascade=true. To correct a memory instead, use `relearn`.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn forget(&self, Parameters(p): Parameters<ForgetParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp forget", |conn, _cfg| {
                commands::forget::forget_value(conn, &p.handle, p.cascade.unwrap_or(false))
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Promote a provisional memory (auto-created by `done`) to active after you have reviewed it, e.g. \"m14\". Keeps its tags, files and task links.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn promote(&self, Parameters(p): Parameters<PromoteParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp promote", |conn, _cfg| {
                commands::promote::promote_value(conn, &p.handle)
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Edit a memory in place by label: pass at least one of `text`, `tags`, `files`. Given `tags` or `files` REPLACE the existing set; omitted ones are kept. Keeps the label, created date, task links and memory links, and re-fingerprints the memory's files (clearing a `stale` flag). Prefer this over forget + learn.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn relearn(&self, Parameters(p): Parameters<RelearnParams>) -> Result<String, String> {
        let v = self
            .with_project(p.project_path.as_deref(), "mcp relearn", |conn, _cfg| {
                commands::relearn::relearn_value(
                    conn,
                    &p.handle,
                    p.text.as_deref(),
                    p.tags.as_deref().unwrap_or(&[]),
                    p.files.as_deref().unwrap_or(&[]),
                    p.force.unwrap_or(false),
                )
            })
            .map_err(mcp_err)?;
        ok_json(v)
    }
}
