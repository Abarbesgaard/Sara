use super::*;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::collections::HashMap;
use uuid::Uuid;

pub const DOING_MAX_CHARS: usize = 200;
pub const DOING_KEEP_PER_TASK: i64 = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityEntry {
    pub id: i64,
    pub task_uuid: Uuid,
    pub text: String,
    pub client: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowKind {
    Doing,
    StepDone,
    StepAdded,
    Note(String),
    Memory(String),
    Change(String),
    AiRun,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlowEvent {
    pub at: DateTime<Utc>,
    pub kind: FlowKind,
    pub text: String,
    pub detail: Option<String>,
}

fn parse_at(s: &str) -> DateTime<Utc> {
    str_to_dt(s)
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .map(|n| n.and_utc())
                .map_err(anyhow::Error::from)
        })
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn row_to_activity(r: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityEntry> {
    let task: String = r.get(1)?;
    let at: String = r.get(4)?;
    Ok(ActivityEntry {
        id: r.get(0)?,
        task_uuid: Uuid::parse_str(&task).unwrap_or_default(),
        text: r.get(2)?,
        client: r.get(3)?,
        at: parse_at(&at),
    })
}

pub fn record_doing(
    conn: &Connection,
    task_uuid: &Uuid,
    text: &str,
    client: Option<&str>,
) -> Result<ActivityEntry> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        anyhow::bail!("`doing` text cannot be empty");
    }
    let text: String = text.chars().take(DOING_MAX_CHARS).collect();
    let client = client.map(str::trim).filter(|c| !c.is_empty());
    let at = Utc::now();
    conn.execute(
        "INSERT INTO task_activity (task_uuid, text, client, at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![task_uuid.to_string(), text, client, dt_to_str(&at)],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM task_activity WHERE task_uuid = ?1 AND id NOT IN \
         (SELECT id FROM task_activity WHERE task_uuid = ?1 ORDER BY id DESC LIMIT ?2)",
        rusqlite::params![task_uuid.to_string(), DOING_KEEP_PER_TASK],
    )?;
    Ok(ActivityEntry {
        id,
        task_uuid: *task_uuid,
        text,
        client: client.map(str::to_owned),
        at,
    })
}

pub fn doing_for_task(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<ActivityEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_uuid, text, client, at FROM task_activity
         WHERE task_uuid = ?1 ORDER BY id",
    )?;
    let rows = stmt
        .query_map([task_uuid.to_string()], row_to_activity)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn latest_doing(conn: &Connection, task_uuid: &Uuid) -> Result<Option<ActivityEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_uuid, text, client, at FROM task_activity
         WHERE task_uuid = ?1 ORDER BY id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([task_uuid.to_string()], row_to_activity)?;
    Ok(rows.next().transpose()?)
}

pub fn data_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA data_version", [], |r| r.get(0))?)
}

const EVENT_TIMES: &str = "
    SELECT task_uuid, at FROM task_activity
    UNION ALL SELECT task_uuid, done_at FROM task_checklist WHERE done_at IS NOT NULL
    UNION ALL SELECT task_uuid, entry FROM annotations
    UNION ALL SELECT task_uuid, changed_at FROM task_history
    UNION ALL SELECT task_uuid, at FROM memory_uses
    UNION ALL SELECT task_uuid, created_at FROM task_ai_runs";

pub fn last_event_times(conn: &Connection) -> Result<HashMap<Uuid, DateTime<Utc>>> {
    let mut stmt = conn.prepare(&format!("SELECT task_uuid, at FROM ({EVENT_TIMES})"))?;
    let mut out: HashMap<Uuid, DateTime<Utc>> = HashMap::new();
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for (task, at) in rows.flatten() {
        let Ok(uuid) = Uuid::parse_str(&task) else {
            continue;
        };
        let at = parse_at(&at);
        out.entry(uuid)
            .and_modify(|cur| {
                if at > *cur {
                    *cur = at;
                }
            })
            .or_insert(at);
    }
    Ok(out)
}

pub fn open_tasks_by_activity(
    conn: &Connection,
    project: Option<&str>,
) -> Result<Vec<(Task, DateTime<Utc>)>> {
    let last = last_event_times(conn)?;
    let mut out: Vec<(Task, DateTime<Utc>)> = list_tasks(conn, project)?
        .into_iter()
        .map(|t| {
            let at = last.get(&t.uuid).copied().unwrap_or(t.modified);
            (t, at)
        })
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.1));
    Ok(out)
}

fn change_text(h: &HistoryEntry) -> String {
    match (&h.old_value, &h.new_value) {
        (Some(o), Some(n)) => format!("{}: {o} → {n}", h.field),
        (None, Some(n)) => format!("{}: {n}", h.field),
        (Some(o), None) => format!("{}: removed {o}", h.field),
        (None, None) => h.field.clone(),
    }
}

pub fn flow_events(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<FlowEvent>> {
    let mut events = Vec::new();
    for a in doing_for_task(conn, task_uuid)? {
        events.push(FlowEvent {
            at: a.at,
            kind: FlowKind::Doing,
            text: a.text,
            detail: a.client,
        });
    }
    for s in get_checklist(conn, task_uuid)? {
        if let Some(done_at) = s.done_at.as_deref().filter(|_| s.done) {
            events.push(FlowEvent {
                at: parse_at(done_at),
                kind: FlowKind::StepDone,
                text: s.text,
                detail: s.result,
            });
        }
    }
    for n in get_annotations(conn, task_uuid)? {
        events.push(FlowEvent {
            at: n.entry,
            kind: FlowKind::Note(n.kind),
            text: n.text,
            detail: Some(n.author),
        });
    }
    for h in get_history(conn, task_uuid)? {
        let dup = h.field == "annotation"
            || (h.field == "checklist" && h.new_value.as_deref() == Some("step done"));
        if dup {
            continue;
        }
        let kind = if matches!(h.field.as_str(), "checklist" | "acceptance")
            && h.old_value.is_none()
            && h.new_value
                .as_deref()
                .is_some_and(|v| v != "step reopened" && v != "step reordered")
        {
            FlowKind::StepAdded
        } else {
            FlowKind::Change(h.field.clone())
        };
        let text = if kind == FlowKind::StepAdded {
            h.new_value.clone().unwrap_or_default()
        } else {
            change_text(&h)
        };
        events.push(FlowEvent {
            at: h.changed_at,
            kind,
            text,
            detail: None,
        });
    }
    for u in memory_uses_for_task(conn, task_uuid)? {
        let label = get_item_by_uuid(conn, &u.item_uuid.to_string())
            .ok()
            .and_then(|i| i.display_id)
            .map(|n| format!("m{n}"))
            .unwrap_or_else(|| u.item_uuid.to_string()[..8].to_string());
        events.push(FlowEvent {
            at: u.at,
            kind: FlowKind::Memory(u.kind.as_str().to_string()),
            text: label,
            detail: None,
        });
    }
    for r in get_ai_runs(conn, task_uuid)? {
        events.push(FlowEvent {
            at: r.created_at,
            kind: FlowKind::AiRun,
            text: r.kind,
            detail: r.model,
        });
    }
    events.sort_by_key(|e| e.at);
    Ok(events)
}
