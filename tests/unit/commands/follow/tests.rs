use super::render::{age, rail, render};
use super::state::*;
use crate::infrastructure::db::{self, FlowEvent, FlowKind};
use crate::infrastructure::tui::theme::Theme;
use crate::test_support::{key, render_to_string};
use chrono::{DateTime, Duration, TimeZone, Utc};
use crossterm::event::KeyCode;
use uuid::Uuid;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
}

fn stall() -> Duration {
    Duration::minutes(DEFAULT_STALL_MINS)
}

fn mk_card(
    id: i64,
    project: &str,
    done: usize,
    total: usize,
    doing: Option<&str>,
    quiet_secs: i64,
) -> Card {
    let last = now() - Duration::seconds(quiet_secs);
    let steps: Vec<Step> = (0..total)
        .map(|i| Step {
            text: format!("step {}", i + 1),
            done: i < done,
        })
        .collect();
    Card {
        uuid: Uuid::new_v4(),
        id: Some(id),
        description: format!("task {id}"),
        project: project.into(),
        pulse: pulse(last, now(), total - done, stall()),
        steps,
        doing: doing.map(str::to_owned),
        last,
    }
}

fn mission(cards: Vec<Card>, project: Option<&str>) -> App {
    let mut app = App::new(
        Mode::Mission,
        false,
        project.map(str::to_owned),
        false,
        stall(),
    );
    app.now = now();
    app.cards = cards;
    app
}

fn draw(app: &App, w: u16, h: u16) -> String {
    let theme = Theme::new(false);
    render_to_string(w, h, |f| render(f, app, &theme))
}

#[test]
fn flow_pulse_is_live_within_two_minutes() {
    assert_eq!(
        pulse(now() - Duration::seconds(30), now(), 3, stall()),
        Pulse::Live
    );
    assert_eq!(
        pulse(
            now() - Duration::seconds(LIVE_WINDOW_SECS),
            now(),
            0,
            stall()
        ),
        Pulse::Live
    );
}

#[test]
fn flow_pulse_stalls_only_with_open_steps_past_threshold() {
    let quiet = now() - Duration::minutes(11);
    assert_eq!(pulse(quiet, now(), 2, stall()), Pulse::Stalled);
    assert_eq!(pulse(quiet, now(), 0, stall()), Pulse::Idle);
    assert_eq!(
        pulse(now() - Duration::minutes(5), now(), 2, stall()),
        Pulse::Idle
    );
    assert_eq!(
        pulse(now() - Duration::minutes(5), now(), 2, Duration::minutes(3)),
        Pulse::Stalled
    );
}

#[test]
fn flow_auto_minimal_below_40_by_12() {
    assert!(!is_minimal(false, 40, 12));
    assert!(is_minimal(false, 39, 30));
    assert!(is_minimal(false, 120, 11));
    assert!(is_minimal(true, 200, 60));
}

#[test]
fn flow_card_from_db_counts_steps_and_reads_doing() {
    let conn = db::open_in_memory_for_test();
    let t = crate::test_support::seed_task(&conn, "build it", "p");
    db::add_checklist_item(&conn, &t.uuid, "one").unwrap();
    db::add_checklist_item(&conn, &t.uuid, "two").unwrap();
    let first = db::get_checklist(&conn, &t.uuid).unwrap().remove(0);
    db::set_step_done(&conn, first.id, true, None, None).unwrap();
    db::record_doing(&conn, &t.uuid, "on step two", None).unwrap();
    let cards = mission_cards(&conn, Utc::now(), stall()).unwrap();
    assert_eq!(cards.len(), 1);
    let c = &cards[0];
    assert_eq!(
        (c.done_count(), c.steps.len(), c.current()),
        (1, 2, Some(1))
    );
    assert_eq!(c.doing.as_deref(), Some("on step two"));
    assert_eq!(c.pulse, Pulse::Live);

    let f = focus(&conn, &t.uuid, Utc::now(), stall()).unwrap();
    assert!(f.events.iter().any(|e| e.kind == FlowKind::Doing));
}

#[test]
fn flow_scope_is_all_projects_unless_tied() {
    let cards = || {
        vec![
            mk_card(1, "a", 0, 2, None, 5),
            mk_card(2, "b", 0, 2, None, 5),
        ]
    };
    let mut app = mission(cards(), Some("a"));
    app.all = true;
    assert_eq!(app.visible_cards(false).len(), 2);
    assert_eq!(app.visible_cards(true).len(), 2);
    app.handle_key(key(KeyCode::Char('p')), true);
    assert_eq!(app.visible_cards(true).len(), 1);
    let tied = mission(cards(), Some("a"));
    assert_eq!(tied.visible_cards(true).len(), 1);
    assert_eq!(tied.visible_cards(false).len(), 1);
}

#[test]
fn flow_keys_select_open_and_return() {
    let mut app = mission(
        vec![
            mk_card(1, "a", 0, 2, None, 5),
            mk_card(2, "a", 0, 2, None, 5),
        ],
        None,
    );
    app.handle_key(key(KeyCode::Down), false);
    app.handle_key(key(KeyCode::Down), false);
    assert_eq!(app.selected, 1);
    let target = app.cards[1].uuid;
    app.handle_key(key(KeyCode::Enter), false);
    assert_eq!(app.mode, Mode::Task(target));
    assert_eq!(app.handle_key(key(KeyCode::Esc), false), Outcome::Continue);
    assert_eq!(app.mode, Mode::Mission);
    assert_eq!(app.handle_key(key(KeyCode::Esc), false), Outcome::Quit);
    assert_eq!(
        app.handle_key(key(KeyCode::Char('q')), false),
        Outcome::Quit
    );
}

#[test]
fn flow_age_is_compact() {
    assert_eq!(age(now(), now() - Duration::seconds(9)), "9s");
    assert_eq!(age(now(), now() - Duration::minutes(3)), "3m");
    assert_eq!(age(now(), now() - Duration::hours(2)), "2h");
    assert_eq!(age(now(), now() - Duration::days(4)), "4d");
}

#[test]
fn follow_render_rail_compresses_long_guides() {
    let theme = Theme::new(false);
    let c = mk_card(1, "a", 3, 5, None, 5);
    let s: String = rail(&c, 10, &theme)
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    assert_eq!(s, "●●●◆○");
    let long = mk_card(1, "a", 10, 20, None, 5);
    let s: String = rail(&long, 10, &theme)
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    assert_eq!(s, "●●●●●◆○○○○");
}

#[test]
fn follow_render_minimal_one_task_fits_24x6() {
    let app = mission(
        vec![mk_card(
            26,
            "sara",
            9,
            14,
            Some("running the parser tests"),
            5,
        )],
        Some("sara"),
    );
    let out = draw(&app, 24, 6);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "26 ●●●●●●●●●◆○○○○ 9/14");
    assert_eq!(lines[1], "  ▸ running the parser…");
    assert!(lines[2..].iter().all(|l| l.is_empty()), "{out}");
}

#[test]
fn follow_render_minimal_many_tasks_overflow_into_a_counter() {
    let cards = (1..=5)
        .map(|i| mk_card(i, "sara", 1, 3, Some("busy"), 5))
        .collect();
    let app = mission(cards, Some("sara"));
    let out = draw(&app, 24, 6);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 6);
    assert_eq!(lines[4], "3 ●◆○ 1/3", "{out}");
    assert_eq!(lines[5], "+2 more", "{out}");
    assert!(lines.iter().all(|l| l.chars().count() <= 24));
}

#[test]
fn follow_render_minimal_no_tasks() {
    let app = mission(vec![], Some("sara"));
    let out = draw(&app, 24, 6);
    assert_eq!(out.lines().next(), Some("sara · no active tasks"));
}

#[test]
fn follow_render_auto_minimal_in_a_small_pane() {
    let app = mission(vec![mk_card(7, "sara", 1, 2, None, 5)], Some("sara"));
    let small = draw(&app, 39, 20);
    assert!(small.starts_with("7 ●◆ 1/2"), "{small}");
    let big = draw(&app, 80, 20);
    assert!(
        big.starts_with(" sara  ▸ follow · mission control"),
        "{big}"
    );
}

#[test]
fn follow_render_mission_shows_pulse_rail_and_doing() {
    let app = mission(
        vec![
            mk_card(26, "sara", 2, 4, Some("writing tests"), 30),
            mk_card(3, "pling", 1, 3, None, 60 * 20),
        ],
        None,
    );
    let out = draw(&app, 80, 14);
    assert!(out.contains("all projects · 1 live · 1 stalled"), "{out}");
    assert!(out.contains("▸  LIVE  26  task 26"), "{out}");
    assert!(out.contains("sara · 30s"), "{out}");
    assert!(out.contains("●●◆○ 2/4  step 3"), "{out}");
    assert!(out.contains("▸ writing tests"), "{out}");
    assert!(out.contains(" STALLED  3  task 3"), "{out}");
    assert!(out.contains("pling · 20m"), "{out}");
    assert!(
        out.lines()
            .last()
            .unwrap()
            .starts_with("↑↓ select  ⏎ follow  m minimal  q quit"),
        "{out}"
    );
}

#[test]
fn follow_render_mission_empty_explains_how_to_report() {
    let out = draw(&mission(vec![], None), 80, 12);
    assert!(out.contains("No agent activity in the last 24h."), "{out}");
    assert!(out.contains("`doing` MCP tool"), "{out}");
}

#[test]
fn follow_render_task_shows_flow_timeline_newest_last() {
    let card = mk_card(26, "sara", 1, 3, Some("wiring the CLI"), 10);
    let ev = |secs: i64, kind: FlowKind, text: &str, detail: Option<&str>| FlowEvent {
        at: now() - Duration::seconds(secs),
        kind,
        text: text.into(),
        detail: detail.map(str::to_owned),
    };
    let events = vec![
        ev(600, FlowKind::StepAdded, "step 1", None),
        ev(300, FlowKind::StepDone, "step 1", Some("green")),
        ev(
            120,
            FlowKind::Note("finding".into()),
            "lexer is lossy",
            None,
        ),
        ev(60, FlowKind::Memory("recalled".into()), "m12", None),
        ev(10, FlowKind::Doing, "wiring the CLI", None),
    ];
    let mut app = App::new(Mode::Task(card.uuid), false, None, false, stall());
    app.now = now();
    app.focus = Some(Focus { card, events });
    let out = draw(&app, 60, 16);
    assert!(out.starts_with(" sara  ▸ follow · 26"), "{out}");
    assert!(out.lines().next().unwrap().contains("LIVE"), "{out}");
    assert!(out.contains(" ◆ step 2"), "{out}");
    assert!(out.contains("10m ○ step added: step 1"), "{out}");
    assert!(out.contains("5m ● step 1 — green"), "{out}");
    assert!(out.contains("2m » finding: lexer is lossy"), "{out}");
    assert!(out.contains("1m ≈ recalled m12"), "{out}");
    let pos = |s: &str| out.find(s).unwrap();
    assert!(pos("step added") < pos("10s ▸ wiring the CLI"));
    assert!(out.lines().last().unwrap().starts_with("esc quit"), "{out}");
}

fn fev(secs: i64, kind: FlowKind, text: &str) -> FlowEvent {
    FlowEvent {
        at: now() - Duration::seconds(secs),
        kind,
        text: text.into(),
        detail: None,
    }
}

fn item(card: &Card, e: FlowEvent) -> FeedItem {
    FeedItem {
        uuid: card.uuid,
        project: card.project.clone(),
        label: card.label(),
        event: e,
    }
}

#[test]
fn follow_render_minimal_feed_is_newest_first() {
    let card = mk_card(26, "sara", 2, 4, Some("wiring"), 5);
    let feed = vec![
        item(&card, fev(300, FlowKind::StepDone, "recall")),
        item(
            &card,
            fev(120, FlowKind::Note("finding".into()), "lexer lossy"),
        ),
        item(&card, fev(5, FlowKind::Doing, "wiring")),
    ];
    let mut app = mission(vec![card], Some("sara"));
    app.feed = feed;
    let out = draw(&app, 30, 6);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "26 ●●◆○ 2/4", "{out}");
    assert_eq!(lines[1], "  ◆ step 3", "{out}");
    assert_eq!(lines[2], " 5s ▸ wiring", "{out}");
    assert_eq!(lines[3], " 2m » finding: lexer lossy", "{out}");
    assert_eq!(lines[4], " 5m ● recall", "{out}");
    assert!(lines.iter().all(|l| l.chars().count() <= 30));
}

#[test]
fn follow_render_minimal_feed_tails_and_labels_many_tasks() {
    let a = mk_card(7, "sara", 1, 2, None, 5);
    let b = mk_card(8, "sara", 0, 2, None, 5);
    let mut feed: Vec<FeedItem> = (0..20)
        .map(|i| item(&a, fev(1000 - i * 10, FlowKind::Doing, &format!("a{i}"))))
        .collect();
    feed.push(item(&b, fev(1, FlowKind::Doing, "b last")));
    let mut app = mission(vec![a, b], Some("sara"));
    app.feed = feed;
    let out = draw(&app, 24, 12);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 12, "{out}");
    assert!(lines[0].starts_with("7 "), "{out}");
    assert_eq!(lines[1], "  ◆ step 2", "{out}");
    assert!(lines[2].starts_with("8 "), "{out}");
    assert_eq!(lines[3], "  ◆ step 1", "{out}");
    assert!(lines[4].starts_with('─'), "{out}");
    assert_eq!(lines[5], " 1s ▸ 8 b last", "{out}");
    assert!(lines[6].contains("7 a19"), "{out}");
    assert!(lines[11].contains("7 a14"), "{out}");
    assert!(lines.iter().all(|l| l.chars().count() <= 24));
}

#[test]
fn follow_render_minimal_task_mode_shows_its_feed_without_label() {
    let card = mk_card(26, "sara", 1, 3, Some("cli"), 10);
    let events = vec![
        fev(600, FlowKind::StepAdded, "step 1"),
        fev(500, FlowKind::Change("modified".into()), "noise"),
        fev(10, FlowKind::Doing, "cli"),
    ];
    let mut app = App::new(Mode::Task(card.uuid), true, None, false, stall());
    app.now = now();
    app.focus = Some(Focus { card, events });
    let out = draw(&app, 24, 6);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "26 ●◆○ 1/3", "{out}");
    assert_eq!(lines[1], "  ◆ step 2", "{out}");
    assert_eq!(lines[2], "10s ▸ cli", "{out}");
    assert_eq!(lines[3], "10m ○ step added: step 1", "{out}");
    assert!(!out.contains("noise"), "{out}");
}

#[test]
fn follow_feed_merges_tasks_ascending_and_skips_changes() {
    let conn = db::open_in_memory_for_test();
    let a = crate::test_support::seed_task(&conn, "alpha", "p").uuid;
    let b = crate::test_support::seed_task(&conn, "beta", "p").uuid;
    db::record_doing(&conn, &a, "a1", None).unwrap();
    db::record_doing(&conn, &b, "b1", None).unwrap();
    db::record_doing(&conn, &a, "a2", None).unwrap();
    let cards = mission_cards(&conn, Utc::now(), stall()).unwrap();
    let items = feed(&conn, &cards).unwrap();
    let texts = |u: Uuid| -> Vec<&str> {
        items
            .iter()
            .filter(|i| i.uuid == u && i.event.kind == FlowKind::Doing)
            .map(|i| i.event.text.as_str())
            .collect()
    };
    assert_eq!(texts(a), ["a1", "a2"]);
    assert_eq!(texts(b), ["b1"]);
    assert!(items.windows(2).all(|w| w[0].event.at <= w[1].event.at));
    assert!(
        items
            .iter()
            .all(|i| !matches!(i.event.kind, FlowKind::Change(_)))
    );
}

#[test]
fn follow_render_minimal_feed_collapses_many_tasks_into_a_counter() {
    let cards: Vec<Card> = (1..=5).map(|i| mk_card(i, "sara", 1, 3, None, 5)).collect();
    let feed = vec![item(&cards[0], fev(5, FlowKind::Doing, "busy"))];
    let mut app = mission(cards, Some("sara"));
    app.feed = feed;
    let out = draw(&app, 24, 6);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "1 ●◆○ 1/3", "{out}");
    assert_eq!(lines[1], "  ◆ step 2", "{out}");
    assert_eq!(lines[2], "+4 more", "{out}");
    assert_eq!(lines[3], " 5s ▸ 1 busy", "{out}");
}

#[test]
fn follow_render_minimal_groups_projects_and_names_them_in_the_feed() {
    let a = mk_card(7, "sara", 1, 2, None, 5);
    let b = mk_card(88, "pling", 0, 2, None, 60);
    let feed = vec![
        item(&b, fev(60, FlowKind::Doing, "tests")),
        item(&a, fev(5, FlowKind::Doing, "wiring")),
    ];
    let mut app = mission(vec![a, b], Some("sara"));
    app.all = true;
    app.feed = feed;
    let out = draw(&app, 30, 12);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], format!("── sara {}", "─".repeat(22)), "{out}");
    assert!(lines[1].starts_with("7 "), "{out}");
    assert_eq!(lines[2], "  ◆ step 2", "{out}");
    assert!(lines[3].starts_with("── pling ─"), "{out}");
    assert!(lines[4].starts_with("88 "), "{out}");
    assert_eq!(lines[5], "  ◆ step 1", "{out}");
    assert!(lines[6].starts_with('─'), "{out}");
    assert_eq!(lines[7], " 5s ▸ sara 7 wiring", "{out}");
    assert_eq!(lines[8], " 1m ▸ pling 88 tests", "{out}");
}

fn styled(app: &App, w: u16, h: u16) -> String {
    let theme = Theme::new(true);
    crate::test_support::render_to_styled_string(w, h, |f| render(f, app, &theme))
}

fn fixed(mut c: Card, n: u128) -> Card {
    c.uuid = Uuid::from_u128(n);
    c
}

#[test]
fn styled_snapshot_follow_mission() {
    let app = mission(
        vec![
            fixed(mk_card(26, "sara", 2, 4, Some("writing tests"), 30), 1),
            fixed(mk_card(3, "pling", 1, 3, None, 60 * 20), 2),
            fixed(mk_card(9, "pling", 3, 3, None, 60 * 60 * 30), 3),
        ],
        None,
    );
    insta::assert_snapshot!(styled(&app, 80, 16));
}

#[test]
fn styled_snapshot_follow_minimal() {
    let a = fixed(mk_card(26, "sara", 2, 4, Some("wiring"), 5), 1);
    let b = fixed(mk_card(3, "pling", 1, 3, None, 60 * 20), 2);
    let feed = vec![
        item(&b, fev(1200, FlowKind::StepDone, "recall")),
        item(
            &a,
            fev(120, FlowKind::Note("finding".into()), "lexer lossy"),
        ),
        item(&a, fev(60, FlowKind::Memory("recalled".into()), "m12")),
        item(&a, fev(5, FlowKind::Doing, "wiring")),
    ];
    let mut app = mission(vec![a, b], None);
    app.minimal = true;
    app.feed = feed;
    insta::assert_snapshot!(styled(&app, 40, 12));
}

#[test]
fn styled_snapshot_follow_task() {
    let card = fixed(mk_card(26, "sara", 1, 3, Some("wiring the CLI"), 10), 1);
    let events = vec![
        fev(600, FlowKind::StepAdded, "step 1"),
        fev(300, FlowKind::StepDone, "step 1"),
        fev(120, FlowKind::Note("risk".into()), "lexer is lossy"),
        fev(60, FlowKind::Memory("recalled".into()), "m12"),
        fev(10, FlowKind::Doing, "wiring the CLI"),
    ];
    let mut app = App::new(Mode::Task(card.uuid), false, None, false, stall());
    app.now = now();
    app.focus = Some(Focus { card, events });
    insta::assert_snapshot!(styled(&app, 60, 16));
}

#[test]
fn styled_snapshot_follow_mission_retro() {
    use crate::infrastructure::tui::theme::{Palette, set_look};
    set_look(Palette::Retro, true, true);
    let app = mission(
        vec![
            fixed(mk_card(26, "sara", 2, 4, Some("writing tests"), 30), 1),
            fixed(mk_card(3, "pling", 1, 3, None, 60 * 20), 2),
        ],
        None,
    );
    let out = styled(&app, 80, 12);
    set_look(Palette::Classic, true, true);
    insta::assert_snapshot!(out);
}

#[test]
fn follow_keys_support_jump_to_top_and_bottom() {
    let mut app = mission(
        vec![
            mk_card(1, "sara", 0, 2, None, 5),
            mk_card(2, "sara", 0, 2, None, 6),
            mk_card(3, "sara", 0, 2, None, 7),
        ],
        None,
    );
    app.handle_key(key(KeyCode::Char('G')), false);
    assert_eq!(app.selected, 2);
    app.handle_key(key(KeyCode::Char('g')), false);
    app.handle_key(key(KeyCode::Char('g')), false);
    assert_eq!(app.selected, 0);
}
