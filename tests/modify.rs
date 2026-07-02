//! Integration tests for `sara_tasks::commands::modify`.
//! Moved out of an inline mod tests block in src/commands/modify/mod.rs.

use chrono::Utc;
use sara_tasks::commands::modify::merge_task_fields;
use sara_tasks::infrastructure::config::Config;
use sara_tasks::infrastructure::model::{Priority, Status, Task};
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
    let t = merge_task_fields(base, &cfg, None, None, None, true, &[], true).unwrap();
    assert!(t.tags.is_empty());
    assert!(t.due.is_none());
}

#[test]
fn invalid_priority_is_rejected() {
    let cfg = Config::default();
    assert!(merge_task_fields(sample(), &cfg, None, Some("X"), None, false, &[], false).is_err());
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
            false
        )
        .is_err()
    );
}

#[test]
fn unspecified_fields_are_left_unchanged() {
    let cfg = Config::default();
    let t = merge_task_fields(sample(), &cfg, None, None, None, false, &[], false).unwrap();
    assert_eq!(t.description, "orig");
    assert_eq!(t.priority, None);
    assert_eq!(t.tags, vec!["old".to_string()]);
}
