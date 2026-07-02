//! Integration tests for `sara_tasks::infrastructure::portable`.
//! Moved out of an inline mod tests block in src/infrastructure/portable.rs.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::Utc;
use sara_tasks::infrastructure::model::{Priority, Status};
use sara_tasks::infrastructure::portable::*;
use uuid::Uuid;

fn sample() -> Bundle {
    let root = Uuid::new_v4();
    let dep = Uuid::new_v4();
    Bundle {
        format: BUNDLE_FORMAT.into(),
        version: BUNDLE_VERSION,
        exported_at: Utc::now(),
        root,
        tasks: vec![
            TaskEnvelope {
                uuid: dep,
                description: "blocker".into(),
                project: "web".into(),
                status: Status::Pending,
                priority: Some(Priority::L),
                due: None,
                entry: Utc::now(),
                tags: vec!["infra".into()],
                estimate_mins: None,
                recur: None,
                blocked_by: vec![],
                annotations: vec![],
                checklist: vec![],
                links: vec![],
                files: vec![],
            },
            TaskEnvelope {
                uuid: root,
                description: "root task".into(),
                project: "web".into(),
                status: Status::Pending,
                priority: Some(Priority::H),
                due: None,
                entry: Utc::now(),
                tags: vec![],
                estimate_mins: Some(30),
                recur: None,
                blocked_by: vec![dep],
                annotations: vec![AnnotationDto {
                    text: "note".into(),
                    kind: "comment".into(),
                    author: "human".into(),
                    target_kind: None,
                    target_id: None,
                }],
                checklist: vec![ChecklistDto {
                    text: "do the thing".into(),
                    done: true,
                    kind: "step".into(),
                    source: "human".into(),
                    intent: None,
                    verify_cmd: Some("cargo test".into()),
                    result: Some("ok".into()),
                    done_commit: Some("abc123".into()),
                }],
                links: vec![LinkDto {
                    url: "https://example.com/pr/1".into(),
                    label: Some("PR 1".into()),
                }],
                files: vec![FileDto {
                    path: "src/main.rs".into(),
                    source: "manual".into(),
                }],
            },
        ],
    }
}

#[test]
fn round_trips_through_blob() {
    let b = sample();
    let blob = b.encode().unwrap();
    assert!(blob.starts_with(BLOB_PREFIX));
    let back = Bundle::decode(&blob).unwrap();
    assert_eq!(back.root, b.root);
    assert_eq!(back.tasks.len(), 2);
    assert_eq!(back.tasks[1].blocked_by, vec![b.tasks[0].uuid]);
    assert_eq!(
        back.tasks[1].checklist[0].verify_cmd.as_deref(),
        Some("cargo test")
    );
}

#[test]
fn decode_tolerates_whitespace_and_missing_prefix() {
    let blob = sample().encode().unwrap();
    let raw = blob.strip_prefix(BLOB_PREFIX).unwrap();
    // Re-wrap with newlines and leading spaces, drop the prefix.
    let wrapped: String = raw
        .as_bytes()
        .chunks(20)
        .map(|c| format!("  {}\n", std::str::from_utf8(c).unwrap()))
        .collect();
    let back = Bundle::decode(&wrapped).unwrap();
    assert_eq!(back.tasks.len(), 2);
}

#[test]
fn rejects_non_bundle_base64() {
    let junk = format!("{BLOB_PREFIX}{}", B64.encode(b"{\"hello\":1}"));
    assert!(Bundle::decode(&junk).is_err());
}

#[test]
fn rejects_future_version() {
    let mut b = sample();
    b.version = BUNDLE_VERSION + 1;
    let blob = b.encode().unwrap();
    let err = Bundle::decode(&blob).unwrap_err().to_string();
    assert!(err.contains("upgrade sara"), "{err}");
}
