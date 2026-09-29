use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    H,
    M,
    L,
}

impl Priority {
    pub fn urgency_coefficient(&self) -> f64 {
        match self {
            Priority::H => 6.0,
            Priority::M => 3.9,
            Priority::L => 1.8,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Priority::H => "H",
            Priority::M => "M",
            Priority::L => "L",
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

impl std::str::FromStr for Priority {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "H" | "HIGH" => Ok(Priority::H),
            "M" | "MED" | "MEDIUM" => Ok(Priority::M),
            "L" | "LOW" => Ok(Priority::L),
            _ => Err(anyhow::anyhow!("Unknown priority: {s}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Completed,
    Deleted,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Status::Pending => "pending",
            Status::Completed => "completed",
            Status::Deleted => "deleted",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub uuid: Uuid,
    pub id: Option<i64>,
    pub description: String,
    pub project: String,
    pub status: Status,
    pub priority: Option<Priority>,
    pub due: Option<DateTime<Utc>>,
    pub entry: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
    pub urgency: f64,
    pub started_at: Option<DateTime<Utc>>,
    pub time_spent: i64,
    pub estimate_mins: Option<i64>,
    pub recur: Option<String>,
}

impl Task {
    pub fn is_active(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn total_time_spent(&self) -> i64 {
        let live = self
            .started_at
            .map(|s| (Utc::now() - s).num_seconds().max(0))
            .unwrap_or(0);
        self.time_spent + live
    }

    pub fn next_due(&self, base: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let interval = self.recur.as_deref()?;
        Some(advance_by_interval(base, interval))
    }
}

pub fn advance_by_interval(base: DateTime<Utc>, interval: &str) -> DateTime<Utc> {
    let s = interval.trim().to_lowercase();
    if s == "daily" {
        return base + chrono::Duration::days(1);
    }
    if s == "weekly" {
        return base + chrono::Duration::weeks(1);
    }
    if s == "monthly" {
        return add_months(base, 1);
    }
    if s == "yearly" {
        return add_months(base, 12);
    }
    if let Some(stripped) = s.strip_suffix('d')
        && let Ok(n) = stripped.parse::<i64>()
    {
        return base + chrono::Duration::days(n);
    }
    if let Some(stripped) = s.strip_suffix('w')
        && let Ok(n) = stripped.parse::<i64>()
    {
        return base + chrono::Duration::weeks(n);
    }
    if let Some(stripped) = s.strip_suffix('m')
        && let Ok(n) = stripped.parse::<i64>()
    {
        return add_months(base, n as u32);
    }
    base + chrono::Duration::weeks(1)
}

fn add_months(dt: DateTime<Utc>, months: u32) -> DateTime<Utc> {
    use chrono::Datelike;
    let total_month = dt.month0() + months;
    let extra_years = total_month / 12;
    let new_month = (total_month % 12) + 1;
    let new_year = dt.year() + extra_years as i32;
    let max_day = days_in_month(new_year, new_month);
    let new_day = dt.day().min(max_day);
    dt.with_day(1)
        .and_then(|d| d.with_year(new_year))
        .and_then(|d| d.with_month(new_month))
        .and_then(|d| d.with_day(new_day))
        .unwrap_or(dt)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

pub fn format_duration(secs: i64) -> String {
    if secs <= 0 {
        return "0m".to_string();
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

impl Task {
    pub fn new(description: String, project: String) -> Self {
        let now = Utc::now();
        Task {
            uuid: Uuid::new_v4(),
            id: None,
            description,
            project,
            status: Status::Pending,
            priority: None,
            due: None,
            entry: now,
            modified: now,
            end: None,
            tags: vec![],
            urgency: 0.0,
            started_at: None,
            time_spent: 0,
            estimate_mins: None,
            recur: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub path: Option<String>,
    pub goal: Option<String>,
    pub stack: Option<String>,
    pub conventions: Option<String>,
    pub notes: Option<String>,
    pub initialized_at: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub github_repo: Option<String>,
    pub github_login: Option<String>,
    pub github_sync_scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubProvenance {
    pub repo: String,
    #[serde(default)]
    pub issue_id: Option<i64>,
    #[serde(default)]
    pub node_id: Option<String>,
    pub number: i64,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub creator: Option<String>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(alias = "imported_at")]
    pub synced_at: DateTime<Utc>,
    #[serde(alias = "imported_by")]
    pub synced_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubComment {
    pub comment_id: i64,
    pub author: String,
    pub body: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RelevantFile {
    pub path: String,
    pub reason: Option<String>,
    pub symbol: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub uuid: Uuid,
    pub display_id: Option<i64>,
    pub kind: String,
    pub title: String,
    pub url: Option<String>,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub path: Option<String>,
    pub summary: Option<String>,
    pub body: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub status: String,
    pub source_task_uuid: Option<Uuid>,
    pub files: Vec<String>,
    pub linked_tasks: Vec<(String, String, String)>,
}

impl Item {
    pub fn new_note(title: String, body: String) -> Self {
        let now = Utc::now();
        Item {
            uuid: Uuid::new_v4(),
            display_id: None,
            kind: "note".to_string(),
            title,
            url: None,
            project: None,
            tags: vec![],
            path: None,
            summary: None,
            body,
            created: now,
            modified: now,
            status: "active".to_string(),
            source_task_uuid: None,
            files: vec![],
            linked_tasks: vec![],
        }
    }

    pub fn new_memory(title: String, body: String, source_task_uuid: Option<Uuid>) -> Self {
        let now = Utc::now();
        Item {
            uuid: Uuid::new_v4(),
            display_id: None,
            kind: "memory".to_string(),
            title,
            url: None,
            project: None,
            tags: vec![],
            path: None,
            summary: None,
            body,
            created: now,
            modified: now,
            status: "active".to_string(),
            source_task_uuid,
            files: vec![],
            linked_tasks: vec![],
        }
    }

    pub fn new_link(url: String, title: String, body: String) -> Self {
        let now = Utc::now();
        Item {
            uuid: Uuid::new_v4(),
            display_id: None,
            kind: "link".to_string(),
            title,
            url: Some(url),
            project: None,
            tags: vec![],
            path: None,
            summary: None,
            body,
            created: now,
            modified: now,
            status: "active".to_string(),
            source_task_uuid: None,
            files: vec![],
            linked_tasks: vec![],
        }
    }

    pub fn handle(&self) -> String {
        let prefix = match self.kind.as_str() {
            "link" => "l",
            _ => "n",
        };
        format!("{}{}", prefix, self.display_id.unwrap_or(0))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/infrastructure/model.rs"]
mod tests;
