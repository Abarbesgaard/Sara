use super::*;
use crate::infrastructure::model::{Priority, Status, Task};
use uuid::Uuid;

fn sample() -> Task {
    Task {
        uuid: Uuid::new_v4(),
        id: Some(1),
        description: "orig".into(),
        project: "p".into(),
        status: Status::Pending,
        priority: None,
        due: None,
        entry: Utc::now(),
        modified: Utc::now(),
        end: None,
        tags: vec!["old".into()],
        urgency: 0.0,
        started_at: None,
        time_spent: 0,
        estimate_mins: None,
        recur: None,
    }
}

#[test]
fn sets_description_priority_and_replaces_tags() {
    let cfg = Config::default();
    let t = merge_task_fields(
        sample(),
        &cfg,
        Some("new desc"),
        Some("h"),
        None,
        false,
        &["a".into(), "b".into()],
        false,
        None,
        false,
        None,
        false,
    )
    .unwrap();
    assert_eq!(t.description, "new desc");
    assert_eq!(t.priority, Some(Priority::H));
    assert_eq!(t.tags, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn clear_tags_and_clear_due_unset_fields() {
    let cfg = Config::default();
    let mut base = sample();
    base.due = Some(Utc::now());
    let t = merge_task_fields(
        base,
        &cfg,
        None,
        None,
        None,
        true,
        &[],
        true,
        None,
        false,
        None,
        false,
    )
    .unwrap();
    assert!(t.tags.is_empty());
    assert!(t.due.is_none());
}

#[test]
fn invalid_priority_is_rejected() {
    let cfg = Config::default();
    assert!(
        merge_task_fields(
            sample(),
            &cfg,
            None,
            Some("X"),
            None,
            false,
            &[],
            false,
            None,
            false,
            None,
            false
        )
        .is_err()
    );
}

#[test]
fn invalid_due_is_rejected() {
    let cfg = Config::default();
    assert!(
        merge_task_fields(
            sample(),
            &cfg,
            None,
            None,
            Some("not-a-date"),
            false,
            &[],
            false,
            None,
            false,
            None,
            false
        )
        .is_err()
    );
}

#[test]
fn unspecified_fields_are_left_unchanged() {
    let cfg = Config::default();
    let t = merge_task_fields(
        sample(),
        &cfg,
        None,
        None,
        None,
        false,
        &[],
        false,
        None,
        false,
        None,
        false,
    )
    .unwrap();
    assert_eq!(t.description, "orig");
    assert_eq!(t.priority, None);
    assert_eq!(t.tags, vec!["old".to_string()]);
}

#[test]
fn sets_estimate_and_recur() {
    let cfg = Config::default();
    let t = merge_task_fields(
        sample(),
        &cfg,
        None,
        None,
        None,
        false,
        &[],
        false,
        Some("2h30m"),
        false,
        Some("weekly"),
        false,
    )
    .unwrap();
    assert_eq!(t.estimate_mins, Some(150));
    assert_eq!(t.recur, Some("weekly".to_string()));
}

#[test]
fn clear_estimate_and_clear_recur_unset_fields() {
    let cfg = Config::default();
    let mut base = sample();
    base.estimate_mins = Some(90);
    base.recur = Some("daily".into());
    let t = merge_task_fields(
        base,
        &cfg,
        None,
        None,
        None,
        false,
        &[],
        false,
        None,
        true,
        None,
        true,
    )
    .unwrap();
    assert!(t.estimate_mins.is_none());
    assert!(t.recur.is_none());
}

#[test]
fn invalid_estimate_is_rejected() {
    let cfg = Config::default();
    assert!(
        merge_task_fields(
            sample(),
            &cfg,
            None,
            None,
            None,
            false,
            &[],
            false,
            Some("not-a-duration"),
            false,
            None,
            false
        )
        .is_err()
    );
}

#[test]
fn form_edit_persists_estimate_and_adds_only_new_links() {
    let conn = crate::infrastructure::db::open_in_memory_for_test();
    let cfg = Config::default();
    let task = crate::test_support::seed_task(&conn, "Edit me", "p");
    crate::infrastructure::db::add_link(&conn, &task.uuid, "https://x.dev/1", None).unwrap();

    let form = FormInput {
        description: "Edit me".into(),
        project: "p".into(),
        estimate: "45m".into(),
        links: "https://x.dev/1 https://x.dev/2".into(),
        ..Default::default()
    };
    let updated = apply_form(&conn, &cfg, &task, form).unwrap();

    assert_eq!(updated.estimate_mins, Some(45));
    let stored = crate::infrastructure::db::resolve_task(&conn, &task.uuid.to_string()).unwrap();
    assert_eq!(stored.estimate_mins, Some(45));
    let urls: Vec<String> = crate::infrastructure::db::get_links(&conn, &task.uuid)
        .unwrap()
        .into_iter()
        .map(|l| l.url)
        .collect();
    assert_eq!(
        urls,
        vec!["https://x.dev/1".to_string(), "https://x.dev/2".to_string()]
    );
}
