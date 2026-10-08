use chrono::{Duration, NaiveDate};
use std::collections::HashMap;

use crate::infrastructure::db::DayTask;

pub(super) struct ActivityData {
    pub counts: HashMap<NaiveDate, u32>,
    pub project: Option<String>,
    pub total_created: u32,
    pub total_completed: u32,
    pub cur_streak: u32,
    pub longest_streak: u32,
}

pub(super) struct Drill {
    pub day: NaiveDate,
    pub tasks: Vec<DayTask>,
}

pub(super) struct ActivityState {
    pub data: ActivityData,
    pub today: NaiveDate,
    pub cursor: NaiveDate,
    pub drill: Option<Drill>,
}

pub(super) const HISTORY_DAYS: i64 = 364;

impl ActivityState {
    pub fn new(data: ActivityData, today: NaiveDate) -> Self {
        Self {
            data,
            today,
            cursor: today,
            drill: None,
        }
    }

    pub fn move_by(&mut self, days: i64) {
        let earliest = self.today - Duration::days(HISTORY_DAYS);
        self.cursor = (self.cursor + Duration::days(days)).clamp(earliest, self.today);
    }

    pub fn count(&self, day: NaiveDate) -> u32 {
        self.data.counts.get(&day).copied().unwrap_or(0)
    }
}
