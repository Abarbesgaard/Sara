use super::*;
use crate::infrastructure::model::Task;
use ratatui::{Terminal, backend::TestBackend};
use std::collections::{HashMap, HashSet};

fn board_state(issues: Vec<IssueNode>, standalone: Vec<Task>) -> BoardState {
    let pending = issues
        .iter()
        .flat_map(|i| &i.tasks)
        .chain(standalone.iter())
        .filter(|t| t.status != Status::Completed)
        .count();
    BoardState {
        project: "tk".to_string(),
        pending,
        done: 0,
        issues,
        standalone,
        badges: HashMap::new(),
        show_finished: false,
        imported: HashSet::new(),
        selected: 0,
        scroll: 0,
    }
}

fn issue_node(number: u64, tasks: Vec<Task>, expanded: bool) -> IssueNode {
    let total = tasks.len();
    let done = tasks
        .iter()
        .filter(|t| t.status == Status::Completed)
        .count();
    IssueNode {
        owner_repo: "o/r".to_string(),
        number,
        title: None,
        tasks,
        done,
        total,
        expanded,
    }
}

/// Character (not byte) column of the first occurrence of `needle` in `line`.
fn char_col_of(line: &str, needle: &str) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
    chars
        .windows(needle_chars.len())
        .position(|w| w == needle_chars.as_slice())
        .unwrap()
}

/// Character column of the first occurrence of any char in `candidates`.
fn char_col_of_any(line: &str, candidates: &[char]) -> usize {
    line.chars().position(|c| candidates.contains(&c)).unwrap()
}

/// A generously sized terminal so the bordered box + header/footer chrome
/// all have room to render (the real board needs a real terminal size too).
fn draw(st: &BoardState) -> String {
    let rows = visible_rows(st);
    let (lines, _) = build_lines(st, &rows);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|f| render(f, st, &lines)).unwrap();
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let mut out = String::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[test]
fn collapsed_issue_hides_its_tasks() {
    let t = Task::new("child".into(), "tk".into());
    let st = board_state(vec![issue_node(5, vec![t], false)], vec![]);
    let out = draw(&st);
    assert!(out.contains("#5"));
    assert!(!out.lines().any(|l| l.contains('└') && l.contains("child")));
    assert!(!out.lines().any(|l| l.contains('├') && l.contains("child")));
}

#[test]
fn expanded_issue_shows_its_tasks() {
    let t = Task::new("child".into(), "tk".into());
    let st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
    let out = draw(&st);
    assert!(out.contains("#5"));
    assert!(out.contains("child"));
}

#[test]
fn pr_badge_renders_on_the_row() {
    let t = Task::new("has a pr".into(), "tk".into());
    let uuid = t.uuid.to_string();
    let mut st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
    st.badges.insert(
        uuid,
        LinkFlags {
            any: true,
            pr: true,
            issue: false,
        },
    );
    assert!(draw(&st).contains("PR"));
}

#[test]
fn issue_badge_only_renders_for_synced_tasks() {
    let t = Task::new("has an issue".into(), "tk".into());
    let uuid = t.uuid.to_string();
    let mut st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
    st.badges.insert(
        uuid.clone(),
        LinkFlags {
            any: true,
            pr: false,
            issue: true,
        },
    );
    assert!(!draw(&st).contains("ISS"));
    st.imported.insert(uuid);
    assert!(draw(&st).contains("ISS"));
}

#[test]
fn no_badge_for_task_without_links() {
    let t = Task::new("plain".into(), "tk".into());
    let st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
    let out = draw(&st);
    assert!(!out.contains("PR "));
    assert!(!out.contains("ISS"));
}

#[test]
fn standalone_tasks_render_without_an_issue_header() {
    let t = Task::new("loose".into(), "tk".into());
    let st = board_state(vec![], vec![t]);
    let out = draw(&st);
    assert!(out.contains("loose"));
    assert!(!out.contains('#'));
}

#[test]
fn nested_tasks_use_correct_tree_connectors() {
    let a = Task::new("alpha".into(), "tk".into());
    let b = Task::new("beta".into(), "tk".into());
    let st = board_state(vec![issue_node(5, vec![a, b], true)], vec![]);
    let out = draw(&st);
    assert!(out.lines().any(|l| l.contains("├─") && l.contains("alpha")));
    assert!(out.lines().any(|l| l.contains("└─") && l.contains("beta")));
}

#[test]
fn nested_tasks_indent_further_than_their_issue_header() {
    let a = Task::new("alpha".into(), "tk".into());
    let st = board_state(vec![issue_node(5, vec![a], true)], vec![]);
    let out = draw(&st);
    let header_line = out
        .lines()
        .find(|l| l.starts_with('│') && l.contains("#5"))
        .unwrap();
    let task_line = out
        .lines()
        .find(|l| l.starts_with('│') && l.contains("alpha"))
        .unwrap();
    // Compare the column of the tree-connector glyph itself, not raw
    // leading spaces — the selection marker on the header row otherwise
    // throws off a naive whitespace count.
    let header_tree_col = char_col_of_any(header_line, &['▸', '▾']);
    let task_tree_col = char_col_of_any(task_line, &['├', '└']);
    assert_eq!(
        task_tree_col,
        header_tree_col + 1,
        "nested task's tree connector should sit one column past its issue header's"
    );
}

#[test]
fn stats_row_shows_issue_count() {
    let t = Task::new("x".into(), "tk".into());
    let st = board_state(vec![issue_node(5, vec![t], false)], vec![]);
    assert!(draw(&st).contains("Issues:"));
}

#[test]
fn task_box_has_a_border_and_project_title() {
    let t = Task::new("x".into(), "tk".into());
    let st = board_state(vec![issue_node(5, vec![t], false)], vec![]);
    let out = draw(&st);
    assert!(out.contains('╭'));
    assert!(out.contains('╰'));
    assert!(out.contains("tk"));
}

#[test]
fn priority_legend_shows_swatches_not_proportional_bars() {
    let mut h = Task::new("h task".into(), "tk".into());
    h.priority = Some(Priority::H);
    let st = board_state(vec![], vec![h]);
    let out = draw(&st);
    assert!(out.contains("High"));
    assert!(out.contains("100%"));
}

#[test]
fn description_columns_align_regardless_of_badges() {
    // Two standalone tasks, one with a PR badge and one without — both
    // descriptions must start at the same column since the badge slot
    // is always reserved.
    let a = Task::new("has badge".into(), "tk".into());
    let b = Task::new("no badge".into(), "tk".into());
    let uuid_a = a.uuid.to_string();
    let mut st = board_state(vec![], vec![a, b]);
    st.badges.insert(
        uuid_a,
        LinkFlags {
            any: true,
            pr: true,
            issue: false,
        },
    );
    let out = draw(&st);
    // Restrict to rows inside the bordered box — the stats row's "[ Next: ... ]"
    // label can otherwise coincidentally contain the same description text.
    let line_a = out
        .lines()
        .find(|l| l.starts_with('│') && l.contains("has badge"))
        .unwrap();
    let line_b = out
        .lines()
        .find(|l| l.starts_with('│') && l.contains("no badge"))
        .unwrap();
    let col_a = char_col_of(line_a, "has badge");
    let col_b = char_col_of(line_b, "no badge");
    assert_eq!(col_a, col_b, "description column must stay aligned");
}

#[test]
fn help_bindings_are_all_things_this_screen_actually_handles() {
    let bindings = help_bindings();
    let labels: Vec<&str> = bindings.iter().map(|(k, _)| *k).collect();
    assert!(labels.contains(&"j/k, ↓/↑"));
    assert!(labels.contains(&"gg / G"));
    assert!(labels.contains(&"Enter"));
    assert!(labels.contains(&"q / Esc"));
    assert!(labels.contains(&"?"));
    let descriptions: Vec<&str> = bindings.iter().map(|(_, d)| *d).collect();
    assert!(descriptions.iter().any(|d| d.contains("expand")));
    assert!(!labels.iter().any(|l| l.contains("Ctrl+S")));
    assert!(!labels.iter().any(|l| l.contains("Shift")));
}

#[test]
fn help_overlay_renders_without_panicking() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| tui::render_help_overlay(f, "Board", &help_bindings()))
        .unwrap();
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buf[(x, y)].symbol());
        }
    }
    assert!(out.contains("Board"));
    assert!(out.contains("any key closes"));
}
