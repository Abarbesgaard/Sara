use super::LearnRequest;
use crate::infrastructure::util::safety;

#[test]
fn size_check_passes_under_threshold() {
    assert!(safety::check_size("short text").is_ok());
    assert!(safety::check_size(&"x".repeat(safety::SIZE_LIMIT_CHARS)).is_ok());
}

#[test]
fn size_check_fails_over_threshold() {
    let long = "x".repeat(safety::SIZE_LIMIT_CHARS + 1);
    assert!(safety::check_size(&long).is_err());
}

#[test]
fn secret_check_flags_api_key_assignment() {
    assert!(safety::detect_secret("api_key=abc123secret").is_some());
    assert!(safety::detect_secret("apikey: supersecret").is_some());
    assert!(safety::detect_secret("client_secret: my-secret-value").is_some());
}

#[test]
fn secret_check_does_not_flag_normal_prose() {
    assert!(safety::detect_secret("sara recall uses item_tags for exact tag lookup").is_none());
    assert!(safety::detect_secret("the token field is optional").is_none());
    assert!(safety::detect_secret("AuthAppUri (required string)").is_none());
}

#[test]
fn secret_check_flags_aws_style_key() {
    assert!(safety::detect_secret("key: AKIAIOSFODNN7EXAMPLE").is_some());
}

#[test]
fn secret_check_flags_high_entropy_hex_token() {
    assert!(
        safety::detect_secret(
            "token: a3f1b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2"
        )
        .is_some()
    );
}

#[test]
fn secret_check_does_not_flag_uuid_in_prose() {
    assert!(
        safety::detect_secret(
            "AuthAppUri is a UUID like a2923ccd-a496-4cbd-9673-f40156552a92 in the config"
        )
        .is_none()
    );
}

#[test]
fn learn_task_prefers_display_id_over_uuid_collision() {
    use crate::infrastructure::{config::Config, db, model::Task};
    use uuid::Uuid;

    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let mut task_a = Task::new("intended target".into(), "proj".into());
    task_a.uuid = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
    db::insert_task(&conn, &mut task_a).unwrap();
    let a_id = task_a.id.unwrap();

    let mut task_b = Task::new("unrelated task".into(), "proj".into());
    task_b.uuid = Uuid::parse_str("1bbbbbbb-0000-0000-0000-000000000002").unwrap();
    db::insert_task(&conn, &mut task_b).unwrap();

    let v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "finding tied to task A",
            tags: &["tag-r".to_string()],
            tasks: &[a_id.to_string()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();

    let item = db::get_item_by_handle(&conn, v["label"].as_str().unwrap()).unwrap();
    let linked = db::get_item_task_links(&conn, &item.uuid).unwrap();
    assert_eq!(linked.len(), 1, "expected exactly one task link");
    assert_eq!(
        linked[0].0.uuid, task_a.uuid,
        "memory linked to the wrong task (uuid-prefix collision instead of display id)"
    );
}

#[test]
fn file_overlaps_detects_tagless_memory_sharing_a_file() {
    use crate::infrastructure::{config::Config, db};
    use std::collections::HashSet;
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    let path = "/repo/src/auth.rs".to_string();

    super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "auth finding",
            files: std::slice::from_ref(&path),
            force: true,
            ..Default::default()
        },
    )
    .unwrap();

    let overlaps =
        super::overlap::file_overlaps(&conn, std::slice::from_ref(&path), &HashSet::new()).unwrap();
    assert_eq!(
        overlaps.len(),
        1,
        "a tagless memory sharing the file must still be detected"
    );
    assert_eq!(overlaps[0].0, path);
}

#[test]
fn learn_value_supersedes_inserts_link() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let old = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "old finding",
            tags: &["tag-a".to_string()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    let old_label = old["label"].as_str().unwrap().to_string();

    let new_v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "new finding supersedes old",
            tags: &["tag-a".to_string()],
            force: true,
            supersedes: std::slice::from_ref(&old_label),
            ..Default::default()
        },
    )
    .unwrap();

    let superseded = new_v["superseded"].as_array().unwrap();
    assert_eq!(superseded.len(), 1);
    assert_eq!(superseded[0].as_str().unwrap(), old_label);
}

#[test]
fn learn_value_supersedes_a_canonical_does_not_orphan_or_auto_archive_children() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let canonical = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "canonical pattern",
            tags: &["pat".to_string()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    let canonical_label = canonical["label"].as_str().unwrap().to_string();

    super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "an application of the pattern",
            force: true,
            derived_from: std::slice::from_ref(&canonical_label),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        super::overlap::canonical_derived_count(
            &conn,
            &db::get_item_by_handle(&conn, &canonical_label)
                .unwrap()
                .uuid
        ),
        1,
        "canonical must have exactly one derived child before superseding"
    );

    let new_v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "corrected canonical pattern",
            tags: &["pat".to_string()],
            force: true,
            supersedes: std::slice::from_ref(&canonical_label),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        new_v["superseded"].as_array().unwrap()[0].as_str().unwrap(),
        canonical_label
    );
}

#[test]
fn canonical_derived_count_zero_for_a_plain_memory() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    let v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "plain note",
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    let label = v["label"].as_str().unwrap();
    let uuid = db::get_item_by_handle(&conn, label).unwrap().uuid;
    assert_eq!(super::overlap::canonical_derived_count(&conn, &uuid), 0);
}

#[test]
fn check_overlap_warns_on_file_overlap() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let file_path = "/tmp/test_sara_overlap_check.rs".to_string();

    super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "first memory about this file",
            tags: &["tag-x".to_string()],
            files: std::slice::from_ref(&file_path),
            force: true,
            ..Default::default()
        },
    )
    .unwrap();

    let result = super::overlap::check_overlap(
        &conn,
        &["tag-y".to_string()],
        std::slice::from_ref(&file_path),
        None,
    );
    assert!(
        result.is_ok(),
        "check_overlap should not error on file overlap"
    );
}

#[test]
fn learn_creates_typed_links() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let canon = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "canonical pattern",
            tags: &["pat".to_string()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    let canon_label = canon["label"].as_str().unwrap().to_string();

    let sibling = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "sibling note",
            tags: &["side".to_string()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    let sibling_label = sibling["label"].as_str().unwrap().to_string();

    let new_v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "applied specialisation of the pattern",
            tags: &["apply".to_string()],
            force: true,
            derived_from: std::slice::from_ref(&canon_label),
            similar_to: std::slice::from_ref(&sibling_label),
            ..Default::default()
        },
    )
    .unwrap();

    let derived = new_v["derived_from"].as_array().unwrap();
    assert_eq!(derived.len(), 1);
    assert_eq!(derived[0].as_str().unwrap(), canon_label);

    let similar = new_v["similar_to"].as_array().unwrap();
    assert_eq!(similar.len(), 1);
    assert_eq!(similar[0].as_str().unwrap(), sibling_label);

    let new_uuid = db::get_item_by_handle(&conn, new_v["label"].as_str().unwrap())
        .unwrap()
        .uuid
        .to_string();
    let out = db::get_memory_links_from(&conn, &new_uuid).unwrap();
    assert!(
        out.iter().any(|l| l.relation == "derived_from"),
        "derived_from edge exists"
    );
    assert!(
        out.iter().any(|l| l.relation == "similar_to"),
        "similar_to edge exists"
    );
}

#[test]
fn learn_unresolvable_link_warns_not_aborts() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "a memory with a dangling link",
            tags: &["solo".to_string()],
            force: true,
            derived_from: &["m9999".to_string()],
            ..Default::default()
        },
    );
    assert!(
        v.is_ok(),
        "unresolvable --derived-from must not abort the learn"
    );
    let v = v.unwrap();
    assert_eq!(v["derived_from"].as_array().unwrap().len(), 0);
}

#[test]
fn overlap_suggests_typed_link() {
    let near = super::overlap::near_dupe_suggestion("m26");
    assert!(
        near.contains("--derived-from m26"),
        "near-dupe offers --derived-from: {near}"
    );
    assert!(
        near.contains("--supersedes m26"),
        "near-dupe offers --supersedes: {near}"
    );

    let partial = super::overlap::partial_overlap_suggestion("m30");
    assert!(
        partial.contains("--similar-to m30"),
        "partial offers --similar-to: {partial}"
    );
    assert!(!partial.to_lowercase().contains("possible contradiction"));
}

#[test]
fn canonical_hint_names_derived_from_and_relearn() {
    let hint = super::overlap::canonical_hint("m7", 3);
    assert!(hint.contains("m7"));
    assert!(hint.contains("3 derived applications"));
    assert!(hint.contains("--derived-from m7"));
    assert!(hint.contains("sara relearn m7"));
}

#[test]
fn check_overlap_detects_canonical_via_derived_from_link() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let canonical = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "CodeQL config pattern",
            tags: &["codeql".into(), "config".into()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    let canonical_label = canonical["label"].as_str().unwrap().to_string();
    let canonical_uuid = db::get_item_by_handle(&conn, &canonical_label)
        .unwrap()
        .uuid;

    assert_eq!(
        super::overlap::canonical_derived_count(&conn, &canonical_uuid),
        0
    );

    super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "applied CodeQL config to repo X",
            tags: &["codeql".into()],
            force: true,
            derived_from: std::slice::from_ref(&canonical_label),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        super::overlap::canonical_derived_count(&conn, &canonical_uuid),
        1
    );

    super::overlap::check_overlap(&conn, &["codeql".into()], &[], None).unwrap();
}

#[test]
fn learn_auto_attaches_new_instance_to_canonical_pattern() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let canonical = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "CANONICAL nsubstitute dependabot restore fix",
            tags: &["nsubstitute".into(), "ci".into()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    let canon_label = canonical["label"].as_str().unwrap().to_string();
    let canon_uuid = db::get_item_by_handle(&conn, &canon_label).unwrap().uuid;

    super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "applied the nsubstitute pin to repo A",
            tags: &["nsubstitute".into()],
            force: true,
            derived_from: std::slice::from_ref(&canon_label),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        super::overlap::canonical_derived_count(&conn, &canon_uuid),
        1
    );

    let new_v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "applied the nsubstitute pin to repo B under warnings-as-errors",
            tags: &["nsubstitute".into(), "ci".into()],
            ..Default::default()
        },
    )
    .unwrap();

    let auto: Vec<String> = new_v["auto_derived_from"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(
        auto.contains(&canon_label),
        "new instance must auto-attach to canonical, got {auto:?}"
    );
    assert_eq!(
        super::overlap::canonical_derived_count(&conn, &canon_uuid),
        2,
        "canonical should now count the auto-attached instance"
    );
}

#[test]
fn learn_auto_attach_does_not_duplicate_explicit_derived_from() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let canonical = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "CANONICAL pattern",
            tags: &["p".into()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    let canon_label = canonical["label"].as_str().unwrap().to_string();
    let canon_uuid = db::get_item_by_handle(&conn, &canon_label).unwrap().uuid;
    super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "seed application",
            tags: &["p".into()],
            force: true,
            derived_from: std::slice::from_ref(&canon_label),
            ..Default::default()
        },
    )
    .unwrap();

    let new_v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "another application, explicitly linked",
            tags: &["p".into()],
            derived_from: std::slice::from_ref(&canon_label),
            ..Default::default()
        },
    )
    .unwrap();
    let explicit: Vec<String> = new_v["derived_from"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(explicit.contains(&canon_label));
    assert!(
        new_v["auto_derived_from"].as_array().unwrap().is_empty(),
        "explicit derived_from must suppress the auto-link for the same canonical"
    );
    assert_eq!(
        super::overlap::canonical_derived_count(&conn, &canon_uuid),
        2
    );
}

#[test]
fn learn_does_not_auto_attach_to_a_plain_non_canonical_overlap() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "a plain finding",
            tags: &["solo".into()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();

    let new_v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "another finding on the same topic",
            tags: &["solo".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        new_v["auto_derived_from"].as_array().unwrap().is_empty(),
        "auto-attach must fire only for canonical patterns, not plain overlaps"
    );
}

#[test]
fn learn_always_embeds_even_with_default_config() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    assert!(
        !cfg.recall.semantic,
        "guard: default config keeps the deprecated toggle off"
    );

    let v = super::learn_value(
        &conn,
        &cfg,
        &LearnRequest {
            text: "dependabot bump broke the restore step; pin the lockfile version",
            tags: &["ci".to_string()],
            force: true,
            ..Default::default()
        },
    )
    .unwrap();

    let item = db::get_item_by_handle(&conn, v["label"].as_str().unwrap()).unwrap();
    let stored = db::get_embedding(&conn, &item.uuid.to_string()).unwrap();
    assert!(
        stored.is_some(),
        "every learned memory must be embedded so semantic recall always \
             works, regardless of the deprecated recall.semantic toggle"
    );
}
