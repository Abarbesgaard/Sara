use chrono::{DateTime, Utc};

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
