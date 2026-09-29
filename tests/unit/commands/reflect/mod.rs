use crate::infrastructure::{db, model::Item};
use uuid::Uuid;

fn insert_memory(conn: &rusqlite::Connection, body: &str, tag: &str) -> Uuid {
    let mut item = Item::new_memory(body.to_string(), body.to_string(), None);
    item.tags = vec![tag.to_string()];
    item.path = Some(String::new());
    db::insert_item(conn, &mut item).unwrap();
    item.uuid
}

fn link(conn: &rusqlite::Connection, from: &Uuid, to: &Uuid, relation: &str) {
    db::insert_memory_link(conn, &from.to_string(), &to.to_string(), relation, 1.0).unwrap();
}

fn weighted_link(conn: &rusqlite::Connection, from: &Uuid, to: &Uuid, weight: f64) {
    db::insert_memory_link(
        conn,
        &from.to_string(),
        &to.to_string(),
        "similar_to",
        weight,
    )
    .unwrap();
}

fn cluster_sizes(v: &serde_json::Value) -> Vec<usize> {
    let mut sizes: Vec<usize> = v["clusters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["members"].as_array().unwrap().len())
        .collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes
}

#[test]
fn chained_mega_component_is_split_at_its_weakest_links() {
    let conn = db::open_in_memory_for_test();
    let m: Vec<Uuid> = (0..12)
        .map(|i| insert_memory(&conn, &format!("memory {i}"), &format!("t{i}")))
        .collect();
    for w in m[2..10].windows(2) {
        weighted_link(&conn, &w[0], &w[1], 1.0);
    }
    for tri in [&m[0..3], &m[9..12]] {
        weighted_link(&conn, &tri[0], &tri[1], 2.0);
        weighted_link(&conn, &tri[1], &tri[2], 2.0);
        weighted_link(&conn, &tri[0], &tri[2], 2.0);
    }

    let unsplit = super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, 0).unwrap();
    assert_eq!(cluster_sizes(&unsplit), vec![12], "0 disables splitting");

    let v = super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, 4).unwrap();
    assert_eq!(cluster_sizes(&v), vec![3, 3]);
}

#[test]
fn component_within_max_size_is_not_split() {
    let conn = db::open_in_memory_for_test();
    let m: Vec<Uuid> = (0..4)
        .map(|i| insert_memory(&conn, &format!("memory {i}"), &format!("t{i}")))
        .collect();
    weighted_link(&conn, &m[0], &m[1], 2.0);
    weighted_link(&conn, &m[1], &m[2], 1.0);
    weighted_link(&conn, &m[2], &m[3], 1.0);
    let v = super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, 4).unwrap();
    assert_eq!(cluster_sizes(&v), vec![4]);
}

#[test]
fn fresh_related_cluster_is_proposed_with_a_canonical() {
    let conn = db::open_in_memory_for_test();
    let a = insert_memory(
        &conn,
        "dependabot bump broke restore in repo a",
        "dependabot",
    );
    let b = insert_memory(
        &conn,
        "dependabot bump broke restore in repo b",
        "dependabot",
    );
    let c = insert_memory(
        &conn,
        "dependabot bump broke restore in repo c",
        "dependabot",
    );
    link(&conn, &a, &b, "similar_to");
    link(&conn, &b, &c, "similar_to");

    let v =
        super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(v["count"].as_u64().unwrap(), 1, "one cluster expected");
    let cluster = &v["clusters"][0];
    assert_eq!(cluster["members"].as_array().unwrap().len(), 3);
    assert!(!cluster["suggested_canonical"].as_str().unwrap().is_empty());
    assert_eq!(cluster["proposed_links"].as_array().unwrap().len(), 2);
    assert!(
        cluster["shared_tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "dependabot")
    );
}

#[test]
fn already_consolidated_cluster_is_excluded() {
    let conn = db::open_in_memory_for_test();
    let canonical = insert_memory(&conn, "canonical pattern", "dependabot");
    let child_a = insert_memory(&conn, "applied in repo a", "dependabot");
    let child_b = insert_memory(&conn, "applied in repo b", "dependabot");

    link(&conn, &child_a, &canonical, "derived_from");
    link(&conn, &child_b, &canonical, "derived_from");

    let v =
        super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(
        v["count"].as_u64().unwrap(),
        0,
        "a canonical + its derived children must not be re-proposed"
    );
}

#[test]
fn unrelated_memories_are_not_clustered() {
    let conn = db::open_in_memory_for_test();
    insert_memory(&conn, "memory about auth", "auth");
    insert_memory(&conn, "memory about billing", "billing");

    let v =
        super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(
        v["count"].as_u64().unwrap(),
        0,
        "no shared anchors -> no cluster"
    );
}

#[test]
fn cross_project_memories_do_not_cluster() {
    let conn = db::open_in_memory_for_test();
    let a = insert_memory(
        &conn,
        "dependabot bump broke restore in repo a",
        "dependabot",
    );
    let b = insert_memory(
        &conn,
        "dependabot bump broke restore in repo b",
        "dependabot",
    );
    db::set_item_projects(&conn, &a, &["repo-a".to_string()]).unwrap();
    db::set_item_projects(&conn, &b, &["repo-b".to_string()]).unwrap();
    db::insert_memory_link(&conn, &a.to_string(), &b.to_string(), "co_activated", 2.0).unwrap();

    let v =
        super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(
        v["count"].as_u64().unwrap(),
        0,
        "incidental co-firing must never merge memories across projects"
    );

    db::set_item_projects(&conn, &b, &["repo-a".to_string()]).unwrap();
    let v2 =
        super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(
        v2["count"].as_u64().unwrap(),
        1,
        "same-project related memories still cluster"
    );
}

#[test]
fn deliberate_link_clusters_across_projects() {
    let conn = db::open_in_memory_for_test();
    let a = insert_memory(&conn, "NU1608 NSubstitute fix pattern", "nsubstitute");
    let b = insert_memory(&conn, "applied NU1608 fix in other repo", "nsubstitute");
    db::set_item_projects(&conn, &a, &["repo-a".to_string()]).unwrap();
    db::set_item_projects(&conn, &b, &["repo-b".to_string()]).unwrap();
    link(&conn, &a, &b, "similar_to");

    let v =
        super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(
        v["count"].as_u64().unwrap(),
        1,
        "a deliberately-linked shared lesson still consolidates across repos"
    );
}

fn derived_from_count(conn: &rusqlite::Connection, uuids: &[&Uuid]) -> usize {
    uuids
        .iter()
        .map(|u| {
            db::get_memory_links_from(conn, &u.to_string())
                .unwrap_or_default()
                .into_iter()
                .filter(|l| l.relation == "derived_from")
                .count()
        })
        .sum()
}

#[test]
fn reflect_apply_creates_derived_links() {
    let conn = db::open_in_memory_for_test();
    let a = insert_memory(&conn, "dependabot bump repo a", "dependabot");
    let b = insert_memory(&conn, "dependabot bump repo b", "dependabot");
    let c = insert_memory(&conn, "dependabot bump repo c", "dependabot");
    link(&conn, &a, &b, "similar_to");
    link(&conn, &b, &c, "similar_to");

    let v =
        super::apply_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(
        v["applied"].as_u64().unwrap(),
        2,
        "two children linked to the canonical"
    );
    assert!(
        v["skipped"].as_array().unwrap().is_empty(),
        "nothing skipped"
    );

    assert_eq!(derived_from_count(&conn, &[&a, &b, &c]), 2);
}

#[test]
fn reflect_apply_is_idempotent_and_excludes_after() {
    let conn = db::open_in_memory_for_test();
    let a = insert_memory(&conn, "dependabot bump repo a", "dependabot");
    let b = insert_memory(&conn, "dependabot bump repo b", "dependabot");
    let c = insert_memory(&conn, "dependabot bump repo c", "dependabot");
    link(&conn, &a, &b, "similar_to");
    link(&conn, &b, &c, "similar_to");

    let first =
        super::apply_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(first["applied"].as_u64().unwrap(), 2);

    let second =
        super::apply_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(second["applied"].as_u64().unwrap(), 0, "no re-application");
    assert!(second["skipped"].as_array().unwrap().is_empty());

    assert_eq!(
        derived_from_count(&conn, &[&a, &b, &c]),
        2,
        "no duplicate links"
    );
    let proposal =
        super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(
        proposal["count"].as_u64().unwrap(),
        0,
        "consolidated cluster excluded"
    );
}

#[test]
fn reflect_apply_skips_cycle_violations() {
    let conn = db::open_in_memory_for_test();
    let a = insert_memory(&conn, "dependabot bump repo a", "dependabot");
    let b = insert_memory(&conn, "dependabot bump repo b", "dependabot");
    let c = insert_memory(&conn, "dependabot bump repo c", "dependabot");
    link(&conn, &a, &b, "similar_to");
    link(&conn, &b, &c, "similar_to");
    link(&conn, &a, &b, "derived_from");
    let mut task =
        crate::infrastructure::model::Task::new("completed task".to_string(), "Sara".to_string());
    task.status = crate::infrastructure::model::Status::Completed;
    db::insert_task(&conn, &mut task).unwrap();
    db::set_item_task_links(&conn, &a, &[(task.uuid, "explicit")]).unwrap();

    let v =
        super::apply_value(&conn, super::DEFAULT_MIN_WEIGHT, super::DEFAULT_MAX_CLUSTER).unwrap();
    assert_eq!(
        v["applied"].as_u64().unwrap(),
        1,
        "only the safe link applies"
    );
    let skipped = v["skipped"].as_array().unwrap();
    assert_eq!(
        skipped.len(),
        1,
        "the cycle-forming link is skipped, not fatal"
    );
    assert!(
        skipped[0]["reason"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("cycle"),
        "skip reason names the cycle: {:?}",
        skipped[0]["reason"]
    );

    let c_links = db::get_memory_links_from(&conn, &c.to_string()).unwrap();
    assert!(
        c_links
            .iter()
            .any(|l| l.relation == "derived_from" && l.to_uuid == a.to_string())
    );
    let b_links = db::get_memory_links_from(&conn, &b.to_string()).unwrap();
    assert!(
        !b_links
            .iter()
            .any(|l| l.relation == "derived_from" && l.to_uuid == a.to_string())
    );
}
