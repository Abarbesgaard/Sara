use super::*;
use chrono::Utc;

#[test]
fn truncate_leaves_short_strings_untouched() {
    assert_eq!(truncate("hello", 10), "hello");
    assert_eq!(truncate("hello", 5), "hello");
}

#[test]
fn truncate_adds_ellipsis_when_over_max() {
    assert_eq!(truncate("hello world", 5), "hell…");
}

#[test]
fn truncate_counts_chars_not_bytes() {
    assert_eq!(truncate("åney", 10), "åney");
}

#[test]
fn truncate_max_zero_does_not_panic() {
    assert_eq!(truncate("hello", 0), "…");
}

#[test]
fn summarize_trims_and_keeps_short_text() {
    assert_eq!(summarize("  hi there  "), "hi there");
}

#[test]
fn summarize_truncates_long_text_with_ellipsis() {
    let long = "x".repeat(200);
    let out = summarize(&long);
    assert_eq!(out.chars().count(), 81);
    assert!(out.ends_with('…'));
}

#[test]
fn month_abbr_maps_known_months() {
    assert_eq!(month_abbr(1), "Jan");
    assert_eq!(month_abbr(12), "Dec");
}

#[test]
fn month_abbr_falls_back_for_invalid() {
    assert_eq!(month_abbr(0), "???");
    assert_eq!(month_abbr(13), "???");
}

#[test]
fn strength_label_maps_thresholds() {
    assert_eq!(strength_label(2.0), "Strong");
    assert_eq!(strength_label(1.5), "Linked");
    assert_eq!(strength_label(1.49), "Weak");
    assert_eq!(strength_label(0.0), "Weak");
}

#[test]
fn plural_empty_for_one_s_otherwise() {
    assert_eq!(plural(1), "");
    assert_eq!(plural(0), "s");
    assert_eq!(plural(2), "s");
}

#[test]
fn rel_time_just_now_for_recent() {
    assert_eq!(rel_time(Utc::now()), "just now");
}

#[test]
fn rel_time_days_and_years() {
    assert_eq!(rel_time(Utc::now() - chrono::Duration::days(3)), "3d ago");
    assert_eq!(rel_time(Utc::now() - chrono::Duration::days(400)), "1y ago");
}

#[test]
fn parse_duration_mins_handles_hours_minutes_and_bare() {
    assert_eq!(parse_duration_mins("2h"), Some(120));
    assert_eq!(parse_duration_mins("1h 30m"), Some(90));
    assert_eq!(parse_duration_mins("45m"), Some(45));
    assert_eq!(parse_duration_mins("45"), Some(45));
    assert_eq!(parse_duration_mins("  "), None);
    assert_eq!(parse_duration_mins("abc"), None);
}

fn seed_memory(conn: &rusqlite::Connection) -> crate::infrastructure::model::Item {
    crate::test_support::seed_memory(conn, "t", "body", &[])
}

#[test]
fn memory_labels_use_kind_and_display_id() {
    let conn = crate::infrastructure::db::open_in_memory_for_test();
    let m = seed_memory(&conn);
    let id = m.display_id.unwrap();
    assert_eq!(item_label(&m), format!("m{id}"));
    assert_eq!(memory_handle(&m), format!("m{id}"));
}

#[test]
fn derived_graph_walks_both_directions() {
    let conn = crate::infrastructure::db::open_in_memory_for_test();
    let canon = seed_memory(&conn);
    let child = seed_memory(&conn);
    crate::test_support::link(&conn, child.uuid, "derived_from", canon.uuid);
    assert_eq!(derived_count(&conn, &canon.uuid.to_string()), 1);
    assert_eq!(
        derived_children(&conn, &canon.uuid.to_string())[0].uuid,
        child.uuid
    );
    let (children, parents) = canonical_labels(&conn, &child);
    assert!(children.is_empty());
    assert_eq!(parents, vec![memory_handle(&canon)]);
}

#[test]
fn guard_allows_uuid_and_forced_mutations() {
    let conn = crate::infrastructure::db::open_in_memory_for_test();
    let mut task = crate::infrastructure::model::Task::new("t".into(), "p".into());
    crate::infrastructure::db::insert_task(&conn, &mut task).unwrap();
    assert!(guard_branch_mutation(&conn, &task.uuid.to_string(), &task, false).is_ok());
    assert!(guard_branch_mutation(&conn, "1", &task, true).is_ok());
    assert!(project_head(&conn, "no-such-project").is_none());
}

#[test]
fn json_strs_extracts_strings_and_tolerates_missing() {
    let v = serde_json::json!({ "a": ["x", 1, "y"] });
    assert_eq!(json_strs(&v["a"]), vec!["x", "y"]);
    assert!(json_strs(&v["missing"]).is_empty());
}

#[test]
fn derived_from_suffix_empty_or_joined() {
    assert_eq!(derived_from_suffix(&[]), "");
    assert_eq!(
        derived_from_suffix(&["m1".into(), "m2".into()]),
        " [derived from: m1, m2]"
    );
}

#[test]
fn item_snippet_prefers_summary_and_caps_chars() {
    let conn = crate::infrastructure::db::open_in_memory_for_test();
    let mut m = seed_memory(&conn);
    assert_eq!(item_snippet(&m, 2), "bo");
    m.summary = Some("summary".into());
    assert_eq!(item_snippet(&m, 3), "sum");
    assert_eq!(short_handle(&m), memory_handle(&m));
}

#[test]
fn scroll_into_view_keeps_line_visible() {
    use crate::infrastructure::tui::scroll_into_view;
    let mut s = 5;
    scroll_into_view(&mut s, 2, 10);
    assert_eq!(s, 2);
    scroll_into_view(&mut s, 20, 10);
    assert_eq!(s, 11);
    scroll_into_view(&mut s, 15, 10);
    assert_eq!(s, 11);
}

#[test]
fn split_csv_trims_and_drops_empty_entries() {
    assert_eq!(split_csv(" a , ,b ,"), ["a", "b"]);
    assert!(split_csv("").is_empty());
}

#[test]
fn normalize_list_trims_and_drops_empty_entries() {
    let input = [" a ".to_string(), "  ".to_string(), "b".to_string()];
    assert_eq!(normalize_list(&input), ["a", "b"]);
}

#[test]
fn short_id_takes_eight_chars_not_bytes() {
    assert_eq!(short_id("0123456789abcdef"), "01234567");
    assert_eq!(short_id("åbc"), "åbc");
    assert_eq!(short_id("ååååååååå"), "åååååååå");
}

#[test]
fn resolve_files_keeps_order_and_length() {
    let input = ["b.rs".to_string(), "a.rs".to_string()];
    let out = resolve_files(&input);
    assert_eq!(out.len(), 2);
    assert!(out[0].ends_with("b.rs") && out[1].ends_with("a.rs"));
}

mod cite {
    use crate::commands::shared::{cited_memories, with_citation};
    use crate::infrastructure::db;
    use crate::infrastructure::model::{Item, Task};

    fn task(conn: &rusqlite::Connection) -> Task {
        let mut t = Task::new("ship it".to_string(), "Sara".to_string());
        db::insert_task(conn, &mut t).unwrap();
        t
    }

    fn label(item: &Item, prefix: char) -> String {
        format!("{prefix}{}", item.display_id.unwrap())
    }

    #[test]
    fn cite_records_each_memory_once_and_reports_handles() {
        let conn = db::open_in_memory_for_test();
        let t = task(&conn);
        let m = crate::test_support::seed_memory(&conn, "retry", "jittered backoff", &[]);
        let h = label(&m, 'm');

        let v = with_citation(&conn, &t.uuid.to_string(), &[h.clone(), h.clone()], || {
            Ok(serde_json::json!({}))
        })
        .unwrap();

        assert_eq!(v["cited"], serde_json::json!([h]));
        let cited = cited_memories(&conn, &t.uuid);
        assert_eq!(
            cited.iter().map(|i| i.uuid).collect::<Vec<_>>(),
            vec![m.uuid]
        );
    }

    #[test]
    fn cite_rejects_a_non_memory_label_before_running_the_command() {
        let conn = db::open_in_memory_for_test();
        let t = task(&conn);
        let m = crate::test_support::seed_memory(&conn, "retry", "jittered backoff", &[]);
        let mut note = Item::new_note("a note".to_string(), "not a memory".to_string());
        note.path = Some(String::new());
        db::insert_item(&conn, &mut note).unwrap();

        let mut ran = false;
        let err = with_citation(
            &conn,
            &t.uuid.to_string(),
            &[label(&m, 'm'), label(&note, 'n'), "m9999".to_string()],
            || {
                ran = true;
                Ok(serde_json::json!({}))
            },
        )
        .unwrap_err()
        .to_string();

        assert!(!ran, "the command must not run on a bad label");
        assert!(
            err.contains(&label(&note, 'n')) && err.contains("m9999"),
            "{err}"
        );
        assert!(
            cited_memories(&conn, &t.uuid).is_empty(),
            "nothing is recorded"
        );
    }

    #[test]
    fn cite_records_nothing_when_the_command_fails() {
        let conn = db::open_in_memory_for_test();
        let t = task(&conn);
        let m = crate::test_support::seed_memory(&conn, "retry", "jittered backoff", &[]);

        let res = with_citation(&conn, &t.uuid.to_string(), &[label(&m, 'm')], || {
            anyhow::bail!("step out of range")
        });

        assert!(res.is_err());
        assert!(cited_memories(&conn, &t.uuid).is_empty());
    }

    #[test]
    fn cite_without_labels_is_a_no_op() {
        let conn = db::open_in_memory_for_test();
        let v = with_citation(&conn, "does-not-matter", &[], || Ok(serde_json::json!({}))).unwrap();
        assert!(v.get("cited").is_none());
    }
}
