use std::collections::HashSet;

use crate::infrastructure::db::LinkFlags;
use crate::infrastructure::model::Task;

pub enum BoardAction {
    Quit,
    OpenTask(String),
}

pub struct IssueNode {
    pub owner_repo: String,
    pub number: u64,
    pub title: Option<String>,
    pub tasks: Vec<Task>,
    pub done: usize,
    pub total: usize,
    pub expanded: bool,
}

pub struct BoardState {
    pub project: String,
    pub issues: Vec<IssueNode>,
    pub standalone: Vec<Task>,
    pub badges: std::collections::HashMap<String, LinkFlags>,
    pub show_finished: bool,
    pub imported: HashSet<String>,
    pub selected: usize,
    pub scroll: u16,
    pub pending: usize,
    pub done: usize,
}
