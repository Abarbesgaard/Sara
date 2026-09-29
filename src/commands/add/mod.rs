mod input;
mod persist;
mod similar;

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

    match similar::find_similar(
        conn,
        cfg,
        &form.description,
        &split_tags(&form.tags),
        &form.project,
        SIMILAR_LIMIT,
    ) {
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

    if let Some(dup) = find_duplicate_open_task(conn, &form.project, &form.description) {
        println!(
            "⚠ An open task in this project already matches — reuse it instead of duplicating:\n  task {} ({}): {}\n",
            dup.id.unwrap_or(0),
            &dup.uuid.to_string()[..8],
            dup.description
        );
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
        println!(
            "Tied to branch '{branch}' — resolve by uuid across branches to stay unambiguous."
        );
    }
    Ok(())
}

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

    let similar = similar::find_similar(
        conn,
        cfg,
        &form.description,
        &split_tags(&form.tags),
        &form.project,
        SIMILAR_LIMIT,
    )
    .unwrap_or_default();

    let duplicate = find_duplicate_open_task(conn, &form.project, &form.description).map(|t| {
        serde_json::json!({
            "id": t.id,
            "uuid": t.uuid.to_string(),
            "description": t.description,
        })
    });

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
        "duplicate": duplicate,
    }))
}

fn auto_tie_branch(conn: &Connection, task: &Task) -> Option<String> {
    let path = db::get_project(conn, &task.project).ok().flatten()?.path?;
    let branch = crate::infrastructure::git::current_branch(std::path::Path::new(&path))?;
    crate::infrastructure::db::set_task_branch(conn, &task.uuid, &branch).ok()?;
    Some(branch)
}

pub fn parse_due(s: &str, cfg: &Config) -> Option<chrono::DateTime<chrono::Utc>> {
    crate::infrastructure::dates::parse_due(s, &cfg.date_dialect)
}

fn split_tags(tags: &str) -> Vec<String> {
    tags.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn find_duplicate_open_task(conn: &Connection, project: &str, description: &str) -> Option<Task> {
    let want = description.trim().to_lowercase();
    db::list_tasks(conn, Some(project))
        .ok()?
        .into_iter()
        .find(|t| t.description.trim().to_lowercase() == want)
}

fn render_similar_body(confidence: &str, body: &str) -> String {
    if confidence == "semantic" {
        let snippet: String = body.chars().take(200).collect();
        let ellipsis = if body.chars().count() > 200 {
            " …"
        } else {
            ""
        };
        format!("      {}{}\n", snippet.trim(), ellipsis)
    } else {
        body.lines().map(|l| format!("      {l}\n")).collect()
    }
}

fn print_similar_hit(hit: &serde_json::Value) {
    let confidence = hit["confidence"].as_str().unwrap_or("medium");
    if hit["ref_kind"].as_str() == Some("memory") {
        let label = hit["memory"].as_str().unwrap_or("memory");
        let provisional = if hit["provisional"].as_bool().unwrap_or(false) {
            " [provisional]"
        } else {
            ""
        };
        let cross = if hit["same_project"].as_bool().unwrap_or(true) {
            String::new()
        } else {
            format!(
                " [other project: {}]",
                hit["project"].as_str().unwrap_or("?")
            )
        };
        println!(
            "  [memory] [{}] {}{}{}: {}",
            confidence,
            label,
            provisional,
            cross,
            hit["title"].as_str().unwrap_or("")
        );
        let body = hit["body"].as_str().unwrap_or("");
        print!("{}", render_similar_body(confidence, body));
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

#[cfg(test)]
#[path = "../../../tests/unit/commands/add/mod.rs"]
mod tests;
