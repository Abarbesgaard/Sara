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
