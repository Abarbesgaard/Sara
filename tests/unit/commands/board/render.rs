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
        cards: HashMap::new(),
        filter: String::new(),
        filtering: false,
        preview: false,
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

fn char_col_of(line: &str, needle: &str) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
    chars
        .windows(needle_chars.len())
        .position(|w| w == needle_chars.as_slice())
        .unwrap()
}

fn char_col_of_any(line: &str, candidates: &[char]) -> usize {
    line.chars().position(|c| candidates.contains(&c)).unwrap()
}

fn draw(st: &BoardState) -> String {
    let rows = visible_rows(st);
    let (lines, _) = build_lines(st, &rows);
    crate::test_support::render_to_string(100, 24, |f| render(f, st, &lines))
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

fn strip_of<'a>(out: &'a str, desc: &str) -> &'a str {
    let lines: Vec<&str> = out.lines().collect();
    let i = lines
        .iter()
        .position(|l| !l.starts_with("[") && l.contains(desc))
        .unwrap();
    lines[i + 1]
}

#[test]
fn every_card_shows_validation_acceptance_and_feedback() {
    let a = Task::new("bare task".into(), "tk".into());
    let st = board_state(vec![], vec![a]);
    let strip = strip_of(&draw(&st), "bare task").replace('\u{a0}', " ");
    assert!(strip.contains("· unvalidated"), "{strip}");
    assert!(strip.contains("☐ no criteria"), "{strip}");
    assert!(strip.contains("· no feedback"), "{strip}");
}

#[test]
fn card_strip_shows_rich_state_when_present() {
    use super::super::types::{CardInfo, Freshness};
    let mut a = Task::new("rich task".into(), "tk".into());
    a.due = Some(chrono::Utc::now() - chrono::Duration::hours(50));
    let uuid = a.uuid.to_string();
    let mut st = board_state(vec![], vec![a]);
    st.cards.insert(
        uuid,
        CardInfo {
            freshness: Freshness::Stale,
            branch: Some("feat/x".into()),
            accept_done: 1,
            accept_total: 3,
            step: Some("Build it".into()),
            feedback: 2,
            revise: 1,
            blocked_by: vec![4, 7],
        },
    );
    let mut out = String::new();
    let rows = visible_rows(&st);
    let (lines, _) = build_lines(&st, &rows);
    for l in &lines {
        out.push_str(&l.to_string());
        out.push('\n');
    }
    let strip = strip_of(&out, "rich task").replace('\u{a0}', " ");
    for want in [
        "⚠ stale",
        "☐ 1/3",
        "! 2 ⟳ 1 revise",
        "⊘ blocked #4 #7",
        "◷ overdue 2d",
        "⎇ feat/x",
        "▸ Build it",
    ] {
        assert!(strip.contains(want), "missing {want}: {strip}");
    }
}

#[test]
fn valid_guide_reads_valid_and_complete_criteria_read_full() {
    use super::super::types::{CardInfo, Freshness};
    let a = Task::new("ok task".into(), "tk".into());
    let card = CardInfo {
        freshness: Freshness::Valid,
        accept_done: 2,
        accept_total: 2,
        ..Default::default()
    };
    let badges = strip_badges(&a, &card, false);
    assert_eq!(badges[0].0.replace('\u{a0}', " "), "✓ valid");
    assert_eq!(badges[1].0.replace('\u{a0}', " "), "☐ 2/2");
}

#[test]
fn issue_header_shows_progress_bar() {
    let t = Task::new("x".into(), "tk".into());
    let st = board_state(vec![issue_node(5, vec![t], false)], vec![]);
    let out = draw(&st);
    let header = out.lines().find(|l| l.contains("#5")).unwrap();
    assert!(header.contains('▱') && header.contains("0/1"), "{header}");
}

#[test]
fn most_urgent_pending_task_is_marked_next() {
    let mut a = Task::new("calm".into(), "tk".into());
    a.urgency = 1.0;
    let mut b = Task::new("urgent".into(), "tk".into());
    b.urgency = 9.0;
    let st = board_state(vec![], vec![a, b]);
    let out = draw(&st);
    let row = |d: &str| {
        out.lines()
            .find(|l| l.starts_with('│') && l.contains(d))
            .unwrap()
            .to_string()
    };
    let strip = |d: &str| strip_of(&out, d).replace('\u{a0}', " ");
    assert!(row("urgent").starts_with("│★"));
    assert!(strip("urgent").contains("★ NEXT"));
    assert!(!row("calm").contains('★'));
    assert!(!strip("calm").contains("★ NEXT"));
    assert!(out.contains("Next: urgent"));
}

#[test]
fn filter_matches_text_and_tags() {
    let mut a = Task::new("wire the theme".into(), "tk".into());
    a.tags = vec!["tui".into()];
    let b = Task::new("fix the db".into(), "tk".into());
    assert!(matches_filter(&a, "THEME"));
    assert!(matches_filter(&a, "+tui"));
    assert!(matches_filter(&a, "tu"));
    assert!(!matches_filter(&a, "+tu"));
    assert!(!matches_filter(&b, "theme"));
    assert!(matches_filter(&b, "fix db"));
    assert!(!matches_filter(&b, "fix theme"));
}

#[test]
fn filter_hides_unmatched_issues_and_opens_matched_ones() {
    let a = Task::new("wire the theme".into(), "tk".into());
    let b = Task::new("other".into(), "tk".into());
    let c = Task::new("theme again".into(), "tk".into());
    let d = Task::new("loose".into(), "tk".into());
    let mut st = board_state(
        vec![
            issue_node(5, vec![a, b], false),
            issue_node(6, vec![c.clone()], false),
        ],
        vec![d],
    );
    st.issues[1].tasks[0].description = "unrelated".into();
    st.filter = "theme".into();
    let out: String = draw(&st)
        .lines()
        .filter(|l| l.starts_with('│') || l.starts_with('╭'))
        .map(|l| format!("{l}\n"))
        .collect();
    assert!(out.contains("#5"));
    assert!(out.contains("wire the theme"));
    assert!(!out.contains("other"));
    assert!(!out.contains("#6"));
    assert!(!out.contains("loose"));
    assert!(
        out.contains("└─"),
        "last visible task closes the tree: {out}"
    );
    assert!(out.contains("tk · / theme"));
}

#[test]
fn filter_with_no_hits_says_so() {
    let mut st = board_state(vec![], vec![Task::new("x".into(), "tk".into())]);
    st.filter = "zzz".into();
    let out = draw(&st);
    assert!(out.contains("no tasks match"), "{out}");
}

#[test]
fn filter_prompt_replaces_footer_while_typing() {
    let mut st = board_state(vec![], vec![Task::new("x".into(), "tk".into())]);
    st.filtering = true;
    st.filter = "th".into();
    let out = draw(&st);
    let footer = out.lines().last().unwrap();
    assert!(footer.contains("/ th▏"), "{footer}");
    assert!(footer.contains("Esc clear"), "{footer}");
}

#[test]
fn footer_is_contextual_to_the_selected_row() {
    let t = Task::new("x".into(), "tk".into());
    let mut st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
    let footer = |st: &BoardState| draw(st).lines().last().unwrap().to_string();
    assert!(footer(&st).contains("h/l fold"));
    st.selected = 1;
    assert!(footer(&st).contains("Enter open"));
    assert!(footer(&st).contains("p preview"));
}

fn draw_with_pane(st: &BoardState, w: u16, pane: &[Line<'static>]) -> String {
    let rows = visible_rows(st);
    let (lines, _) = build_lines(st, &rows);
    crate::test_support::render_to_string(w, 24, |f| render_with(f, st, &lines, Some(pane)))
}

#[test]
fn preview_pane_shows_beside_the_list_when_wide_enough() {
    let st = board_state(vec![], vec![Task::new("x".into(), "tk".into())]);
    let pane = vec![Line::from("PREVIEW BODY")];
    let out = draw_with_pane(&st, 130, &pane);
    assert!(out.contains(" preview "), "{out}");
    assert!(out.contains("PREVIEW BODY"));
    let narrow = draw_with_pane(&st, 100, &pane);
    assert!(!narrow.contains("PREVIEW BODY"));
}

#[test]
fn preview_pane_hints_when_an_issue_is_selected() {
    let st = board_state(vec![], vec![Task::new("x".into(), "tk".into())]);
    let out = draw_with_pane(&st, 130, &[]);
    assert!(out.contains("select a task to preview it"));
}

#[test]
fn preview_width_only_when_wide() {
    assert_eq!(preview_width(PREVIEW_MIN_WIDTH - 1), None);
    assert!(preview_width(140).unwrap() > 40);
}

#[test]
fn info_preview_lines_reuse_the_info_body() {
    let conn = crate::infrastructure::db::open_in_memory_for_test();
    let t = crate::test_support::seed_task(&conn, "previewed task", "tk");
    let cfg = crate::infrastructure::config::Config::default();
    let lines = crate::commands::info::preview_lines(&conn, &cfg, &t.uuid.to_string(), 50).unwrap();
    let text: String = lines.iter().map(|l| l.to_string() + "\n").collect();
    assert!(text.contains("previewed task"), "{text}");
    assert!(lines.iter().all(|l| l.width() <= 50));
}

fn styled_board() -> BoardState {
    let mut a = Task::new("wire the theme".into(), "tk".into());
    a.id = Some(1);
    a.priority = Some(Priority::H);
    a.tags = vec!["tui".into()];
    a.due = Some(chrono::Utc::now() + chrono::Duration::hours(84));
    let mut b = Task::new("pin snapshots".into(), "tk".into());
    b.id = Some(2);
    b.priority = Some(Priority::M);
    b.started_at = Some(chrono::Utc::now());
    let mut c = Task::new("old chore".into(), "tk".into());
    c.id = Some(3);
    c.priority = Some(Priority::L);
    let mut d = Task::new("loose end".into(), "tk".into());
    d.id = Some(4);
    let pr = a.uuid.to_string();
    let mut st = board_state(
        vec![
            issue_node(225, vec![a, b], true),
            issue_node(224, vec![c], false),
        ],
        vec![d],
    );
    st.badges.insert(
        pr,
        LinkFlags {
            any: true,
            pr: true,
            issue: true,
        },
    );
    st.cards.insert(
        st.issues[0].tasks[0].uuid.to_string(),
        super::super::types::CardInfo {
            freshness: super::super::types::Freshness::Stale,
            branch: Some("feat/theme".into()),
            accept_done: 1,
            accept_total: 2,
            step: Some("Pin the palette".into()),
            feedback: 1,
            revise: 0,
            blocked_by: vec![],
        },
    );
    st.cards.insert(
        st.issues[0].tasks[1].uuid.to_string(),
        super::super::types::CardInfo {
            freshness: super::super::types::Freshness::Valid,
            accept_done: 2,
            accept_total: 2,
            blocked_by: vec![1],
            ..Default::default()
        },
    );
    st.issues[0].tasks[0].urgency = 9.0;
    st.selected = 1;
    st
}

#[test]
fn styled_snapshot_board_with_preview() {
    let mut st = styled_board();
    st.preview = true;
    let pane = vec![Line::from("wire the theme"), Line::from("▌ CHECKLIST  1/2")];
    let rows = visible_rows(&st);
    let (lines, _) = build_lines(&st, &rows);
    insta::assert_snapshot!(crate::test_support::render_to_string(130, 24, |f| {
        render_with(f, &st, &lines, Some(&pane))
    }));
}

#[test]
fn styled_snapshot_board() {
    let st = styled_board();
    let rows = visible_rows(&st);
    let (lines, _) = build_lines(&st, &rows);
    insta::assert_snapshot!(crate::test_support::render_to_styled_string(100, 24, |f| {
        render(f, &st, &lines)
    }));
}

#[test]
fn styled_snapshot_board_retro() {
    use crate::infrastructure::tui::theme::{Palette, set_look};
    set_look(Palette::Retro, true, true);
    let st = styled_board();
    let rows = visible_rows(&st);
    let (lines, _) = build_lines(&st, &rows);
    let out = crate::test_support::render_to_styled_string(100, 24, |f| render(f, &st, &lines));
    set_look(Palette::Classic, true, true);
    insta::assert_snapshot!(out);
}

#[test]
fn board_too_small_shows_fallback() {
    let st = styled_board();
    let rows = visible_rows(&st);
    let (lines, _) = build_lines(&st, &rows);
    let out = crate::test_support::render_to_string(59, 20, |f| render(f, &st, &lines));
    assert!(out.contains("Terminal too small"), "{out}");
}
