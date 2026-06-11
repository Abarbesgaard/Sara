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
}
