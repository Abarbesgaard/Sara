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

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Freshness {
    #[default]
    Unvalidated,
    Valid,
    Stale,
}

#[derive(Clone, Default, Debug)]
pub struct CardInfo {
    pub freshness: Freshness,
    pub branch: Option<String>,
    pub accept_done: usize,
    pub accept_total: usize,
    pub step: Option<String>,
    pub feedback: usize,
    pub revise: usize,
    pub blocked_by: Vec<i64>,
}

pub struct BoardState {
    pub project: String,
    pub issues: Vec<IssueNode>,
    pub standalone: Vec<Task>,
    pub badges: std::collections::HashMap<String, LinkFlags>,
    pub cards: std::collections::HashMap<String, CardInfo>,
    pub filter: String,
    pub filtering: bool,
    pub preview: bool,
    pub show_finished: bool,
    pub imported: HashSet<String>,
    pub selected: usize,
    pub scroll: u16,
    pub pending: usize,
    pub done: usize,
}
