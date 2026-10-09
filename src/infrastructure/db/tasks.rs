use super::*;
use crate::infrastructure::model::Task;
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

struct UndoCtx {
    batch_id: String,
    command: String,
}

thread_local! {
    static UNDO_CTX: std::cell::RefCell<Option<UndoCtx>> = const { std::cell::RefCell::new(None) };
}

pub fn begin_undo_batch(command: &str) {
    UNDO_CTX.with(|c| {
        *c.borrow_mut() = Some(UndoCtx {
            batch_id: Uuid::new_v4().to_string(),
            command: command.to_string(),
        });
    });
}

fn log_undo(
    conn: &Connection,
    task_uuid: &Uuid,
    before: Option<&Task>,
    after: Option<&Task>,
) -> Result<()> {
    let entry = UNDO_CTX.with(|c| {
        c.borrow()
            .as_ref()
            .map(|ctx| (ctx.batch_id.clone(), ctx.command.clone()))
    });
    let Some((batch_id, command)) = entry else {
        return Ok(());
    };
    let before_json = before.map(serde_json::to_string).transpose()?;
    let after_json = after.map(serde_json::to_string).transpose()?;
    conn.execute(
        "INSERT INTO undo_log (batch_id, command, task_uuid, before_json, after_json, created_at)
         VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            batch_id,
            command,
            task_uuid.to_string(),
            before_json,
            after_json,
            dt_to_str(&Utc::now()),
        ],
    )?;
    Ok(())
}

fn restore_task_row(conn: &Connection, t: &Task) -> Result<()> {
    let n = conn.execute(
        "UPDATE tasks SET id=?1, description=?2, project=?3, status=?4, priority=?5, due=?6,
                          entry=?7, modified=?8, end=?9, tags_json=?10, urgency=?11,
                          started_at=?12, time_spent=?13
         WHERE uuid=?14",
        params![
            t.id,
            t.description,
            t.project,
            t.status.to_string(),
            t.priority.as_ref().map(|p| p.label()),
            t.due.as_ref().map(dt_to_str),
            dt_to_str(&t.entry),
            dt_to_str(&t.modified),
            t.end.as_ref().map(dt_to_str),
            serde_json::to_string(&t.tags).unwrap_or_else(|_| "[]".into()),
            t.urgency,
            t.started_at.as_ref().map(dt_to_str),
            t.time_spent,
            t.uuid.to_string(),
        ],
    )?;
    if n == 0 {
        conn.execute(
            "INSERT INTO tasks (uuid, id, description, project, status, priority, due,
                                entry, modified, end, tags_json, urgency, started_at, time_spent,
                                estimate_mins, recur)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                t.uuid.to_string(),
                t.id,
                t.description,
                t.project,
                t.status.to_string(),
                t.priority.as_ref().map(|p| p.label()),
                t.due.as_ref().map(dt_to_str),
                dt_to_str(&t.entry),
                dt_to_str(&t.modified),
                t.end.as_ref().map(dt_to_str),
                serde_json::to_string(&t.tags).unwrap_or_else(|_| "[]".into()),
                t.urgency,
                t.started_at.as_ref().map(dt_to_str),
                t.time_spent,
                t.estimate_mins,
                t.recur,
            ],
        )?;
    }
    Ok(())
}

pub fn undo(conn: &Connection) -> Result<Option<String>> {
    let latest: Option<(String, String)> = conn
        .query_row(
            "SELECT batch_id, command FROM undo_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((batch_id, command)) = latest else {
        return Ok(None);
    };

    let entries: Vec<(Option<String>, String)> = {
        let mut stmt = conn.prepare(
            "SELECT before_json, task_uuid FROM undo_log WHERE batch_id=?1 ORDER BY id DESC",
        )?;
        stmt.query_map([&batch_id], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (before_json, task_uuid) in entries {
        match before_json {
            Some(json) => {
                let task: Task =
                    serde_json::from_str(&json).context("Failed to decode undo snapshot")?;
                restore_task_row(conn, &task)?;
            }
            None => {
                conn.execute("DELETE FROM tasks WHERE uuid=?1", [&task_uuid])?;
            }
        }
    }

    conn.execute("DELETE FROM undo_log WHERE batch_id=?1", [&batch_id])?;
    repack_ids(conn)?;
    Ok(Some(command))
}

pub fn next_display_id(conn: &Connection) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT id FROM tasks WHERE status='pending' AND id IS NOT NULL ORDER BY id ASC",
    )?;
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    let mut next = 1i64;
    for id in ids {
        if id == next {
            next += 1;
        } else {
            break;
        }
    }
    Ok(next)
}

pub fn insert_task(conn: &Connection, task: &mut Task) -> Result<()> {
    with_write_lock(conn, || {
        let id = next_display_id(conn)?;
        task.id = Some(id);
        conn.execute(
            "INSERT INTO tasks (uuid, id, description, project, status, priority, due,
                                entry, modified, end, tags_json, urgency, started_at, time_spent,
                                estimate_mins, recur)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                task.uuid.to_string(),
                task.id,
                task.description,
                task.project,
                task.status.to_string(),
                task.priority.as_ref().map(|p| p.label()),
                task.due.as_ref().map(dt_to_str),
                dt_to_str(&task.entry),
                dt_to_str(&task.modified),
                task.end.as_ref().map(dt_to_str),
                serde_json::to_string(&task.tags).unwrap_or_else(|_| "[]".into()),
                task.urgency,
                task.started_at.as_ref().map(dt_to_str),
                task.time_spent,
                task.estimate_mins,
                task.recur,
            ],
        )?;
        conn.execute(
            "INSERT INTO task_history (task_uuid, field, old_value, new_value, changed_at)
             VALUES (?1, 'created', NULL, ?2, ?3)",
            params![
                task.uuid.to_string(),
                task.description,
                dt_to_str(&task.entry),
            ],
        )?;
        log_undo(conn, &task.uuid, None, Some(task))?;
        Ok(())
    })
}

pub fn get_task_by_id(conn: &Connection, id: i64) -> Result<Option<Task>> {
    let mut stmt = conn.prepare(
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
         FROM tasks WHERE id=?1 AND status='pending' LIMIT 1",
    )?;
    let mut rows = stmt.query_map([id], row_to_task)?;
    Ok(rows.next().transpose()?)
}

pub(crate) fn like_prefix_pattern(prefix: &str) -> String {
    let escaped = prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("{escaped}%")
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Minimum fragment length (hyphen-free hex chars) accepted by `find`, so a
/// trivially short fragment cannot match nearly every task.
pub const MIN_UUID_FRAGMENT_LEN: usize = 4;

/// Status scope for [`find_tasks_by_uuid_fragment`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindStatus {
    Pending,
    Completed,
    /// Pending and completed (never deleted).
    All,
}

/// Search tasks whose uuid *contains* `fragment` as a substring, hyphens
/// optional: both the fragment and each stored uuid are lowercased and have
/// their hyphens stripped before matching, so `4c449b0e` matches
/// `…-4c44-9b0e-…`. LIKE wildcards (`%`, `_`, `\`) in the fragment are escaped
/// and matched literally. Results are sorted pending-first then most recently
/// modified, capped at `limit`.
///
/// Returns `Err` if `fragment` (after stripping hyphens) is shorter than
/// [`MIN_UUID_FRAGMENT_LEN`]. A read-only search: unlike [`resolve_task`] it
/// never resolves to a single task, so it cannot target a mutation.
pub fn find_tasks_by_uuid_fragment(
    conn: &Connection,
    fragment: &str,
    project: Option<&str>,
    status: FindStatus,
    limit: usize,
) -> Result<Vec<Task>> {
    let normalized: String = fragment
        .chars()
        .filter(|c| *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect();
    if normalized.len() < MIN_UUID_FRAGMENT_LEN {
        return Err(anyhow::anyhow!(
            "uuid fragment '{fragment}' is too short — give at least {MIN_UUID_FRAGMENT_LEN} characters (hyphens don't count)"
        ));
    }
    let pattern = format!("%{}%", like_escape(&normalized));

    let status_clause = match status {
        FindStatus::Pending => " AND status='pending'",
        FindStatus::Completed => " AND status='completed'",
        FindStatus::All => " AND status IN ('pending','completed')",
    };
    let project_clause = if project.is_some() {
        " AND project=?2"
    } else {
        ""
    };
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM tasks
         WHERE REPLACE(LOWER(uuid),'-','') LIKE ?1 ESCAPE '\\'{status_clause}{project_clause}
         ORDER BY status='pending' DESC, modified DESC
         LIMIT {limit}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(p) = project {
        stmt.query_map(params![pattern, p], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map([pattern], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

pub fn get_task_by_uuid_prefix(conn: &Connection, prefix: &str) -> Result<Option<Task>> {
    let pattern = like_prefix_pattern(prefix);
    let mut stmt = conn.prepare(
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
         FROM tasks WHERE uuid LIKE ?1 ESCAPE '\\' ORDER BY status='pending' DESC, uuid LIMIT 6",
    )?;
    let mut tasks = stmt
        .query_map([pattern], row_to_task)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if tasks.len() > 1 {
        let candidates = tasks
            .iter()
            .take(5)
            .map(|t| {
                let short: String = t.uuid.to_string().chars().take(8).collect();
                format!("  {short} [{}] {}", t.status, t.description)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let more = if tasks.len() > 5 { "\n  …" } else { "" };
        return Err(anyhow::anyhow!(
            "Ambiguous task uuid prefix '{prefix}' matches multiple tasks — use a longer prefix:\n{candidates}{more}"
        ));
    }
    Ok(tasks.pop())
}

const MIN_NUMERIC_UUID_PREFIX: usize = 8;

pub fn resolve_task(conn: &Connection, id_or_uuid: &str) -> Result<Task> {
    if let Ok(n) = id_or_uuid.parse::<i64>() {
        if let Some(t) = get_task_by_id(conn, n)? {
            return Ok(t);
        }
        if id_or_uuid.len() < MIN_NUMERIC_UUID_PREFIX {
            return Err(anyhow::anyhow!(
                "No pending task with display id {n} — display ids are renumbered when tasks complete; target the task by its uuid prefix instead"
            ));
        }
    }
    if let Some(t) = get_task_by_uuid_prefix(conn, id_or_uuid)? {
        return Ok(t);
    }
    Err(anyhow::anyhow!(
        "No pending task with id or uuid matching '{id_or_uuid}'"
    ))
}

pub fn list_tasks(conn: &Connection, project: Option<&str>) -> Result<Vec<Task>> {
    let sql = if project.is_some() {
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
         FROM tasks WHERE status='pending' AND project=?1 ORDER BY urgency DESC"
    } else {
        "SELECT uuid,id,description,project,status,priority,due,entry,modified,end,tags_json,urgency,started_at,time_spent,estimate_mins,recur
         FROM tasks WHERE status='pending' ORDER BY urgency DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(p) = project {
        stmt.query_map([p], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map([], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

pub fn list_tasks_for_board(conn: &Connection, project: &str) -> Result<Vec<Task>> {
    let mut tasks = Vec::new();
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE project=?1 AND status='pending' ORDER BY urgency DESC"
    ))?;
    tasks.extend(
        stmt.query_map([project], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE project=?1 AND status='completed' ORDER BY end DESC"
    ))?;
    tasks.extend(
        stmt.query_map([project], row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    Ok(tasks)
}

pub fn update_task(conn: &Connection, task: &Task) -> Result<()> {
    let prev = get_task_by_uuid_prefix(conn, &task.uuid.to_string())?;
    conn.execute(
        "UPDATE tasks SET description=?1, project=?2, status=?3, priority=?4, due=?5,
                         modified=?6, end=?7, tags_json=?8, urgency=?9,
                         started_at=?10, time_spent=?11, estimate_mins=?12, recur=?13
         WHERE uuid=?14",
        params![
            task.description,
            task.project,
            task.status.to_string(),
            task.priority.as_ref().map(|p| p.label()),
            task.due.as_ref().map(dt_to_str),
            dt_to_str(&task.modified),
            task.end.as_ref().map(dt_to_str),
            serde_json::to_string(&task.tags).unwrap_or_else(|_| "[]".into()),
            task.urgency,
            task.started_at.as_ref().map(dt_to_str),
            task.time_spent,
            task.estimate_mins,
            task.recur,
            task.uuid.to_string(),
        ],
    )?;
    if let Some(prev) = prev {
        log_undo(conn, &task.uuid, Some(&prev), Some(task))?;
        record_changes(conn, &prev, task)?;
    }
    Ok(())
}

fn tracked_field_values(t: &Task) -> [(&'static str, Option<String>); 9] {
    [
        ("description", non_empty(&t.description)),
        ("project", non_empty(&t.project)),
        ("status", Some(t.status.to_string())),
        (
            "priority",
            t.priority.as_ref().map(|p| p.label().to_string()),
        ),
        (
            "due",
            t.due.map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d")
                    .to_string()
            }),
        ),
        (
            "tags",
            if t.tags.is_empty() {
                None
            } else {
                Some(t.tags.join(", "))
            },
        ),
        ("estimate", t.estimate_mins.map(fmt_estimate)),
        ("recur", t.recur.clone()),
        ("timer", t.started_at.map(|_| "running".to_string())),
    ]
}

fn fmt_estimate(mins: i64) -> String {
    if mins >= 60 {
        let h = mins / 60;
        let r = mins % 60;
        if r == 0 {
            format!("{h}h")
        } else {
            format!("{h}h{r}m")
        }
    } else {
        format!("{mins}m")
    }
}

fn non_empty(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

pub(crate) fn record_history(
    conn: &Connection,
    task_uuid: &Uuid,
    field: &str,
    old_value: Option<&str>,
    new_value: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO task_history (task_uuid, field, old_value, new_value, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            task_uuid.to_string(),
            field,
            old_value,
            new_value,
            dt_to_str(&Utc::now())
        ],
    )?;
    Ok(())
}

fn record_changes(conn: &Connection, old: &Task, new: &Task) -> Result<()> {
    let olds = tracked_field_values(old);
    let news = tracked_field_values(new);
    let at = dt_to_str(&new.modified);
    for ((field, old_val), (_, new_val)) in olds.into_iter().zip(news) {
        if old_val != new_val {
            conn.execute(
                "INSERT INTO task_history (task_uuid, field, old_value, new_value, changed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![new.uuid.to_string(), field, old_val, new_val, at],
            )?;
        }
    }
    Ok(())
}

pub fn repack_ids(conn: &Connection) -> Result<()> {
    with_write_lock(conn, || {
        let mut stmt =
            conn.prepare("SELECT uuid FROM tasks WHERE status='pending' ORDER BY entry ASC")?;
        let uuids: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        for (i, uuid) in uuids.iter().enumerate() {
            conn.execute(
                "UPDATE tasks SET id=?1 WHERE uuid=?2",
                params![i as i64 + 1, uuid],
            )?;
        }
        Ok(())
    })
}

pub fn add_dependency(conn: &Connection, task_uuid: &Uuid, dep_uuid: &Uuid) -> Result<()> {
    if task_uuid == dep_uuid {
        anyhow::bail!("A task cannot depend on itself");
    }
    if would_create_cycle(conn, task_uuid, dep_uuid)? {
        anyhow::bail!("Adding this dependency would create a cycle");
    }
    let n = conn.execute(
        "INSERT OR IGNORE INTO dependencies (task_uuid, depends_on_uuid) VALUES (?1,?2)",
        params![task_uuid.to_string(), dep_uuid.to_string()],
    )?;
    if n > 0 {
        let label = dep_label(conn, dep_uuid);
        record_history(conn, task_uuid, "dependency", None, Some(&label))?;
    }
    Ok(())
}

pub fn remove_dependency(conn: &Connection, task_uuid: &Uuid, dep_uuid: &Uuid) -> Result<()> {
    let n = conn.execute(
        "DELETE FROM dependencies WHERE task_uuid=?1 AND depends_on_uuid=?2",
        params![task_uuid.to_string(), dep_uuid.to_string()],
    )?;
    if n > 0 {
        let label = dep_label(conn, dep_uuid);
        record_history(conn, task_uuid, "dependency", Some(&label), None)?;
    }
    Ok(())
}

fn dep_label(conn: &Connection, dep_uuid: &Uuid) -> String {
    get_task_by_uuid_prefix(conn, &dep_uuid.to_string()[..8])
        .ok()
        .flatten()
        .map(|t| format!("[{}] {}", t.id.unwrap_or(0), t.description))
        .unwrap_or_else(|| dep_uuid.to_string())
}

fn would_create_cycle(conn: &Connection, task: &Uuid, new_dep: &Uuid) -> Result<bool> {
    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![new_dep.to_string()];
    while let Some(cur) = queue.pop() {
        if cur == task.to_string() {
            return Ok(true);
        }
        if !visited.insert(cur.clone()) {
            continue;
        }
        let mut stmt =
            conn.prepare("SELECT depends_on_uuid FROM dependencies WHERE task_uuid=?1")?;
        let deps: Vec<String> = stmt
            .query_map([&cur], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        queue.extend(deps);
    }
    Ok(false)
}

pub fn get_blockers(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Uuid>> {
    let mut stmt = conn.prepare(
        "SELECT d.depends_on_uuid FROM dependencies d
         JOIN tasks t ON t.uuid=d.depends_on_uuid
         WHERE d.task_uuid=?1 AND t.status='pending'",
    )?;
    let uuids = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(uuids)
}

pub fn get_blocking(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Uuid>> {
    let mut stmt = conn.prepare("SELECT task_uuid FROM dependencies WHERE depends_on_uuid=?1")?;
    let uuids = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(uuids)
}

pub fn get_dependency_uuids(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Uuid>> {
    let mut stmt = conn.prepare("SELECT depends_on_uuid FROM dependencies WHERE task_uuid=?1")?;
    let uuids = stmt
        .query_map([task_uuid.to_string()], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(uuids)
}

#[derive(Debug, Default, Clone)]
pub struct DepInfo {
    pub blocked_by: Vec<i64>,
    pub blocking: usize,
}

impl DepInfo {
    pub fn is_blocked(&self) -> bool {
        !self.blocked_by.is_empty()
    }
}

pub fn dep_info_by_task(conn: &Connection) -> Result<std::collections::HashMap<String, DepInfo>> {
    let mut map: std::collections::HashMap<String, DepInfo> = std::collections::HashMap::new();

    let mut stmt = conn.prepare(
        "SELECT d.task_uuid, b.id
         FROM dependencies d
         JOIN tasks b ON b.uuid = d.depends_on_uuid
         WHERE b.status='pending'",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
    })?;
    for row in rows {
        let (task_uuid, blocker_id) = row?;
        if let Some(id) = blocker_id {
            map.entry(task_uuid).or_default().blocked_by.push(id);
        }
    }

    let mut stmt = conn.prepare(
        "SELECT d.depends_on_uuid, COUNT(*)
         FROM dependencies d
         JOIN tasks t ON t.uuid = d.task_uuid
         WHERE t.status='pending'
         GROUP BY d.depends_on_uuid",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (dep_uuid, count) = row?;
        map.entry(dep_uuid).or_default().blocking = count as usize;
    }

    for info in map.values_mut() {
        info.blocked_by.sort_unstable();
    }
    Ok(map)
}
