use std::collections::HashMap;

use serde_json::{Value, json};

use crate::commands::shared::{short_handle, short_id};
use crate::infrastructure::model::Item;

pub(super) const DETAIL_LIMIT: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Ok,
    Warn,
    Info,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Info => "info",
        }
    }
}

pub(super) struct Check {
    pub(super) id: &'static str,
    pub(super) status: Status,
    pub(super) count: usize,
    pub(super) summary: String,
    pub(super) fix: Option<&'static str>,
    pub(super) details: Value,
}

impl Check {
    pub(super) fn new(id: &'static str, flagged: Status, count: usize, fix: &'static str) -> Self {
        let status = if count == 0 { Status::Ok } else { flagged };
        Check {
            id,
            status,
            count,
            summary: String::new(),
            fix: (count > 0).then_some(fix),
            details: Value::Null,
        }
    }

    pub(super) fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "status": self.status.as_str(),
            "count": self.count,
            "summary": self.summary,
            "fix": self.fix,
            "details": self.details,
        })
    }
}

pub(super) struct Labels(HashMap<String, String>);

impl Labels {
    pub(super) fn new(memories: &[Item]) -> Self {
        Labels(
            memories
                .iter()
                .map(|m| (m.uuid.to_string(), short_handle(m)))
                .collect(),
        )
    }

    pub(super) fn of(&self, uuid: &str) -> String {
        self.0.get(uuid).cloned().unwrap_or_else(|| short_id(uuid))
    }
}
