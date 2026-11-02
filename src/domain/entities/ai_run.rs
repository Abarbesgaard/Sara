use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct AiRun {
    pub id: i64,
    pub kind: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub created_at: DateTime<Utc>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}
