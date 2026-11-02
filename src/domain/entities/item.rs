use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::value_objects::MemoryStatus;

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
    pub status: MemoryStatus,
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
            status: MemoryStatus::Active,
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
            status: MemoryStatus::Active,
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
            status: MemoryStatus::Active,
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
#[path = "../../../tests/unit/domain/entities/item.rs"]
mod tests;
