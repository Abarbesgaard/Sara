//! Project profiles, urgency scoring, checklist/steps, activity stats.
//!
//! Split out of the db monolith (issue #168); re-exported by `super` so
//! `db::*` call sites are unchanged. Shared low-level helpers live in the
//! parent `db` module, reached here via `use super::*`.

use super::*;
use crate::infrastructure::model::{Project, Task};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use uuid::Uuid;

// ── projects ─────────────────────────────────────────────────────────────────

pub fn upsert_project_seen(conn: &Connection, name: &str, path: Option<&str>) -> Result<()> {
    let now = dt_to_str(&Utc::now());
    conn.execute(
        "INSERT INTO projects (name, path, last_seen) VALUES (?1,?2,?3)
         ON CONFLICT(name) DO UPDATE SET
           path     = COALESCE(?2, path),
           last_seen = ?3",
        params![name, path, now],
    )?;
    Ok(())
}

pub fn save_project_profile(conn: &Connection, project: &Project) -> Result<()> {
    let now = dt_to_str(&Utc::now());
    conn.execute(
        "INSERT INTO projects (name, path, goal, stack, conventions, notes,
                              initialized_at, last_seen,
                              github_repo, github_login, github_sync_scope)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7,?8,?9,?10)
         ON CONFLICT(name) DO UPDATE SET
           path          = COALESCE(?2, path),
           goal          = COALESCE(?3, goal),
           stack         = COALESCE(?4, stack),
           conventions   = COALESCE(?5, conventions),
           notes         = COALESCE(?6, notes),
           initialized_at = COALESCE(?7, initialized_at),
           last_seen      = ?7,
           github_repo    = COALESCE(?8, github_repo),
           github_login   = COALESCE(?9, github_login),
           github_sync_scope = COALESCE(?10, github_sync_scope)",
        params![
            project.name,
            project.path,
            project.goal,
            project.stack,
            project.conventions,
            project.notes,
            now,
            project.github_repo,
            project.github_login,
            project.github_sync_scope,
        ],
    )?;
    Ok(())
}

pub fn get_project(conn: &Connection, name: &str) -> Result<Option<Project>> {
    let mut stmt = conn.prepare(
        "SELECT name,path,goal,stack,conventions,notes,initialized_at,last_seen,
                github_repo,github_login,github_sync_scope
         FROM projects WHERE name=?1",
    )?;
    let mut rows = stmt.query_map([name], |row| {
        Ok(Project {
            name: row.get(0)?,
            path: row.get(1)?,
            goal: row.get(2)?,
            stack: row.get(3)?,
            conventions: row.get(4)?,
            notes: row.get(5)?,
            initialized_at: row
                .get::<_, Option<String>>(6)?
                .and_then(|s| str_to_dt(&s).ok()),
            last_seen: row
                .get::<_, Option<String>>(7)?
                .and_then(|s| str_to_dt(&s).ok()),
            github_repo: row.get(8)?,
            github_login: row.get(9)?,
            github_sync_scope: row.get(10)?,
        })
    })?;
    Ok(rows.next().transpose()?)
}

/// All known project names — the union of registered profiles and any project
/// referenced by a task — sorted. Used for shell-completion candidates.
pub fn project_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT project FROM tasks
         UNION
         SELECT name FROM projects
         ORDER BY 1",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Look up a project profile by its (canonical) path. When several profiles
/// share a path — e.g. stale rows from before path resolution was fixed — the
/// most-recently-seen one wins.
pub fn get_project_by_path(conn: &Connection, path: &str) -> Result<Option<Project>> {
    let mut stmt = conn.prepare(
        "SELECT name,path,goal,stack,conventions,notes,initialized_at,last_seen,
                github_repo,github_login,github_sync_scope
         FROM projects WHERE path=?1 ORDER BY last_seen DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([path], |row| {
        Ok(Project {
            name: row.get(0)?,
            path: row.get(1)?,
            goal: row.get(2)?,
            stack: row.get(3)?,
            conventions: row.get(4)?,
            notes: row.get(5)?,
            initialized_at: row
                .get::<_, Option<String>>(6)?
                .and_then(|s| str_to_dt(&s).ok()),
            last_seen: row
                .get::<_, Option<String>>(7)?
                .and_then(|s| str_to_dt(&s).ok()),
            github_repo: row.get(8)?,
            github_login: row.get(9)?,
            github_sync_scope: row.get(10)?,
        })
    })?;
    Ok(rows.next().transpose()?)
}

/// How many tasks a project currently owns (any status).
pub fn count_project_tasks(conn: &Connection, name: &str) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(n as usize)
}

/// The most recent task-modification time across a project's tasks, if any.
/// Used by the project browser to show each project's last activity.
pub fn project_last_activity(conn: &Connection, name: &str) -> Result<Option<DateTime<Utc>>> {
    let raw: Option<String> = conn.query_row(
        "SELECT MAX(modified) FROM tasks WHERE project=?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(raw.and_then(|s| str_to_dt(&s).ok()))
}

/// Nuke a project: delete all of its tasks (cascading to their dependencies,
/// files, links, annotations and history), purge undo-log rows for those tasks,
/// and remove the project profile itself. Returns the number of tasks deleted.
pub fn reset_project(conn: &mut Connection, name: &str) -> Result<usize> {
    let tx = conn.transaction()?;

    // Collect the task uuids first so we can clean the undo log (no FK cascade).
    let uuids: Vec<String> = {
        let mut stmt = tx.prepare("SELECT uuid FROM tasks WHERE project=?1")?;
        let rows = stmt.query_map([name], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    for uuid in &uuids {
        tx.execute("DELETE FROM undo_log WHERE task_uuid=?1", [uuid])?;
    }

    // Cascades remove dependencies, task_files, annotations, task_history,
    // and task_links for each deleted task.
    let deleted = tx.execute("DELETE FROM tasks WHERE project=?1", [name])?;
    tx.execute("DELETE FROM projects WHERE name=?1", [name])?;

    tx.commit()?;
    Ok(deleted)
}

// ── urgency ──────────────────────────────────────────────────────────────────

pub fn compute_urgency(
    task: &Task,
    cfg: &crate::infrastructure::config::UrgencyConfig,
    is_blocked: bool,
    blocking_count: usize,
) -> f64 {
    let mut score = 0.0;

    if let Some(ref p) = task.priority {
        score += p.urgency_coefficient();
    }

    if let Some(due) = task.due {
        let days_until: f64 = (due - Utc::now()).num_seconds() as f64 / 86400.0;
        // ramp: overdue -> 1.0, due in 7+ days -> 0.0
        let factor = if days_until <= 0.0 {
            1.0
        } else if days_until >= 7.0 {
            0.0
        } else {
            1.0 - (days_until / 7.0)
        };
        score += cfg.due * factor;
    }

    if blocking_count > 0 {
        score += cfg.blocking;
    }
    if is_blocked {
        score += cfg.blocked;
    }
    if task.is_active() {
        score += cfg.active;
    }
    if !task.tags.is_empty() {
        score += cfg.has_tags;
    }
    if task.project != "inbox" {
        score += cfg.project;
    }

    let age_days = (Utc::now() - task.entry).num_days() as f64;
    let age_factor = (age_days / cfg.age_max).min(1.0);
    score += cfg.age * age_factor;

    score
}

pub fn refresh_urgency(
    conn: &Connection,
    cfg: &crate::infrastructure::config::UrgencyConfig,
    task_uuid: &Uuid,
) -> Result<()> {
    let task = {
        let mut stmt = conn.prepare(
            "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
             FROM tasks WHERE uuid=?1",
        )?;
        let mut rows = stmt.query_map([task_uuid.to_string()], row_to_task)?;
        rows.next()
            .ok_or_else(|| anyhow::anyhow!("Task not found"))??
    };
    let blockers = get_blockers(conn, task_uuid)?;
    let blocking = get_blocking(conn, task_uuid)?;
    let urgency = compute_urgency(&task, cfg, !blockers.is_empty(), blocking.len());
    conn.execute(
        "UPDATE tasks SET urgency=?1 WHERE uuid=?2",
        params![urgency, task_uuid.to_string()],
    )?;
    Ok(())
}

// ── urgency breakdown ─────────────────────────────────────────────────────────

pub struct UrgencyBreakdown {
    pub priority: f64,
    pub due: f64,
    pub blocking: f64,
    pub blocked: f64,
    pub active: f64,
    pub tags: f64,
    pub project: f64,
    pub age: f64,
}

pub fn compute_urgency_breakdown(
    task: &Task,
    cfg: &crate::infrastructure::config::UrgencyConfig,
    is_blocked: bool,
    blocking_count: usize,
) -> UrgencyBreakdown {
    let priority = task
        .priority
        .as_ref()
        .map(|p| p.urgency_coefficient())
        .unwrap_or(0.0);

    let due = if let Some(due) = task.due {
        let days_until: f64 = (due - Utc::now()).num_seconds() as f64 / 86400.0;
        let factor = if days_until <= 0.0 {
            1.0
        } else if days_until >= 7.0 {
            0.0
        } else {
            1.0 - (days_until / 7.0)
        };
        cfg.due * factor
    } else {
        0.0
    };

    let blocking = if blocking_count > 0 {
        cfg.blocking
    } else {
        0.0
    };
    let blocked = if is_blocked { cfg.blocked } else { 0.0 };
    let active = if task.is_active() { cfg.active } else { 0.0 };
    let tags = if !task.tags.is_empty() {
        cfg.has_tags
    } else {
        0.0
    };
    let project = if task.project != "inbox" {
        cfg.project
    } else {
        0.0
    };
    let age_days = (Utc::now() - task.entry).num_days() as f64;
    let age = cfg.age * (age_days / cfg.age_max).min(1.0);

    UrgencyBreakdown {
        priority,
        due,
        blocking,
        blocked,
        active,
        tags,
        project,
        age,
    }
}

// ── similar tasks ─────────────────────────────────────────────────────────────

/// Tasks in the same project sharing at least one tag, excluding the task itself.
pub fn similar_tasks(
    conn: &Connection,
    task_uuid: &Uuid,
    project: &str,
    tags: &[String],
) -> Result<Vec<(i64, String, f64)>> {
    if tags.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE status='pending' AND project=?1 AND uuid!=?2"
    ))?;
    let all: Vec<Task> = stmt
        .query_map(
            rusqlite::params![project, task_uuid.to_string()],
            row_to_task,
        )?
        .filter_map(|r| r.ok())
        .collect();

    let result = all
        .into_iter()
        .filter(|t| t.tags.iter().any(|tag| tags.contains(tag)))
        .filter_map(|t| t.id.map(|id| (id, t.description.clone(), t.urgency)))
        .collect();
    Ok(result)
}
