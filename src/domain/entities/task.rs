use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::value_objects::{Priority, Status};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub uuid: Uuid,
    pub id: Option<i64>,
    pub description: String,
    pub project: String,
    pub status: Status,
    pub priority: Option<Priority>,
    pub due: Option<DateTime<Utc>>,
    pub entry: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
    pub urgency: f64,
    pub started_at: Option<DateTime<Utc>>,
    pub time_spent: i64,
    pub estimate_mins: Option<i64>,
    pub recur: Option<String>,
}

impl Task {
    pub fn is_active(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn total_time_spent(&self) -> i64 {
        let live = self
            .started_at
            .map(|s| (Utc::now() - s).num_seconds().max(0))
            .unwrap_or(0);
        self.time_spent + live
    }

    pub fn next_due(&self, base: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let interval = self.recur.as_deref()?;
        Some(advance_by_interval(base, interval))
    }

    pub fn stop_timer(&mut self, now: DateTime<Utc>) {
        if let Some(started) = self.started_at.take() {
            self.time_spent += (now - started).num_seconds().max(0);
        }
    }

    pub fn complete(&mut self, now: DateTime<Utc>) {
        self.stop_timer(now);
        self.close(Status::Completed, now);
    }

    pub fn delete(&mut self, now: DateTime<Utc>) {
        self.close(Status::Deleted, now);
    }

    fn close(&mut self, status: Status, now: DateTime<Utc>) {
        self.status = status;
        self.end = Some(now);
        self.modified = now;
    }

    /// The task that replaces this one when a recurring task is completed.
    pub fn next_occurrence(&self, now: DateTime<Utc>) -> Option<Task> {
        let due = self.next_due(self.due.unwrap_or(now))?;
        let mut next = Task::new(self.description.clone(), self.project.clone());
        next.priority = self.priority.clone();
        next.tags = self.tags.clone();
        next.due = Some(due);
        next.recur = self.recur.clone();
        next.estimate_mins = self.estimate_mins;
        Some(next)
    }
}

pub fn advance_by_interval(base: DateTime<Utc>, interval: &str) -> DateTime<Utc> {
    let s = interval.trim().to_lowercase();
    if s == "daily" {
        return base + chrono::Duration::days(1);
    }
    if s == "weekly" {
        return base + chrono::Duration::weeks(1);
    }
    if s == "monthly" {
        return add_months(base, 1);
    }
    if s == "yearly" {
        return add_months(base, 12);
    }
    if let Some(stripped) = s.strip_suffix('d')
        && let Ok(n) = stripped.parse::<i64>()
    {
        return base + chrono::Duration::days(n);
    }
    if let Some(stripped) = s.strip_suffix('w')
        && let Ok(n) = stripped.parse::<i64>()
    {
        return base + chrono::Duration::weeks(n);
    }
    if let Some(stripped) = s.strip_suffix('m')
        && let Ok(n) = stripped.parse::<i64>()
    {
        return add_months(base, n as u32);
    }
    base + chrono::Duration::weeks(1)
}

fn add_months(dt: DateTime<Utc>, months: u32) -> DateTime<Utc> {
    use chrono::Datelike;
    let total_month = dt.month0() + months;
    let extra_years = total_month / 12;
    let new_month = (total_month % 12) + 1;
    let new_year = dt.year() + extra_years as i32;
    let max_day = days_in_month(new_year, new_month);
    let new_day = dt.day().min(max_day);
    dt.with_day(1)
        .and_then(|d| d.with_year(new_year))
        .and_then(|d| d.with_month(new_month))
        .and_then(|d| d.with_day(new_day))
        .unwrap_or(dt)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

pub fn format_duration(secs: i64) -> String {
    if secs <= 0 {
        return "0m".to_string();
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

impl Task {
    pub fn new(description: String, project: String) -> Self {
        let now = Utc::now();
        Task {
            uuid: Uuid::new_v4(),
            id: None,
            description,
            project,
            status: Status::Pending,
            priority: None,
            due: None,
            entry: now,
            modified: now,
            end: None,
            tags: vec![],
            urgency: 0.0,
            started_at: None,
            time_spent: 0,
            estimate_mins: None,
            recur: None,
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/domain/entities/task.rs"]
mod tests;
