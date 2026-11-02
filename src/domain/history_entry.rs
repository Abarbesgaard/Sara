use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub changed_at: DateTime<Utc>,
}
