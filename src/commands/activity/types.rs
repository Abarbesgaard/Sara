use chrono::NaiveDate;
use std::collections::HashMap;

pub(super) struct ActivityData {
    pub counts: HashMap<NaiveDate, u32>,
    pub project: Option<String>,
    pub total_created: u32,
    pub total_completed: u32,
    pub cur_streak: u32,
    pub longest_streak: u32,
}
