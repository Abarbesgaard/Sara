use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Annotation {
    pub id: i64,
    pub text: String,
    pub entry: DateTime<Utc>,
    pub kind: String,
    pub author: String,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub status: String,
    pub request_revision: bool,
    pub resolved_by_run: Option<i64>,
}
