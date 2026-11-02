//! Memory strength: how much a memory should be trusted when it is surfaced.
//! It grows with the outcome of the work it came from or was used in, with
//! recent recalls, and with the memories derived from it.

use crate::domain::value_objects::{MemoryStatus, Status};

/// Provisional memories never start stronger than an unproven one.
pub const PROVISIONAL_BASE_CAP: f64 = 1.0;
pub const RECALL_BOOST_WINDOW_DAYS: i64 = 30;
pub const RECALL_BOOST_PER_HIT: f64 = 0.1;
pub const RECALL_BOOST_CAP: f64 = 0.5;
pub const DERIVED_BONUS_PER_CHILD: f64 = 0.1;
pub const DERIVED_BONUS_CAP: f64 = 0.5;

/// Base strength from the task a memory was learned in (when it still exists)
/// or, failing that, the tasks it is linked to: 2.0 once that work completed,
/// 1.5 while it is pending, 1.0 otherwise.
pub fn base_strength(status: MemoryStatus, source_task: Option<&Status>, linked: &[Status]) -> f64 {
    let base = match source_task {
        Some(Status::Completed) => 2.0,
        Some(_) => 1.5,
        None => from_linked_tasks(linked),
    };
    if status == MemoryStatus::Provisional {
        base.min(PROVISIONAL_BASE_CAP)
    } else {
        base
    }
}

fn from_linked_tasks(linked: &[Status]) -> f64 {
    let mut best = 1.0_f64;
    for st in linked {
        match st {
            Status::Completed => return 2.0,
            Status::Pending => best = 1.5,
            Status::Deleted => {}
        }
    }
    best
}

/// Bonus for being recalled `recalls` times within the boost window.
pub fn recall_boost(recalls: i64) -> f64 {
    (recalls as f64 * RECALL_BOOST_PER_HIT).min(RECALL_BOOST_CAP)
}

/// Bonus for having `children` memories derived from this one.
pub fn derived_bonus(children: usize) -> f64 {
    (children as f64 * DERIVED_BONUS_PER_CHILD).min(DERIVED_BONUS_CAP)
}

#[cfg(test)]
#[path = "../../../tests/unit/domain/services/memory_strength.rs"]
mod tests;
