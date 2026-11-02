use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RelevantFile {
    pub path: String,
    pub reason: Option<String>,
    pub symbol: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
}
