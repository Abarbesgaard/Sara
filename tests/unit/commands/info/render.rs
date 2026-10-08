use super::super::handler::focusables;
use super::super::types::{Detail, Focusable, GraphNode, TaskTree};
use super::*;
use crate::infrastructure::model::{Priority, Status, Task};
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
        ledger: vec![],
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
fn checklist_detail_shows_for_current_and_selected_rows_unless_verbose() {
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
    assert!(out.contains("do the first thing"));
    assert!(!out.contains("do the second thing"));

    let idx = focusables(&st.detail, st.show_notes)
        .iter()
        .position(|f| matches!(f, Focusable::Checklist(1)))
        .unwrap();
    st.selected = idx;
    let out2 = draw(&mut st);
    assert!(out2.contains("do the first thing"));
    assert!(out2.contains("do the second thing"));

    st.detail.checklist[0].done = true;
    st.selected = 0;
    let out_after = draw(&mut st);
    assert!(!out_after.contains("do the first thing"));
    assert!(out_after.contains("do the second thing"));

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

#[test]
fn info_too_small_shows_fallback_and_recovers_on_resize() {
    let mut st = base_state(base_detail(task()));
    let small = draw_at(&mut st, 50, 10);
    assert!(small.contains("Terminal too small"), "{small}");
    assert!(small.contains("need 60×16, have 50×10"), "{small}");
    let big = draw_at(&mut st, 100, 24);
    assert!(!big.contains("Terminal too small"), "{big}");
}

fn snap_filters() -> Vec<(&'static str, &'static str)> {
    vec![
        (r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}", "YYYY-MM-DD hh:mm"),
        (r"\d{2}-\d{2} \d{2}:\d{2}", "MM-DD hh:mm"),
        (r"\b\d{2}:\d{2}\b", "hh:mm"),
    ]
}

fn comment(
    id: i64,
    text: &str,
    target: Option<(&str, &str)>,
    status: &str,
    request_revision: bool,
) -> crate::infrastructure::db::Annotation {
    crate::infrastructure::db::Annotation {
        id,
        text: text.into(),
        entry: Utc::now(),
        kind: "comment".into(),
        author: "human".into(),
        target_kind: target.map(|(k, _)| k.to_string()),
        target_id: target.map(|(_, i)| i.to_string()),
        status: status.into(),
        request_revision,
        resolved_by_run: None,
    }
}

fn link(id: i64, url: &str, label: Option<&str>) -> crate::infrastructure::db::Link {
    crate::infrastructure::db::Link {
        id,
        url: url.into(),
        label: label.map(String::from),
        entry: Utc::now(),
    }
}

fn anchor(path: &str, source: &str, reason: Option<&str>) -> crate::infrastructure::db::Anchor {
    crate::infrastructure::db::Anchor {
        path: path.into(),
        source: source.into(),
        reason: reason.map(String::from),
        symbol: Some("render".into()),
        line_start: Some(10),
        line_end: Some(40),
    }
}

fn history_entry(
    field: &str,
    old: Option<&str>,
    new: Option<&str>,
) -> crate::infrastructure::db::HistoryEntry {
    crate::infrastructure::db::HistoryEntry {
        field: field.into(),
        old_value: old.map(String::from),
        new_value: new.map(String::from),
        changed_at: Utc::now(),
    }
}

fn select(st: &mut EditState, target: Focusable) {
    st.selected = focusables(&st.detail, st.show_notes)
        .iter()
        .position(|f| *f == target)
        .unwrap();
}

fn styled_snap(st: &mut EditState, w: u16, h: u16) -> String {
    crate::test_support::render_to_styled_string(w, h, |f| render(f, st))
}

#[test]
fn styled_snapshot_info_narrow_with_blockers() {
    let mut d = styled_detail();
    d.blocked_by = vec!["#3 wire the theme".into(), "#4 pick colours".into()];
    d.blocking = vec!["#9 ship the release".into()];
    d.ledger = vec![ledger_entry(
        "m12",
        "prefer ink() over Color literals",
        1,
        0,
        0,
    )];
    let mut st = base_state(d);
    st.selected = 2;
    let out = styled_snap(&mut st, 90, 50);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_links() {
    let mut d = styled_detail();
    d.links = vec![
        link(1, "https://github.com/acme/sara/pull/225", None),
        link(2, "https://example.com/docs", Some("design doc")),
        link(3, "https://example.com", Some("https://example.com")),
    ];
    let mut st = base_state(d);
    select(&mut st, Focusable::Link(1));
    let out = styled_snap(&mut st, 120, 48);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_files_and_anchors() {
    let mut d = styled_detail();
    d.manual_files = vec!["src/commands/info/render.rs".into(), "src/main.rs".into()];
    d.anchors = vec![
        anchor(
            "src/commands/info/handler.rs",
            "suggested",
            Some("builds focusables"),
        ),
        anchor("src/infrastructure/tui/theme.rs", "manual", None),
    ];
    d.annotations.push(comment(
        10,
        "check this one",
        Some(("anchor", "src/commands/info/handler.rs")),
        "open",
        true,
    ));
    d.annotations.push(comment(
        11,
        "old remark",
        Some(("anchor", "src/commands/info/handler.rs")),
        "resolved",
        false,
    ));
    let mut st = base_state(d);
    select(&mut st, Focusable::File("src/main.rs".into()));
    let out = styled_snap(&mut st, 120, 48);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_anchor_selected() {
    let mut d = styled_detail();
    d.anchors = vec![
        anchor(
            "src/commands/info/handler.rs",
            "suggested",
            Some("builds focusables"),
        ),
        anchor("src/infrastructure/tui/theme.rs", "manual", None),
    ];
    d.annotations.push(comment(
        10,
        "check this one",
        Some(("anchor", "src/commands/info/handler.rs")),
        "open",
        true,
    ));
    let mut st = base_state(d);
    select(&mut st, Focusable::Anchor(0));
    let out = styled_snap(&mut st, 120, 48);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_verification() {
    let mut d = styled_detail();
    let mut done_acc = step(4, "clippy clean", true, "acceptance");
    done_acc.done_commit = Some("0123456789abcdef".into());
    done_acc.done_at = Some("2026-01-02".into());
    done_acc.source = "ai".into();
    d.checklist.push(done_acc);
    d.project_commands = crate::infrastructure::db::ProjectCommands {
        setup_cmd: None,
        test_cmd: Some("cargo test".into()),
        lint_cmd: Some("cargo clippy".into()),
        run_cmd: Some("  ".into()),
    };
    d.guide.meta_json = Some(r#"{"bench_cmd":"cargo bench","empty":""}"#.into());
    d.annotations.push(comment(
        20,
        "split it smaller",
        Some(("step", "2")),
        "open",
        true,
    ));
    let mut st = base_state(d);
    select(&mut st, Focusable::Checklist(1));
    let selected = styled_snap(&mut st, 120, 60);
    st.verbose = true;
    let verbose = styled_snap(&mut st, 120, 60);
    let out = format!("{selected}\n{verbose}");
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_scrolled_to_selection() {
    let mut d = styled_detail();
    d.checklist = many_steps(30);
    d.checklist[25].intent = Some("why this step matters".into());
    d.annotations.push(comment(
        40,
        "on step 26",
        Some(("step", "26")),
        "open",
        false,
    ));
    let mut st = base_state(d);
    select(&mut st, Focusable::Checklist(25));
    let out = styled_snap(&mut st, 100, 20);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_ai_runs_and_similar() {
    let mut d = styled_detail();
    d.ai_runs = vec![
        crate::infrastructure::db::AiRun {
            id: 1,
            kind: "plan".into(),
            model: Some("gpt".into()),
            provider: Some("copilot".into()),
            created_at: Utc::now(),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        },
        crate::infrastructure::db::AiRun {
            id: 2,
            kind: "reflect".into(),
            model: None,
            provider: None,
            created_at: Utc::now(),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        },
    ];
    d.similar = vec![
        (11, "tidy the board".into(), 2.0),
        (12, "theme the follow screen".into(), 6.5),
        (13, "split board render".into(), 4.25),
        (14, "snapshot every screen".into(), 1.0),
    ];
    d.branch = Some(crate::infrastructure::db::BranchRecord {
        branch: "tui/info-split".into(),
    });
    let mut st = base_state(d);
    st.verbose = true;
    let out = styled_snap(&mut st, 120, 60);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_comments() {
    let mut d = styled_detail();
    d.annotations.extend([
        comment(30, "looks good overall", None, "open", false),
        comment(31, "reword this note", Some(("note", "1")), "open", true),
        comment(32, "step needs a test", Some(("step", "2")), "open", false),
        comment(
            33,
            "accept is vague",
            Some(("acceptance", "3")),
            "resolved",
            true,
        ),
        comment(34, "anchor remark", Some(("anchor", "x.rs")), "open", false),
        comment(35, "dangling", Some(("step", "999")), "open", false),
    ]);
    let mut st = base_state(d);
    select(&mut st, Focusable::Comment(1));
    let out = styled_snap(&mut st, 120, 60);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

fn with_history(mut d: Detail) -> Detail {
    d.history = vec![
        history_entry("created", None, Some("root task")),
        history_entry("priority", None, Some("H")),
        history_entry("annotation", None, Some("first comment")),
        history_entry("link", Some("https://old.example"), None),
        history_entry("due", Some("2026-01-01"), Some("2026-02-01")),
    ];
    d
}

#[test]
fn styled_snapshot_info_history() {
    let mut st = base_state(with_history(styled_detail()));
    let out = styled_snap(&mut st, 120, 48);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_editing_field() {
    let mut st = base_state(with_history(styled_detail()));
    st.editing = true;
    st.selected = 0;
    st.editor = ratatui_textarea::TextArea::from(["root task renamed"]);
    let out = styled_snap(&mut st, 120, 48);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_editing_errors() {
    let mut st = base_state(styled_detail());
    st.editing = true;
    st.selected = 3;
    st.due_error = true;
    st.editor = ratatui_textarea::TextArea::from(["not a date"]);
    let due = styled_snap(&mut st, 120, 48);
    st.due_error = false;
    st.selected = 7;
    st.dep_error = Some("no task 99".into());
    let dep = styled_snap(&mut st, 120, 48);
    st.dep_error = None;
    let dep_ok = styled_snap(&mut st, 120, 48);
    let out = format!("{due}\n{dep}\n{dep_ok}");
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_commenting() {
    let mut st = base_state(styled_detail());
    select(&mut st, Focusable::Checklist(1));
    st.commenting = true;
    st.editor = ratatui_textarea::TextArea::from(["needs a test"]);
    let on_step = styled_snap(&mut st, 120, 48);
    st.selected = 0;
    let on_task = styled_snap(&mut st, 120, 48);
    let out = format!("{on_step}\n{on_task}");
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn styled_snapshot_info_adding_step() {
    let mut st = base_state(with_history(styled_detail()));
    st.adding_step = true;
    st.editor = ratatui_textarea::TextArea::from(["write the snapshot"]);
    let with_hist = styled_snap(&mut st, 120, 48);
    st.detail.history.clear();
    let without_hist = styled_snap(&mut st, 120, 48);
    let out = format!("{with_hist}\n{without_hist}");
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

#[test]
fn header_shows_status_freshness_branch_feedback_and_ids() {
    let mut d = base_detail(task());
    d.task.id = Some(9);
    d.task.uuid = uuid::Uuid::parse_str("f9ea645f-0f27-46de-a527-ff70e13e8c53").unwrap();
    let out = draw(&mut base_state(d));
    let head = out.lines().next().unwrap().to_string();
    assert!(head.contains("○ pending"), "{head}");
    assert!(head.contains("· never validated"), "{head}");
    assert!(head.contains("⎇ no branch"), "{head}");
    assert!(head.contains("0 open"), "{head}");
    assert!(head.contains("#9 · f9ea645f"), "{head}");

    let mut d = base_detail(task());
    d.guide.validated_commit = Some("abc12345".into());
    d.head_commit = Some("abc12345".into());
    d.branch = Some(crate::infrastructure::db::BranchRecord {
        branch: "feat/info".into(),
    });
    d.annotations = vec![
        comment(1, "fix it", None, "open", true),
        comment(2, "nit", None, "open", false),
        comment(3, "old", None, "resolved", true),
    ];
    let out = draw(&mut base_state(d));
    let head = out.lines().next().unwrap().to_string();
    assert!(head.contains("✓ validated @ abc12345"), "{head}");
    assert!(head.contains("⎇ feat/info"), "{head}");
    assert!(head.contains("2 open"), "{head}");
    assert!(head.contains("⟳ 1 needs revision"), "{head}");

    let mut d = base_detail(task());
    d.guide.validated_commit = Some("abc12345".into());
    d.head_commit = Some("def67890".into());
    let out = draw(&mut base_state(d));
    let head = out.lines().next().unwrap().to_string();
    assert!(
        head.contains("⚠ stale @ abc12345 (HEAD def67890)"),
        "{head}"
    );

    let mut d = base_detail(task());
    d.task.started_at = Some(Utc::now());
    let out = draw(&mut base_state(d));
    let head = out.lines().next().unwrap().to_string();
    assert!(head.contains("● active"), "{head}");
}

#[test]
fn anchor_block_leads_the_body_and_is_absent_without_guide() {
    let out = draw(&mut base_state(styled_detail()));
    let anchor = out.find("Anchor").unwrap();
    let desc = out.find("Description").unwrap();
    assert!(anchor < desc, "{out}");
    assert!(out.contains("Why         screens drifted apart"), "{out}");
    let bare = draw(&mut base_state(base_detail(task())));
    assert!(!bare.contains("Anchor"), "{bare}");
}

#[test]
fn checklist_marks_current_step_and_shows_acceptance_bar_and_verify() {
    let mut d = base_detail(task());
    d.checklist = vec![
        step(1, "recall", true, "step"),
        step(2, "observe", false, "step"),
        step(3, "build", false, "step"),
        step(4, "tests pass", true, "acceptance"),
        step(5, "clippy clean", false, "acceptance"),
    ];
    let out = draw(&mut base_state(d));
    assert!(out.contains("◆ [ ] observe  ← next"), "{out}");
    assert!(!out.contains("build  ← next"), "{out}");
    assert!(out.contains("why observe"), "{out}");
    assert!(!out.contains("why build"), "{out}");
    assert!(
        out.contains("acceptance  ██████████░░░░░░░░░░ 1/2"),
        "{out}"
    );
    assert!(out.contains("verify cargo test  → green"), "{out}");
    assert!(out.contains("verify cargo test  → not run"), "{out}");
}

#[test]
fn styled_snapshot_info_header_and_steps() {
    let mut d = styled_detail();
    d.guide.validated_commit = Some("abc12345".into());
    d.head_commit = Some("def67890".into());
    d.branch = Some(crate::infrastructure::db::BranchRecord {
        branch: "feat/info".into(),
    });
    d.checklist
        .push(step(4, "clippy clean", true, "acceptance"));
    d.annotations.push(comment(
        9,
        "tighten this",
        Some(("step", "2")),
        "open",
        true,
    ));
    let mut st = base_state(d);
    let out = styled_snap(&mut st, 120, 48);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}

fn ledger_entry(
    handle: &str,
    snippet: &str,
    c: usize,
    r: usize,
    s: usize,
) -> super::super::types::LedgerEntry {
    super::super::types::LedgerEntry {
        handle: handle.into(),
        snippet: snippet.into(),
        cited: c,
        recalled: r,
        surfaced: s,
    }
}

fn ai_run(
    id: i64,
    p: Option<i64>,
    c: Option<i64>,
    t: Option<i64>,
) -> crate::infrastructure::db::AiRun {
    crate::infrastructure::db::AiRun {
        id,
        kind: "plan".into(),
        model: Some("gpt".into()),
        provider: Some("copilot".into()),
        created_at: Utc::now(),
        prompt_tokens: p,
        completion_tokens: c,
        total_tokens: t,
    }
}

#[test]
fn memory_ledger_lists_each_memory_with_use_counts_and_is_selectable() {
    let mut d = base_detail(task());
    d.ledger = vec![
        ledger_entry("m12", "retry the flaky test", 1, 2, 0),
        ledger_entry("m7", "prefer theme inks", 0, 0, 3),
    ];
    let mut st = base_state(d);
    let out = draw(&mut st);
    assert!(out.contains("Memory ledger  (2)"), "{out}");
    assert!(
        out.contains("m12   retry the flaky test  cited  recalled ×2"),
        "{out}"
    );
    assert!(
        out.contains("m7    prefer theme inks  surfaced ×3"),
        "{out}"
    );
    select(&mut st, Focusable::Memory(1));
    let out = draw(&mut st);
    assert!(out.contains("▶ m7"), "{out}");
}

#[test]
fn ai_activity_shows_tokens_per_run_and_a_total() {
    let mut d = base_detail(task());
    d.ai_runs = vec![
        ai_run(1, Some(1200), Some(345), Some(1545)),
        ai_run(2, Some(10), Some(5), None),
        ai_run(3, None, None, None),
    ];
    let out = draw(&mut base_state(d));
    assert!(out.contains("· 1,545 tok (1,200 in / 345 out)"), "{out}");
    assert!(out.contains("· 15 tok (10 in / 5 out)"), "{out}");
    assert!(out.contains("3 runs · 1,560 tokens"), "{out}");
}

#[test]
fn styled_snapshot_info_memory_ledger_and_tokens() {
    let mut d = styled_detail();
    d.ledger = vec![
        ledger_entry("m855", "one file per palette under tui/themes", 1, 1, 2),
        ledger_entry("m829", "TUI inventory", 0, 1, 0),
        ledger_entry("m487", "fold design analogue", 0, 0, 4),
    ];
    d.ai_runs = vec![ai_run(1, Some(1200), Some(345), Some(1545))];
    let mut st = base_state(d);
    select(&mut st, Focusable::Memory(0));
    let out = styled_snap(&mut st, 120, 52);
    insta::with_settings!({filters => snap_filters()}, {
        insta::assert_snapshot!(out);
    });
}
