use anyhow::Result;
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::enrich;
use crate::model::{Priority, Task};
use crate::project::{detect_current_project, parse_add_tokens};
use crate::tui;
use crate::tui::review_form::{FormContext, FormInput, run_form};

pub fn run(
    conn: &Connection,
    cfg: &Config,
    words: &[String],
    project_override: Option<&str>,
    priority_override: Option<&str>,
    extra_tags: &[String],
    yes: bool,
    no_llm: bool,
    recur_override: Option<&str>,
) -> Result<()> {
    // Parse inline tokens
    let mut parsed = parse_add_tokens(words);

    if parsed.description.trim().is_empty() {
        anyhow::bail!("Task description cannot be empty");
    }

    // Flags override inline tokens
    if let Some(p) = project_override {
        parsed.project = Some(p.to_string());
    }
    if let Some(p) = priority_override {
        parsed.priority = Some(p.to_uppercase());
    }
    parsed.tags.extend_from_slice(extra_tags);
    if let Some(r) = recur_override {
        parsed.recur = Some(r.to_string());
    }

    // Resolve project
    let (project_name, _path) = if let Some(ref p) = parsed.project {
        let path_opt = db::get_project(conn, p)?.and_then(|pr| pr.path);
        db::upsert_project_seen(conn, p, path_opt.as_deref())?;
        (p.clone(), path_opt)
    } else {
        detect_current_project(conn, cfg)?
    };

    let project_profile = db::get_project(conn, &project_name)?.unwrap_or(crate::model::Project {
        name: project_name.clone(),
        path: None,
        goal: None,
        stack: None,
        conventions: None,
        notes: None,
        initialized_at: None,
        last_seen: None,
    });

    // LLM enrichment runs by default (Azure/Ollama from config). Skip with --no-llm.
    let (enrichment, llm_error) = if no_llm {
        (None, None)
    } else {
        enrich::enrich_task(conn, cfg, &parsed.description, &project_profile)
    };

    // Check if we're in a TTY
    let is_tty = atty_check();
    let yes = yes || words.iter().any(|w| w == "--yes" || w == "-y");

    let form_result: Option<FormInput> = if yes || !is_tty {
        // Non-interactive: accept LLM proposals directly
        let priority = enrichment
            .as_ref()
            .and_then(|e| e.priority.as_deref())
            .and_then(|p| p.parse::<Priority>().ok())
            .or_else(|| parsed.priority.as_deref().and_then(|p| p.parse().ok()));
        let due = enrichment
            .as_ref()
            .and_then(|e| e.due.clone())
            .unwrap_or_default();
        let mut tags = parsed.tags.clone();
        if let Some(ref e) = enrichment {
            for t in &e.tags {
                if !tags.contains(t) {
                    tags.push(t.clone());
                }
            }
        }
        Some(FormInput {
            description: parsed.description.clone(),
            project: project_name.clone(),
            priority,
            due,
            tags: tags.join(","),
            selected_deps: vec![],
            selected_files: vec![],
        })
    } else {
        // Build form context
        let pending = db::list_tasks(conn, None)?;
        let available_deps: Vec<(String, String)> = pending
            .iter()
            .map(|t| {
                let short = format!("{}", t.id.unwrap_or(0));
                (short, t.description.clone())
            })
            .collect();

        // Project files *and folders* for the manual picker.
        let project_files: Vec<String> = project_profile
            .path
            .as_deref()
            .map(|p| crate::files::collect_project_entries(std::path::Path::new(p)))
            .unwrap_or_default();

        let priority_init = enrichment
            .as_ref()
            .and_then(|e| e.priority.as_deref())
            .and_then(|p| p.parse::<Priority>().ok())
            .or_else(|| parsed.priority.as_deref().and_then(|p| p.parse().ok()));

        let mut init_tags = parsed.tags.clone();
        if let Some(ref e) = enrichment {
            for t in &e.tags {
                if !init_tags.contains(t) {
                    init_tags.push(t.clone());
                }
            }
        }

        let ctx = FormContext {
            initial: FormInput {
                description: parsed.description.clone(),
                project: project_name.clone(),
                priority: priority_init,
                due: enrichment
                    .as_ref()
                    .and_then(|e| e.due.clone())
                    .unwrap_or_default(),
                tags: init_tags.join(","),
                selected_deps: vec![],
                selected_files: vec![],
            },
            available_deps,
            available_files: project_files,
            suggested_dep_indices: vec![], // could map LLM dep suggestions here
            suggested_files: vec![],
            llm_status: llm_error,
        };

        let mut terminal = tui::init_terminal()?;
        let result = run_form(&mut terminal, ctx);
        tui::restore_terminal()?;
        result?
    };

    let Some(form) = form_result else {
        println!("Cancelled.");
        return Ok(());
    };

    // Build and insert the task
    let mut task = Task::new(form.description, form.project.clone());
    task.priority = form.priority;
    task.tags = form
        .tags
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    task.recur = parsed.recur.clone();

    // Parse due date
    if !form.due.is_empty() {
        task.due = parse_due(&form.due, cfg);
    }

    // Compute initial urgency
    task.urgency = db::compute_urgency(&task, &cfg.urgency, false, 0);

    db::insert_task(conn, &mut task)?;

    // Attach selected files/folders. All are user-picked (manual). Paths come
    // straight from the form and may include arbitrary paths the user typed.
    if !form.selected_files.is_empty() {
        db::set_task_files(conn, &task.uuid, &form.selected_files)?;
    }

    // Add selected deps
    let pending = db::list_tasks(conn, None)?;
    for &dep_idx in &form.selected_deps {
        if let Some(dep_task) = pending.get(dep_idx)
            && let Err(e) = db::add_dependency(conn, &task.uuid, &dep_task.uuid)
        {
            eprintln!("Warning: could not add dependency: {e}");
        }
    }

    // Persist the full LLM-authored guide (steps, acceptance, findings, anchors,
    // rationale, deps) with provenance — so the task is a self-contained guide.
    if let Some(ref e) = enrichment {
        persist_enrichment(conn, cfg, &task, e, &parsed.description, _path.as_deref())?;
    }

    // Refresh urgency with blocking info
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    println!(
        "Created task {} [{}]: {}",
        task.id.unwrap_or(0),
        task.project,
        task.description
    );
    Ok(())
}

/// Write everything the LLM attached to the task, each item tagged as AI-sourced,
/// and record the enrichment run for the audit trail.
fn persist_enrichment(
    conn: &Connection,
    cfg: &Config,
    task: &Task,
    e: &crate::llm::EnrichmentResponse,
    original_description: &str,
    project_path: Option<&str>,
) -> Result<()> {
    let uuid = &task.uuid;

    // Traceability: the prompt that spawned this task.
    if !original_description.trim().is_empty() {
        let _ = db::set_assignment(conn, uuid, original_description.trim());
    }
    if let Some(r) = e.rationale.as_deref().filter(|s| !s.trim().is_empty()) {
        let _ = db::set_rationale(conn, uuid, r.trim());
    }

    // Ordered steps + acceptance criteria (AI-authored).
    for step in &e.steps {
        if !step.trim().is_empty() {
            let _ = db::add_step(
                conn,
                uuid,
                step.trim(),
                None,
                db::STEP_KIND_STEP,
                "ai",
                None,
            );
        }
    }
    for crit in &e.acceptance_criteria {
        if !crit.trim().is_empty() {
            let _ = db::add_step(
                conn,
                uuid,
                crit.trim(),
                None,
                db::STEP_KIND_ACCEPTANCE,
                "ai",
                None,
            );
        }
    }

    // Typed, attributed notes.
    let note_groups: [(&str, &Vec<String>); 5] = [
        ("finding", &e.findings),
        ("constraint", &e.constraints),
        ("non_goal", &e.non_goals),
        ("assumption", &e.assumptions),
        ("open_question", &e.open_questions),
    ];
    for (kind, items) in note_groups {
        for text in items {
            if !text.trim().is_empty() {
                let _ =
                    db::add_annotation_full(conn, uuid, text.trim(), kind, "ai", None, None, false);
            }
        }
    }

    // Relevant files / code anchors (AI-suggested).
    for f in &e.relevant_files {
        if f.path.trim().is_empty() {
            continue;
        }
        let _ = db::add_task_file(
            conn,
            uuid,
            f.path.trim(),
            db::SOURCE_SUGGESTED,
            f.reason.as_deref(),
            f.symbol.as_deref(),
            f.line_start,
            f.line_end,
        );
    }

    // Suggested dependencies (resolve uuid-prefix or id tokens to tasks).
    for dep in &e.suggested_dependencies {
        if let Ok(dep_task) = db::resolve_task(conn, dep.trim()) {
            let _ = db::add_dependency(conn, uuid, &dep_task.uuid);
        }
    }

    // Verification commands + any extra metadata in the JSON1 grab-bag.
    let mut meta = serde_json::Map::new();
    if let Some(c) = e.test_cmd.as_deref().filter(|s| !s.trim().is_empty()) {
        meta.insert("test_cmd".into(), serde_json::Value::String(c.to_string()));
    }
    if let Some(c) = e.lint_cmd.as_deref().filter(|s| !s.trim().is_empty()) {
        meta.insert("lint_cmd".into(), serde_json::Value::String(c.to_string()));
    }
    if !meta.is_empty() {
        let _ = db::set_meta_json(conn, uuid, &serde_json::Value::Object(meta).to_string());
    }

    // Freshness: stamp the commit the guide was validated against.
    if let Some(path) = project_path
        && let Some(sha) = crate::git::head_commit(std::path::Path::new(path))
    {
        let _ = db::set_validated(conn, uuid, &sha);
    }

    // Audit trail: record the enrichment run.
    let llm = cfg.effective_llm();
    let response_json = serde_json::to_string(e).ok();
    let _ = db::record_ai_run(
        conn,
        uuid,
        "enrich",
        Some(&llm.model),
        Some(&llm.provider),
        None,
        response_json.as_deref(),
    );

    Ok(())
}

fn atty_check() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

pub fn parse_due(s: &str, cfg: &Config) -> Option<chrono::DateTime<chrono::Utc>> {
    crate::dates::parse_due(s, &cfg.date_dialect)
}
