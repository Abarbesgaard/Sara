use super::*;
use crate::infrastructure::model::Task;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Link {
    pub id: i64,
    pub url: String,
    pub label: Option<String>,
    pub entry: DateTime<Utc>,
}

impl Link {
    pub fn display(&self) -> String {
        self.label
            .clone()
            .or_else(|| derive_link_label(&self.url))
            .unwrap_or_else(|| self.url.clone())
    }
}

pub fn is_url(s: &str) -> bool {
    let s = s.trim();
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.contains("://")
        || s.starts_with("www.")
}

pub fn derive_link_label(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("github.com/"))?;
    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 4 {
        let owner = parts[0];
        let repo = parts[1];
        let kind = parts[2];
        let num = parts[3]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap_or("");
        let tag = match kind {
            "pull" => Some("PR"),
            "issues" => Some("Issue"),
            _ => None,
        };
        if let (Some(tag), false) = (tag, num.is_empty()) {
            return Some(format!("{tag} #{num} · {owner}/{repo}"));
        }
    }
    None
}

pub fn is_issue_link(url: &str) -> bool {
    derive_link_label(url)
        .map(|l| l.starts_with("Issue "))
        .unwrap_or(false)
}

pub fn is_pr_link(url: &str) -> bool {
    derive_link_label(url)
        .map(|l| l.starts_with("PR "))
        .unwrap_or(false)
}

pub fn parse_issue_link(url: &str) -> Option<(String, u64)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("github.com/"))?;
    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 4 || parts[2] != "issues" {
        return None;
    }
    let num_str = parts[3]
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("");
    let number: u64 = num_str.parse().ok()?;
    Some((format!("{}/{}", parts[0], parts[1]), number))
}

#[derive(Debug, Clone)]
pub struct IssueGroup {
    pub owner_repo: String,
    pub number: u64,
    pub tasks: Vec<Task>,
}

pub fn group_tasks_by_issue(
    conn: &Connection,
    tasks: &[Task],
) -> Result<(Vec<IssueGroup>, Vec<Task>)> {
    let mut groups: std::collections::BTreeMap<(String, u64), Vec<Task>> =
        std::collections::BTreeMap::new();
    let mut ungrouped = Vec::new();

    for task in tasks {
        let links = get_links(conn, &task.uuid)?;
        match links.iter().find_map(|l| parse_issue_link(&l.url)) {
            Some(key) => groups.entry(key).or_default().push(task.clone()),
            None => ungrouped.push(task.clone()),
        }
    }

    let groups = groups
        .into_iter()
        .map(|((owner_repo, number), tasks)| IssueGroup {
            owner_repo,
            number,
            tasks,
        })
        .collect();
    Ok((groups, ungrouped))
}

pub fn add_link(conn: &Connection, task_uuid: &Uuid, url: &str, label: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO task_links (task_uuid, url, label, entry) VALUES (?1,?2,?3,?4)",
        params![task_uuid.to_string(), url, label, dt_to_str(&Utc::now())],
    )?;
    let display = label
        .map(|s| s.to_string())
        .or_else(|| derive_link_label(url))
        .unwrap_or_else(|| url.to_string());
    record_history(conn, task_uuid, "link", None, Some(&display))?;
    Ok(())
}

pub fn get_links(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<Link>> {
    let mut stmt = conn.prepare(
        "SELECT id, url, label, entry FROM task_links WHERE task_uuid=?1 ORDER BY entry ASC",
    )?;
    let links = stmt
        .query_map([task_uuid.to_string()], |row| {
            let entry_str: String = row.get(3)?;
            Ok(Link {
                id: row.get(0)?,
                url: row.get(1)?,
                label: row.get(2)?,
                entry: str_to_dt(&entry_str).unwrap_or_else(|_| Utc::now()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(links)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LinkFlags {
    pub any: bool,
    pub pr: bool,
    pub issue: bool,
}

pub fn link_flags_by_task(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, LinkFlags>> {
    let mut stmt = conn.prepare("SELECT task_uuid, url FROM task_links")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map: std::collections::HashMap<String, LinkFlags> = std::collections::HashMap::new();
    for row in rows {
        let (uuid, url) = row?;
        let is_pr = is_pr_link(&url);
        let is_issue = is_issue_link(&url);
        let entry = map.entry(uuid).or_default();
        entry.any = true;
        entry.pr = entry.pr || is_pr;
        entry.issue = entry.issue || is_issue;
    }
    Ok(map)
}

pub fn github_synced_task_uuids(conn: &Connection) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT uuid FROM tasks
         WHERE meta_json IS NOT NULL
           AND json_extract(meta_json, '$.github') IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut set = std::collections::HashSet::new();
    for row in rows {
        set.insert(row?);
    }
    Ok(set)
}

pub fn github_issue_titles_for_project(
    conn: &Connection,
    project: &str,
) -> Result<std::collections::HashMap<String, String>> {
    let mut stmt = conn.prepare(
        "SELECT uuid, json_extract(meta_json, '$.github.title')
         FROM tasks
         WHERE project = ?1 AND json_extract(meta_json, '$.github.title') IS NOT NULL",
    )?;
    let rows = stmt.query_map([project], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (uuid, title) = row?;
        map.insert(uuid, title);
    }
    Ok(map)
}

pub fn delete_link(conn: &Connection, link_id: i64) -> Result<bool> {
    let existing: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT task_uuid, url, label FROM task_links WHERE id=?1",
            [link_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .ok();

    let n = conn.execute("DELETE FROM task_links WHERE id=?1", [link_id])?;
    if n > 0
        && let Some((uuid_str, url, label)) = existing
        && let Ok(uuid) = Uuid::parse_str(&uuid_str)
    {
        let display = label.or_else(|| derive_link_label(&url)).unwrap_or(url);
        record_history(conn, &uuid, "link", Some(&display), None)?;
    }
    Ok(n > 0)
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub changed_at: DateTime<Utc>,
}

pub fn get_history(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT field, old_value, new_value, changed_at
         FROM task_history WHERE task_uuid=?1 ORDER BY changed_at ASC, id ASC",
    )?;
    let rows = stmt
        .query_map([task_uuid.to_string()], |row| {
            let changed_str: String = row.get(3)?;
            Ok(HistoryEntry {
                field: row.get(0)?,
                old_value: row.get(1)?,
                new_value: row.get(2)?,
                changed_at: str_to_dt(&changed_str).unwrap_or_else(|_| Utc::now()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}
