use super::super::types::TaskTree;
use super::*;
use crate::infrastructure::model::{Status, Task};
use chrono::Utc;

fn task() -> Task {
    Task::new("root task".into(), "tk".into())
}

fn node(id: i64, status: Status) -> GraphNode {
    GraphNode {
        uuid: uuid::Uuid::new_v4(),
        id: Some(id),
        status,
        badge: None,
        description: format!("task {id}"),
        children: vec![],
        hidden_children: 0,
    }
}

fn node_with_children(id: i64, children: Vec<GraphNode>) -> GraphNode {
    GraphNode {
        children,
        ..node(id, Status::Pending)
    }
}

fn typed_note(id: i64, kind: &str, text: &str) -> crate::infrastructure::db::Annotation {
    crate::infrastructure::db::Annotation {
        id,
        text: text.into(),
        entry: Utc::now(),
        kind: kind.into(),
        author: "ai".into(),
        target_kind: None,
        target_id: None,
        status: "open".into(),
        request_revision: false,
        resolved_by_run: None,
    }
}

fn base_detail(task: Task) -> Detail {
    Detail {
        task,
        blocked_by: vec![],
        blocking: vec![],
        cited: vec![],
        depends_on_ids: vec![],
        manual_files: vec![],
        suggested_files: vec![],
        links: vec![],
        annotations: vec![],
        history: vec![],
        project_root: None,
        branch: None,
        similar: vec![],
        checklist: vec![],
        urgency_breakdown: None,
        activity: std::collections::HashMap::new(),
        stats: None,
        guide: crate::infrastructure::db::TaskGuideFields::default(),
        anchors: vec![],
        ai_runs: vec![],
        head_commit: None,
        project_commands: crate::infrastructure::db::ProjectCommands::default(),
        tree: TaskTree::default(),
    }
}

fn base_state(detail: Detail) -> EditState {
    EditState {
        detail,
        selected: 0,
        editing: false,
        commenting: false,
        adding_step: false,
        editor: ratatui_textarea::TextArea::default(),
        due_error: false,
        dep_error: None,
        scroll: 0,
        last_selected: None,
        tree_expanded: false,
        show_urgency_breakdown: false,
        verbose: false,
        show_notes: false,
    }
}

fn draw(st: &mut EditState) -> String {
    draw_at(st, 140, 60)
}

fn draw_at(st: &mut EditState, w: u16, h: u16) -> String {
    crate::test_support::render_to_string(w, h, |f| render(f, st))
}

#[test]
fn task_tree_does_not_panic_when_empty() {
    let d = base_detail(task());
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(out.contains("Task tree"));
    assert!(out.contains("none"));
}

#[test]
fn task_tree_shows_branching_blockers_and_dependents() {
    let mut d = base_detail(task());
    d.tree = TaskTree {
        blockers: vec![node(1, Status::Pending), node(2, Status::Completed)],
        blockers_hidden: 0,
        dependents: vec![node(3, Status::Pending)],
        dependents_hidden: 0,
    };
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(out.contains("blocked by (2)"));
    assert!(out.contains("blocks"));
    assert!(out.contains("task 1"));
    assert!(out.contains("task 2"));
    assert!(out.contains("task 3"));
}

#[test]
fn task_tree_collapses_overflow_to_a_summary_row_by_default() {
    let mut d = base_detail(task());
    d.tree = TaskTree {
        blockers: (1..=8).map(|i| node(i, Status::Pending)).collect(),
        blockers_hidden: 0,
        dependents: vec![],
        dependents_hidden: 0,
    };
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(out.contains("more"));
}

#[test]
fn task_tree_expanded_shows_everything_and_drops_the_summary_row() {
    let mut d = base_detail(task());
    d.tree = TaskTree {
        blockers: (1..=8).map(|i| node(i, Status::Pending)).collect(),
        blockers_hidden: 0,
        dependents: vec![],
        dependents_hidden: 0,
    };
    let mut st = base_state(d);
    st.tree_expanded = true;
    let out = draw(&mut st);
    assert!(!out.contains("more"));
    for i in 1..=8 {
        assert!(
            out.contains(&format!("task {i}")),
            "expected task {i} to be visible"
        );
    }
}

#[test]
fn task_tree_nests_grandchildren_with_tree_connectors() {
    let mut d = base_detail(task());
    let grandchild = node(100, Status::Pending);
    let child = node_with_children(2, vec![grandchild]);
    d.tree = TaskTree {
        blockers: vec![node(1, Status::Pending), child],
        blockers_hidden: 0,
        dependents: vec![],
        dependents_hidden: 0,
    };
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(out.contains("task 100"));
    assert!(out.contains("└─") || out.contains("├─"));
}

#[test]
fn task_tree_hides_beyond_compact_depth_until_expanded() {
    let great_grandchild = node(1000, Status::Pending);
    let grandchild = node_with_children(100, vec![great_grandchild]);
    let child = node_with_children(10, vec![grandchild]);
    let mut d = base_detail(task());
    d.tree = TaskTree {
        blockers: vec![child],
        blockers_hidden: 0,
        dependents: vec![],
        dependents_hidden: 0,
    };
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(out.contains("task 10"));
    assert!(out.contains("task 100"));
    assert!(!out.contains("task 1000"));

    st.tree_expanded = true;
    let out2 = draw(&mut st);
    assert!(out2.contains("task 1000"));
}

#[test]
fn status_row_hidden_when_pending_shown_otherwise() {
    let d = base_detail(task());
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(!out.lines().any(|l| l.trim_start().starts_with("Status")));

    let mut completed_task = task();
    completed_task.status = Status::Completed;
    let d2 = base_detail(completed_task);
    let mut st2 = base_state(d2);
    let out2 = draw(&mut st2);
    assert!(out2.contains("Status"));
    assert!(out2.contains("completed"));
}

#[test]
fn urgency_breakdown_hidden_by_default_and_shown_when_toggled() {
    let mut d = base_detail(task());
    d.task.urgency = 5.0;
    d.urgency_breakdown = Some(crate::infrastructure::db::UrgencyBreakdown {
        priority: 3.0,
        due: 0.0,
        blocking: 0.0,
        blocked: 0.0,
        active: 0.0,
        tags: 0.0,
        project: 0.0,
        age: 0.0,
    });
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(out.contains("u for breakdown"));
    assert!(!out.contains("pri 3.0"));

    st.show_urgency_breakdown = true;
    let out2 = draw(&mut st);
    assert!(out2.contains("pri 3.0"));
}

#[test]
fn risk_notes_always_show_but_other_notes_collapse_until_toggled() {
    let mut d = base_detail(task());
    d.annotations = vec![
        typed_note(1, "risk", "touches the shared urgency formula"),
        typed_note(2, "finding", "existing tests cover this path"),
        typed_note(3, "decision", "kept the old signature"),
    ];
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(out.contains("Risks"));
    assert!(out.contains("touches the shared urgency formula"));
    assert!(!out.contains("existing tests cover this path"));
    assert!(!out.contains("kept the old signature"));
    assert!(out.contains("n to view"));

    let mut st2 = st;
    st2.show_notes = true;
    let out2 = draw(&mut st2);
    assert!(out2.contains("existing tests cover this path"));
    assert!(out2.contains("kept the old signature"));
}

#[test]
fn long_assignment_is_collapsed_until_verbose() {
    let mut d = base_detail(task());
    let long = "x".repeat(200);
    d.guide.assignment = Some(long.clone());
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(out.contains("v to expand"));
    assert!(!out.contains(&long));

    st.verbose = true;
    let out2 = draw(&mut st);
    assert!(!out2.contains("v to expand"));
}

#[test]
fn checklist_detail_only_shows_for_selected_row_unless_verbose() {
    let mut d = base_detail(task());
    d.checklist = vec![
        crate::infrastructure::db::ChecklistItem {
            id: 1,
            text: "first step".into(),
            done: false,
            position: 0,
            intent: Some("do the first thing".into()),
            kind: "step".into(),
            source: "human".into(),
            verify_cmd: None,
            result: None,
            done_commit: None,
            done_at: None,
        },
        crate::infrastructure::db::ChecklistItem {
            id: 2,
            text: "second step".into(),
            done: false,
            position: 1,
            intent: Some("do the second thing".into()),
            kind: "step".into(),
            source: "human".into(),
            verify_cmd: None,
            result: None,
            done_commit: None,
            done_at: None,
        },
    ];
    let mut st = base_state(d);
    st.selected = 0;
    let out = draw(&mut st);
    assert!(!out.contains("do the first thing"));
    assert!(!out.contains("do the second thing"));

    let idx = focusables(&st.detail, st.show_notes)
        .iter()
        .position(|f| matches!(f, Focusable::Checklist(0)))
        .unwrap();
    st.selected = idx;
    let out2 = draw(&mut st);
    assert!(out2.contains("do the first thing"));
    assert!(!out2.contains("do the second thing"));

    st.verbose = true;
    let out3 = draw(&mut st);
    assert!(out3.contains("do the first thing"));
    assert!(out3.contains("do the second thing"));
}

fn many_steps(n: i64) -> Vec<crate::infrastructure::db::ChecklistItem> {
    (0..n)
        .map(|i| crate::infrastructure::db::ChecklistItem {
            id: i + 1,
            text: format!("step number {i}"),
            done: false,
            position: i,
            intent: None,
            kind: "step".into(),
            source: "human".into(),
            verify_cmd: None,
            result: None,
            done_commit: None,
            done_at: None,
        })
        .collect()
}

#[test]
fn selection_follow_scrolls_highlighted_row_into_view() {
    let mut d = base_detail(task());
    d.checklist = many_steps(40);
    let mut st = base_state(d);
    st.selected = focusables(&st.detail, st.show_notes)
        .iter()
        .position(|f| matches!(f, Focusable::Checklist(39)))
        .unwrap();

    let out = draw_at(&mut st, 100, 20);
    assert!(st.scroll > 0, "viewport should have scrolled down");
    assert!(out.contains("step number 39"));
}

#[test]
fn manual_scroll_is_clamped_but_not_snapped_back() {
    let mut d = base_detail(task());
    d.checklist = many_steps(40);
    let mut st = base_state(d);
    st.last_selected = Some(st.selected);
    st.scroll = 500;
    draw_at(&mut st, 100, 20);
    assert!(st.scroll < 500, "scroll should be clamped to content");
    assert!(st.scroll > 0, "scroll must not snap back to the selection");
}

fn step(id: i64, text: &str, done: bool, kind: &str) -> crate::infrastructure::db::ChecklistItem {
    crate::infrastructure::db::ChecklistItem {
        id,
        text: text.into(),
        done,
        position: id,
        intent: Some(format!("why {text}")),
        kind: kind.into(),
        source: "human".into(),
        verify_cmd: (kind == "acceptance").then(|| "cargo test".into()),
        result: done.then(|| "green".into()),
        done_commit: None,
        done_at: None,
    }
}

fn styled_detail() -> Detail {
    let mut t = task();
    t.uuid = uuid::Uuid::from_u128(0xa71b8b5f);
    t.id = Some(6);
    t.priority = Some(Priority::H);
    t.tags = vec!["tui".into(), "theme".into()];
    t.due = Some(Utc::now() + chrono::Duration::hours(84));
    t.urgency = 7.5;
    let mut d = base_detail(t);
    d.guide.assignment = Some("make the TUI coherent".into());
    d.guide.rationale = Some("screens drifted apart".into());
    d.checklist = vec![
        step(1, "recall", true, "step"),
        step(2, "observe", false, "step"),
        step(3, "snapshots pass", false, "acceptance"),
    ];
    d.annotations = vec![
        typed_note(1, "risk", "info render is huge"),
        typed_note(2, "finding", "only text snapshots exist"),
    ];
    let mut a = node(1, Status::Completed);
    a.uuid = uuid::Uuid::from_u128(1);
    let mut b = node(2, Status::Pending);
    b.uuid = uuid::Uuid::from_u128(2);
    d.tree = TaskTree {
        blockers: vec![a],
        blockers_hidden: 0,
        dependents: vec![b],
        dependents_hidden: 0,
    };
    d
}

#[test]
fn styled_snapshot_info() {
    let mut st = base_state(styled_detail());
    st.selected = 1;
    let out = crate::test_support::render_to_styled_string(120, 48, |f| render(f, &mut st));
    insta::with_settings!({filters => vec![(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}", "YYYY-MM-DD hh:mm")]}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_with_notes() {
    let mut st = base_state(styled_detail());
    st.show_notes = true;
    st.show_urgency_breakdown = true;
    st.verbose = true;
    let out = crate::test_support::render_to_styled_string(120, 48, |f| render(f, &mut st));
    insta::with_settings!({filters => vec![(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}", "YYYY-MM-DD hh:mm")]}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_retro() {
    use crate::infrastructure::tui::theme::{Palette, set_look};
    set_look(Palette::Retro, true, true);
    let mut st = base_state(styled_detail());
    st.selected = 1;
    let out = crate::test_support::render_to_styled_string(120, 48, |f| render(f, &mut st));
    set_look(Palette::Classic, true, true);
    insta::with_settings!({filters => vec![(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}", "YYYY-MM-DD hh:mm")]}, {
        insta::assert_snapshot!(out);
    });
}
