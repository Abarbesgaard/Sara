mod input;
mod persist;
mod similar;

/// How many prior tasks/notes to surface as "similar past work" before creating
/// a new task. Kept small since these are informational, not a hard gate.
const SIMILAR_LIMIT: i64 = 5;

use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;

pub fn run(
    conn: &Connection,
    cfg: &Config,
    words: &[String],
    project_override: Option<&str>,
    priority_override: Option<&str>,
    extra_tags: &[String],
    yes: bool,
    recur_override: Option<&str>,
    annotations: &[String],
    links: &[String],
    checks: &[String],
    depends_on: &[String],
) -> Result<()> {
    let Some((form, recur)) = input::resolve(
        conn,
        cfg,
        words,
        project_override,
        priority_override,
        extra_tags,
        yes,
        recur_override,
    )?
    else {
        println!("Cancelled.");
        return Ok(());
    };

    match similar::find_similar(conn, cfg, &form.description, &split_tags(&form.tags), SIMILAR_LIMIT) {
        Ok(hits) if !hits.is_empty() => {
            println!("Similar past work found — consider reusing instead of starting fresh:");
            for hit in &hits {
                print_similar_hit(hit);
            }
            println!();
        }
        Ok(_) => {}
        Err(e) => eprintln!("Warning: recall check failed: {e}"),
    }

    let task = persist::save(
        conn,
        cfg,
        form,
        recur,
        annotations,
        links,
        checks,
        depends_on,
    )?;

    let tied_branch = auto_tie_branch(conn, &task);

    println!(
        "Created task {} [{}] ({}): {}",
        task.id.unwrap_or(0),
        task.project,
        &task.uuid.to_string()[..8],
        task.description
    );
    if let Some(branch) = tied_branch {
        println!("Tied to branch '{branch}' — resolve by uuid across branches to stay unambiguous.");
    }
    Ok(())
}

/// Create a task and return it — the print-free core for the MCP `add` tool.
/// Always forces `yes = true` so the TUI review form never opens.
#[allow(clippy::too_many_arguments)]
pub fn run_value(
    conn: &Connection,
    cfg: &Config,
    words: &[String],
    project_override: Option<&str>,
    priority_override: Option<&str>,
    extra_tags: &[String],
    recur_override: Option<&str>,
    annotations: &[String],
    links: &[String],
    checks: &[String],
    depends_on: &[String],
) -> Result<serde_json::Value> {
    let Some((form, recur)) = input::resolve(
        conn,
        cfg,
        words,
        project_override,
        priority_override,
        extra_tags,
        true,
        recur_override,
    )?
    else {
        anyhow::bail!("task creation was cancelled");
    };

    let similar =
        similar::find_similar(conn, cfg, &form.description, &split_tags(&form.tags), SIMILAR_LIMIT)
            .unwrap_or_default();

    let task: Task = persist::save(
        conn,
        cfg,
        form,
        recur,
        annotations,
        links,
        checks,
        depends_on,
    )?;

    let tied_branch = auto_tie_branch(conn, &task);

    Ok(serde_json::json!({
        "id": task.id,
        "uuid": task.uuid.to_string(),
        "project": task.project,
        "description": task.description,
        "branch": tied_branch,
        "similar": similar,
    }))
}

/// Best-effort tie the new task to the project repo's current git branch, so
/// downstream commands can detect a wrong-branch mutation by a recycled display
/// id (see `guide::guard_branch_mutation`). Silent no-op when the project has no
/// path, isn't a repo, or is in detached HEAD — the tie is a safety hint, never
/// a hard requirement. Returns the branch it tied, if any.
fn auto_tie_branch(conn: &Connection, task: &Task) -> Option<String> {
    let path = db::get_project(conn, &task.project).ok().flatten()?.path?;
    let branch = crate::infrastructure::git::current_branch(std::path::Path::new(&path))?;
    crate::infrastructure::db::set_task_branch(conn, &task.uuid, &branch).ok()?;
    Some(branch)
}

pub fn parse_due(s: &str, cfg: &Config) -> Option<chrono::DateTime<chrono::Utc>> {
    crate::infrastructure::dates::parse_due(s, &cfg.date_dialect)
}

/// Split a comma-joined tag string into trimmed, non-empty tags — matching how
/// `persist::save` stores them, so tag-exact recall keys on the same values.
fn split_tags(tags: &str) -> Vec<String> {
    tags.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Render one `find_similar` hit for the human terminal. Memory hits print their
/// **full body** (the actionable knowledge); task hits print a snippet + ref.
fn print_similar_hit(hit: &serde_json::Value) {
    let confidence = hit["confidence"].as_str().unwrap_or("medium");
    if hit["ref_kind"].as_str() == Some("memory") {
        let label = hit["memory"].as_str().unwrap_or("memory");
        let provisional = if hit["provisional"].as_bool().unwrap_or(false) {
            " [provisional]"
        } else {
            ""
        };
        println!(
            "  [memory] [{}] {}{}: {}",
            confidence,
            label,
            provisional,
            hit["title"].as_str().unwrap_or("")
        );
        // The whole body, indented — no second recall needed to read it.
        for line in hit["body"].as_str().unwrap_or("").lines() {
            println!("      {line}");
        }
    } else {
        println!(
            "  [{}] [{}] task {}: {} — {}",
            hit["ref_kind"].as_str().unwrap_or(""),
            confidence,
            hit["task"].as_i64().unwrap_or(0),
            hit["description"].as_str().unwrap_or(""),
            hit["snippet"].as_str().unwrap_or("")
        );
    }
}
