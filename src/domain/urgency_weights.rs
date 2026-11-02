use serde::{Deserialize, Serialize};

/// How much each signal contributes to a task's urgency score. Loaded from the
/// `[urgency]` section of the config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UrgencyWeights {
    pub priority_h: f64,
    pub priority_m: f64,
    pub priority_l: f64,
    pub due: f64,
    pub blocking: f64,
    pub blocked: f64,
    pub active: f64,
    pub has_tags: f64,
    pub project: f64,
    pub age_max: f64,
    pub age: f64,
}

impl Default for UrgencyWeights {
    fn default() -> Self {
        UrgencyWeights {
            priority_h: 6.0,
            priority_m: 3.9,
            priority_l: 1.8,
            due: 12.0,
            blocking: 8.0,
            blocked: -5.0,
            active: 4.0,
            has_tags: 1.0,
            project: 1.0,
            age_max: 365.0,
            age: 2.0,
        }
    }
}
