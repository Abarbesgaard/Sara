use super::*;
use crate::infrastructure::config::Config;
use crate::infrastructure::model::{Item, Task};

#[test]
fn tag_exact_memory_surfaces_with_full_body_as_canonical() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let long_body = "dependabot bumped NSubstitute 5.3.0->6.2.0; AutoFixture.AutoNSubstitute \
            4.18.1 caps it <6.0.0 -> NU1608. Revert NSubstitute to 5.3.0 across the test projects.";
    let mut mem = Item::new_memory("NSubstitute restore fault".into(), long_body.into(), None);
    mem.tags = vec!["nsubstitute".into()];
    mem.path = Some(String::new());
    db::insert_item(&conn, &mut mem).unwrap();
    db::set_item_tags(&conn, &mem.uuid, &["nsubstitute".into()]).unwrap();

    let hits = find_similar(
        &conn,
        &cfg,
        "restore fails on the dependabot bump",
        &["nsubstitute".to_string()],
        "proj",
        5,
    )
    .unwrap();

    let mem_hit = hits
        .iter()
        .find(|h| h["ref_kind"] == "memory")
        .expect("tag-exact memory must surface");
    assert_eq!(mem_hit["confidence"], "canonical");
    assert_eq!(
        mem_hit["body"].as_str().unwrap(),
        long_body,
        "the FULL body is emitted, not a snippet — no second recall needed"
    );
}

#[test]
fn task_hits_still_surface_as_snippet_pointers() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let mut prior = Task::new("fix the flaky payment retry logic".into(), "proj".into());
    db::insert_task(&conn, &mut prior).unwrap();

    let hits = find_similar(
        &conn,
        &cfg,
        "fix the flaky payment retry logic",
        &[],
        "proj",
        5,
    )
    .unwrap();
    assert!(
        hits.iter().any(|h| h["ref_kind"] == "task"),
        "a matching prior task still surfaces as a pointer"
    );
}

#[test]
fn same_project_memory_hits_rank_first_and_are_labeled() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let mut other = Item::new_memory("other province auth".into(), "cross body".into(), None);
    other.tags = vec!["auth".into()];
    other.project = Some("other".into());
    other.path = Some(String::new());
    db::insert_item(&conn, &mut other).unwrap();
    db::set_item_tags(&conn, &other.uuid, &["auth".into()]).unwrap();

    let mut local = Item::new_memory("local province auth".into(), "local body".into(), None);
    local.tags = vec!["auth".into()];
    local.project = Some("proj".into());
    local.path = Some(String::new());
    db::insert_item(&conn, &mut local).unwrap();
    db::set_item_tags(&conn, &local.uuid, &["auth".into()]).unwrap();

    let hits = find_similar(&conn, &cfg, "add auth", &["auth".to_string()], "proj", 5).unwrap();
    let mem_hits: Vec<&Value> = hits.iter().filter(|h| h["ref_kind"] == "memory").collect();
    assert!(mem_hits.len() >= 2, "both memories surface");
    assert_eq!(
        mem_hits[0]["same_project"], true,
        "local province prior art ranks first"
    );
    assert_eq!(mem_hits[0]["project"], "proj");
    assert!(
        mem_hits.iter().any(|h| h["same_project"] == false),
        "the cross-project hit is still surfaced, just labeled"
    );
}
