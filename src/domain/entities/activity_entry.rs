use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityEntry {
    pub id: i64,
    pub task_uuid: Uuid,
    pub text: String,
    pub client: Option<String>,
    pub at: DateTime<Utc>,
}
