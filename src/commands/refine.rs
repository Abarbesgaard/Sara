use anyhow::Result;
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::model::Project;

/// `sara refine <id>` — hand the task back to Sara. She re-reads the task, the
/// open feedback (flagged-for-reconsider first), open questions, and the repo,
/// then uses her configured LLM to improve the guide: new findings/constraints/
/// anchors/steps are written as `author=ai`, the run is recorded as kind `refine`,
/// and the addressed feedback is marked resolved (linked to the run).
pub fn run(conn: &Connection, cfg: &Config, id: &str, only_flagged: bool) -> Result<()> {
    let task = db::resolve_task(conn, id)?;

    let project = db::get_project(conn, &task.project)?.unwrap_or_else(|| Project {
        name: task.project.clone(),
        path: None,
        goal: None,
        stack: None,
        conventions: None,
        notes: None,
        initialized_at: None,
        last_seen: None,
    });

    // Gather the human comments + open questions to steer the refinement.
    let mut feedback = db::get_open_feedback(conn, &task.uuid)?;
    if only_flagged {
        feedback.retain(|f| f.request_revision);
    }
    let notes = db::get_annotations(conn, &task.uuid)?;
    let open_questions: Vec<&db::Annotation> = notes
        .iter()
        .filter(|a| a.kind == "open_question" && a.status == "open")
        .collect();

    // Compose a refinement brief embedding the current guide + feedback.
    let mut brief = format!(
        "Improve the implementation guide for this task: {}",
        task.description
    );
    let guide = db::get_guide_fields(conn, &task.uuid)?;
    if let Some(r) = &guide.rationale {
        brief.push_str(&format!("\n\nCurrent rationale: {r}"));
    }
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    if !steps.is_empty() {
        brief.push_str("\n\nCurrent steps:");
        for (i, s) in steps.iter().enumerate() {
            brief.push_str(&format!("\n  {}. {}", i + 1, s.text));
        }
    }
    if !feedback.is_empty() {
        brief.push_str("\n\nHuman feedback to address (revise these specifically):");
        for f in &feedback {
            let tgt = match (&f.target_kind, &f.target_id) {
                (Some(k), Some(idv)) => format!(" [{k}:{idv}]"),
                _ => String::new(),
            };
            let flag = if f.request_revision {
                " (RECONSIDER)"
            } else {
                ""
            };
            brief.push_str(&format!("\n  - {}{}: {}", tgt, flag, f.text));
        }
    }
    if !open_questions.is_empty() {
        brief.push_str("\n\nOpen questions (answer or refine):");
        for q in &open_questions {
            brief.push_str(&format!("\n  - {}", q.text));
        }
    }

    let (resp, err) = crate::enrich::enrich_task(conn, cfg, &brief, &project);
    let Some(e) = resp else {
        if let Some(err) = err {
            anyhow::bail!("Refinement failed: {err}");
        }
        anyhow::bail!("Refinement produced no result");
    };

    // Record the run first so resolved feedback can link to it.
    let llm = cfg.effective_llm();
    let response_json = serde_json::to_string(&e).ok();
    let run_id = db::record_ai_run(
        conn,
        &task.uuid,
        "refine",
        Some(&llm.model),
        Some(&llm.provider),
        Some(&brief),
        response_json.as_deref(),
    )?;

    // Append the improvements as AI-sourced guide content.
    let mut added = 0u32;
    if guide.rationale.is_none()
        && let Some(r) = e.rationale.as_deref().filter(|s| !s.trim().is_empty())
    {
        db::set_rationale(conn, &task.uuid, r.trim())?;
        added += 1;
    }
    for step in &e.steps {
        if !step.trim().is_empty() {
            db::add_step(
                conn,
                &task.uuid,
                step.trim(),
                None,
                db::STEP_KIND_STEP,
                "ai",
                None,
            )?;
            added += 1;
        }
    }
    for crit in &e.acceptance_criteria {
        if !crit.trim().is_empty() {
            db::add_step(
                conn,
                &task.uuid,
                crit.trim(),
                None,
                db::STEP_KIND_ACCEPTANCE,
                "ai",
                None,
            )?;
            added += 1;
        }
    }
    let groups: [(&str, &Vec<String>); 5] = [
        ("finding", &e.findings),
        ("constraint", &e.constraints),
        ("non_goal", &e.non_goals),
        ("assumption", &e.assumptions),
        ("open_question", &e.open_questions),
    ];
    for (kind, items) in groups {
        for text in items {
            if !text.trim().is_empty() {
                db::add_annotation_full(
                    conn,
                    &task.uuid,
                    text.trim(),
                    kind,
                    "ai",
                    None,
                    None,
                    false,
                )?;
                added += 1;
            }
        }
    }
    for f in &e.relevant_files {
        if !f.path.trim().is_empty() {
            db::add_task_file(
                conn,
                &task.uuid,
                f.path.trim(),
                db::SOURCE_SUGGESTED,
                f.reason.as_deref(),
                f.symbol.as_deref(),
                f.line_start,
                f.line_end,
            )?;
            added += 1;
        }
    }

    // Mark the addressed feedback resolved and linked to this run.
    let mut resolved = 0u32;
    for f in &feedback {
        if db::resolve_annotation(conn, f.id, Some(run_id))? {
            resolved += 1;
        }
    }

    println!(
        "Refined task {} ({} additions, {} feedback resolved) via {} [{}].",
        task.id.unwrap_or(0),
        added,
        resolved,
        llm.model,
        llm.provider
    );
    Ok(())
}
