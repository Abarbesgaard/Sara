use chrono::{DateTime, Utc};

use super::{Task, UrgencyWeights};

/// A task's urgency score, split into the contribution of each signal.
pub struct UrgencyBreakdown {
    pub priority: f64,
    pub due: f64,
    pub blocking: f64,
    pub blocked: f64,
    pub active: f64,
    pub tags: f64,
    pub project: f64,
    pub age: f64,
}

impl UrgencyBreakdown {
    pub fn total(&self) -> f64 {
        self.priority
            + self.due
            + self.blocking
            + self.blocked
            + self.active
            + self.tags
            + self.project
            + self.age
    }
}

pub fn compute_urgency(
    task: &Task,
    weights: &UrgencyWeights,
    is_blocked: bool,
    blocking_count: usize,
    now: DateTime<Utc>,
) -> f64 {
    compute_urgency_breakdown(task, weights, is_blocked, blocking_count, now).total()
}

pub fn compute_urgency_breakdown(
    task: &Task,
    weights: &UrgencyWeights,
    is_blocked: bool,
    blocking_count: usize,
    now: DateTime<Utc>,
) -> UrgencyBreakdown {
    let priority = task
        .priority
        .as_ref()
        .map(|p| p.urgency_coefficient())
        .unwrap_or(0.0);

    // Ramps from 0 a week out to the full weight once the task is due.
    let due = task.due.map_or(0.0, |due| {
        let days_until = (due - now).num_seconds() as f64 / 86400.0;
        weights.due * (1.0 - days_until / 7.0).clamp(0.0, 1.0)
    });

    let age_days = (now - task.entry).num_days() as f64;

    UrgencyBreakdown {
        priority,
        due,
        blocking: if blocking_count > 0 {
            weights.blocking
        } else {
            0.0
        },
        blocked: if is_blocked { weights.blocked } else { 0.0 },
        active: if task.is_active() {
            weights.active
        } else {
            0.0
        },
        tags: if task.tags.is_empty() {
            0.0
        } else {
            weights.has_tags
        },
        project: if task.project == "inbox" {
            0.0
        } else {
            weights.project
        },
        age: weights.age * (age_days / weights.age_max).min(1.0),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/domain/urgency_breakdown.rs"]
mod tests;
