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
    /// Stable surrogate key (never recycled)
    pub uuid: Uuid,
    /// Small sequential display ID (recycled on completion)
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
    /// Set while the task is actively being worked on (time tracking)
    pub started_at: Option<DateTime<Utc>>,
    /// Accumulated active time in seconds
    pub time_spent: i64,
    /// Optional time estimate in minutes
    pub estimate_mins: Option<i64>,
    /// Recurrence interval: "daily", "weekly", "2w", "1m", etc. None = no recurrence.
    pub recur: Option<String>,
}

impl Task {
    pub fn is_active(&self) -> bool {
        self.started_at.is_some()
    }

    /// Total time spent including the current active session.
    pub fn total_time_spent(&self) -> i64 {
        let live = self
            .started_at
            .map(|s| (Utc::now() - s).num_seconds().max(0))
            .unwrap_or(0);
        self.time_spent + live
    }

    /// Compute the next due date for a recurring task based on its recur string.
    /// Anchors from `base` (usually the current due date, or today if none).
    pub fn next_due(&self, base: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let interval = self.recur.as_deref()?;
        Some(advance_by_interval(base, interval))
    }
}

/// Advance a datetime by a recurrence interval string.
/// Supported: "daily"/"1d", "weekly"/"1w", "monthly"/"1m", "Nd", "Nw", "Nm".
pub fn advance_by_interval(base: DateTime<Utc>, interval: &str) -> DateTime<Utc> {
    let s = interval.trim().to_lowercase();
    // Named aliases
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
    // Numeric prefix: "Nd", "Nw", "Nm"
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
    // Fallback: +1 week
    base + chrono::Duration::weeks(1)
}

fn add_months(dt: DateTime<Utc>, months: u32) -> DateTime<Utc> {
    use chrono::Datelike;
    let total_month = dt.month0() + months;
    let extra_years = total_month / 12;
    let new_month = (total_month % 12) + 1;
    let new_year = dt.year() + extra_years as i32;
    // Clamp day to last day of target month
    let max_day = days_in_month(new_year, new_month);
    let new_day = dt.day().min(max_day);
    // Collapse the day to 1 first so intermediate `with_year`/`with_month`
    // steps can never land on an invalid date (e.g. Jan 31 -> Feb 31), which
    // would short-circuit the chain and leave the date unchanged.
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
    /// GitHub full repository name (e.g. "owner/repo"). Never a secret.
    pub github_repo: Option<String>,
    /// GitHub login (username) used when syncing. Never a PAT or token.
    pub github_login: Option<String>,
    /// Comma-separated sync scopes, e.g. "issues" or "issues,prs".
    pub github_sync_scope: Option<String>,
}

/// Non-secret provenance metadata for a task imported from a GitHub issue or PR.
/// Stored under the key `"github"` inside `tasks.meta_json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubProvenance {
    /// Full repository name (owner/repo).
    pub repo: String,
    /// GitHub database id for the issue or PR.
    #[serde(default)]
    pub issue_id: Option<i64>,
    /// GitHub node id for the issue or PR.
    #[serde(default)]
    pub node_id: Option<String>,
    /// Issue or PR number on GitHub.
    pub number: i64,
    /// Canonical HTML URL for the remote issue.
    #[serde(default)]
    pub html_url: Option<String>,
    /// Remote issue title as imported from GitHub.
    #[serde(default)]
    pub title: Option<String>,
    /// Remote issue body as imported from GitHub.
    #[serde(default)]
    pub body: Option<String>,
    /// Remote issue state ("open" / "closed").
    #[serde(default)]
    pub state: Option<String>,
    /// Assignee logins attached to the remote issue.
    #[serde(default)]
    pub assignees: Vec<String>,
    /// Remote issue creator login.
    #[serde(default)]
    pub creator: Option<String>,
    /// RFC3339 timestamp when the remote issue was last updated.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// RFC3339 timestamp when this task was synced.
    #[serde(alias = "imported_at")]
    pub synced_at: DateTime<Utc>,
    /// GitHub login of the user who performed the sync (not a token).
    #[serde(alias = "imported_by")]
    pub synced_by: Option<String>,
}

/// A single comment imported from a GitHub issue.
/// Stored under the key `"github_comments"` inside `tasks.meta_json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubComment {
    /// GitHub database id for the comment (stable deduplication key).
    pub comment_id: i64,
    /// Login of the comment author.
    pub author: String,
    /// Comment body text.
    pub body: String,
    /// Canonical HTML URL for the comment.
    pub url: String,
    /// When the comment was created on GitHub.
    pub created_at: DateTime<Utc>,
    /// When the comment was last updated on GitHub.
    pub updated_at: DateTime<Utc>,
}

/// A code anchor pointing at a file (or symbol) relevant to a task.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RelevantFile {
    pub path: String,
    pub reason: Option<String>,
    pub symbol: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
}

/// A captured note or link in Sara's store (indexed in SQLite, body in markdown).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub uuid: Uuid,
    /// Small display id within kind (n1, l2 prefixes in CLI)
    pub display_id: Option<i64>,
    pub kind: String,
    pub title: String,
    pub url: Option<String>,
    pub project: Option<String>,
    pub tags: Vec<String>,
    /// Relative path inside the store
    pub path: Option<String>,
    pub summary: Option<String>,
    pub body: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub status: String,
    /// The task this memory was learned from/about, if any. Loose reference
    /// (no FK): a memory outlives the task it points at.
    pub source_task_uuid: Option<Uuid>,
    /// File paths this memory is associated with (absolute). Not auto-populated
    /// by row_to_item — call set_item_files / find_items_by_file explicitly.
    pub files: Vec<String>,
    /// Tasks linked to this memory with their source label ("auto" or
    /// "explicit"). Not auto-populated by row_to_item — call
    /// set_item_task_links / get_item_task_links explicitly.
    /// Tuple: (task display id as string, description, source label).
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
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ymd(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
    }

    #[test]
    fn add_months_clamps_short_target_month() {
        use chrono::Datelike;
        // Jan 31 + 1 month must land on Feb 28 (2025 is not a leap year),
        // not silently stay on Jan 31.
        let r = add_months(ymd(2025, 1, 31), 1);
        assert_eq!((r.year(), r.month(), r.day()), (2025, 2, 28));

        // Leap year: Feb has 29 days.
        let r = add_months(ymd(2024, 1, 31), 1);
        assert_eq!((r.year(), r.month(), r.day()), (2024, 2, 29));

        // 31-day month -> 30-day month.
        let r = add_months(ymd(2025, 3, 31), 1);
        assert_eq!((r.year(), r.month(), r.day()), (2025, 4, 30));
    }

    #[test]
    fn add_months_preserves_valid_day_and_rolls_year() {
        use chrono::Datelike;
        let r = add_months(ymd(2025, 1, 15), 1);
        assert_eq!((r.year(), r.month(), r.day()), (2025, 2, 15));

        // Crossing the year boundary.
        let r = add_months(ymd(2025, 12, 15), 1);
        assert_eq!((r.year(), r.month(), r.day()), (2026, 1, 15));

        // +12 months (yearly), Feb 29 -> Feb 28 on a non-leap target.
        let r = add_months(ymd(2024, 2, 29), 12);
        assert_eq!((r.year(), r.month(), r.day()), (2025, 2, 28));
    }
}
