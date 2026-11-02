pub struct ChecklistItem {
    pub id: i64,
    pub text: String,
    pub done: bool,
    pub position: i64,
    pub intent: Option<String>,
    pub kind: String,
    pub source: String,
    pub verify_cmd: Option<String>,
    pub result: Option<String>,
    pub done_commit: Option<String>,
    pub done_at: Option<String>,
}
