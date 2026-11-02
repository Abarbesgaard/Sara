use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::value_objects::MemoryUseKind;

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryUse {
    pub item_uuid: Uuid,
    pub task_uuid: Uuid,
    pub kind: MemoryUseKind,
    pub at: DateTime<Utc>,
}
