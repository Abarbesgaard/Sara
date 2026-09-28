use crate::infrastructure::safety;

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

    // Task A: the intended target. Its UUID deliberately does NOT start with "1".
    let mut task_a = Task::new("intended target".into(), "proj".into());
    task_a.uuid = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
    db::insert_task(&conn, &mut task_a).unwrap();
    let a_id = task_a.id.unwrap(); // display id 1

    // Task B: an unrelated task whose UUID starts with the same digit as A's
    // display id. A raw uuid-prefix lookup on "1" would wrongly match this.
    let mut task_b = Task::new("unrelated task".into(), "proj".into());
    task_b.uuid = Uuid::parse_str("1bbbbbbb-0000-0000-0000-000000000002").unwrap();
    db::insert_task(&conn, &mut task_b).unwrap();

    // Learn a memory with --task <A's display id>.
    let v = super::learn_value(
        &conn,
        &cfg,
        "finding tied to task A",
        &["tag-r".to_string()],
        &[],
        &[a_id.to_string()],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();

    // The memory must be linked to task A (by display id), never task B.
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

    // A memory saved with a file but NO tags — the exact case the overlap
    // check used to skip via an early return, hiding the file collision.
    super::learn_value(
        &conn,
        &cfg,
        "auth finding",
        &[],
        &[],
        &[],
        std::slice::from_ref(&path),
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();

    let overlaps =
        super::file_overlaps(&conn, std::slice::from_ref(&path), &HashSet::new()).unwrap();
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

    // Learn a first memory to supersede.
    let old = super::learn_value(
        &conn,
        &cfg,
        "old finding",
        &["tag-a".to_string()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();
    let old_label = old["label"].as_str().unwrap().to_string();

    // Learn a new memory that supersedes the old one.
    let new_v = super::learn_value(
        &conn,
        &cfg,
        "new finding supersedes old",
        &["tag-a".to_string()],
        &[],
        &[],
        &[],
        false,
        true,
        std::slice::from_ref(&old_label),
        &[],
        &[],
    )
    .unwrap();

    // The superseded array should contain the old label.
    let superseded = new_v["superseded"].as_array().unwrap();
    assert_eq!(superseded.len(), 1);
    assert_eq!(superseded[0].as_str().unwrap(), old_label);
}

#[test]
fn learn_value_supersedes_a_canonical_does_not_orphan_or_auto_archive_children() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    // Canonical memory.
    let canonical = super::learn_value(
        &conn,
        &cfg,
        "canonical pattern",
        &["pat".to_string()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();
    let canonical_label = canonical["label"].as_str().unwrap().to_string();

    // A derived child, linked via --derived-from.
    super::learn_value(
        &conn,
        &cfg,
        "an application of the pattern",
        &[],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        std::slice::from_ref(&canonical_label),
        &[],
    )
    .unwrap();
    assert_eq!(
        super::canonical_derived_count(
            &conn,
            &db::get_item_by_handle(&conn, &canonical_label)
                .unwrap()
                .uuid
        ),
        1,
        "canonical must have exactly one derived child before superseding"
    );

    // Superseding the canonical must succeed and must not touch the
    // derived child's status — it stays active (never auto-archived).
    let new_v = super::learn_value(
        &conn,
        &cfg,
        "corrected canonical pattern",
        &["pat".to_string()],
        &[],
        &[],
        &[],
        false,
        true,
        std::slice::from_ref(&canonical_label),
        &[],
        &[],
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
        "plain note",
        &[],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();
    let label = v["label"].as_str().unwrap();
    let uuid = db::get_item_by_handle(&conn, label).unwrap().uuid;
    assert_eq!(super::canonical_derived_count(&conn, &uuid), 0);
}

#[test]
fn check_overlap_warns_on_file_overlap() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let file_path = "/tmp/test_sara_overlap_check.rs".to_string();

    // Learn first memory tied to the file.
    super::learn_value(
        &conn,
        &cfg,
        "first memory about this file",
        &["tag-x".to_string()],
        &[],
        &[],
        std::slice::from_ref(&file_path),
        false,
        true, // force — skip safety guardrails
        &[],
        &[],
        &[],
    )
    .unwrap();

    // Learning a second memory on the same file with DIFFERENT tags should
    // still succeed (file-overlap is advisory, not blocking). The test just
    // verifies check_overlap() itself doesn't error out.
    let result = super::check_overlap(
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

    // A canonical memory and a lateral one to link against.
    let canon = super::learn_value(
        &conn,
        &cfg,
        "canonical pattern",
        &["pat".to_string()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();
    let canon_label = canon["label"].as_str().unwrap().to_string();

    let sibling = super::learn_value(
        &conn,
        &cfg,
        "sibling note",
        &["side".to_string()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();
    let sibling_label = sibling["label"].as_str().unwrap().to_string();

    // Learn a new memory that is derived_from the canonical AND similar_to the sibling.
    let new_v = super::learn_value(
        &conn,
        &cfg,
        "applied specialisation of the pattern",
        &["apply".to_string()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        std::slice::from_ref(&canon_label),
        std::slice::from_ref(&sibling_label),
    )
    .unwrap();

    let derived = new_v["derived_from"].as_array().unwrap();
    assert_eq!(derived.len(), 1);
    assert_eq!(derived[0].as_str().unwrap(), canon_label);

    let similar = new_v["similar_to"].as_array().unwrap();
    assert_eq!(similar.len(), 1);
    assert_eq!(similar[0].as_str().unwrap(), sibling_label);

    // The typed edges must actually exist in the graph.
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

    // Reference a memory label that does not exist — learn must still succeed.
    let v = super::learn_value(
        &conn,
        &cfg,
        "a memory with a dangling link",
        &["solo".to_string()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &["m9999".to_string()],
        &[],
    );
    assert!(
        v.is_ok(),
        "unresolvable --derived-from must not abort the learn"
    );
    let v = v.unwrap();
    // The link was skipped, so the reported array is empty.
    assert_eq!(v["derived_from"].as_array().unwrap().len(), 0);
}

#[test]
fn overlap_suggests_typed_link() {
    // Near-duplicate → derived-from / supersedes; partial → similar-to.
    let near = super::near_dupe_suggestion("m26");
    assert!(
        near.contains("--derived-from m26"),
        "near-dupe offers --derived-from: {near}"
    );
    assert!(
        near.contains("--supersedes m26"),
        "near-dupe offers --supersedes: {near}"
    );

    let partial = super::partial_overlap_suggestion("m30");
    assert!(
        partial.contains("--similar-to m30"),
        "partial offers --similar-to: {partial}"
    );
    // No longer the vague untyped "possible contradiction" wording.
    assert!(!partial.to_lowercase().contains("possible contradiction"));
}

#[test]
fn canonical_hint_names_derived_from_and_relearn() {
    let hint = super::canonical_hint("m7", 3);
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

    // Canonical memory with two tags.
    let canonical = super::learn_value(
        &conn,
        &cfg,
        "CodeQL config pattern",
        &["codeql".into(), "config".into()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();
    let canonical_label = canonical["label"].as_str().unwrap().to_string();
    let canonical_uuid = db::get_item_by_handle(&conn, &canonical_label)
        .unwrap()
        .uuid;

    // Before any derived child exists, it isn't canonical yet.
    assert_eq!(super::canonical_derived_count(&conn, &canonical_uuid), 0);

    // A derived application, linked via --derived-from.
    super::learn_value(
        &conn,
        &cfg,
        "applied CodeQL config to repo X",
        &["codeql".into()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        std::slice::from_ref(&canonical_label),
        &[],
    )
    .unwrap();

    // Now the canonical has one derived child — the detection
    // `check_overlap` relies on to upgrade a partial-tag match into the
    // near-dupe band with a `--derived-from`/`relearn` hint.
    assert_eq!(super::canonical_derived_count(&conn, &canonical_uuid), 1);

    // A new memory sharing only one of the canonical's two tags (partial
    // overlap) must not error when check_overlap runs against it.
    super::check_overlap(&conn, &["codeql".into()], &[], None).unwrap();
}

#[test]
fn learn_auto_attaches_new_instance_to_canonical_pattern() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    // Canonical pattern memory with two tags.
    let canonical = super::learn_value(
        &conn,
        &cfg,
        "CANONICAL nsubstitute dependabot restore fix",
        &["nsubstitute".into(), "ci".into()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();
    let canon_label = canonical["label"].as_str().unwrap().to_string();
    let canon_uuid = db::get_item_by_handle(&conn, &canon_label).unwrap().uuid;

    // One explicit derived application turns it into a canonical.
    super::learn_value(
        &conn,
        &cfg,
        "applied the nsubstitute pin to repo A",
        &["nsubstitute".into()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        std::slice::from_ref(&canon_label),
        &[],
    )
    .unwrap();
    assert_eq!(super::canonical_derived_count(&conn, &canon_uuid), 1);

    // A NEW instance sharing all the canonical's tags, learned WITHOUT an
    // explicit --derived-from (force=false so overlap detection runs): it
    // must auto-attach to the canonical pattern.
    let new_v = super::learn_value(
        &conn,
        &cfg,
        "applied the nsubstitute pin to repo B under warnings-as-errors",
        &["nsubstitute".into(), "ci".into()],
        &[],
        &[],
        &[],
        false,
        false,
        &[],
        &[],
        &[],
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
        super::canonical_derived_count(&conn, &canon_uuid),
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
        "CANONICAL pattern",
        &["p".into()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();
    let canon_label = canonical["label"].as_str().unwrap().to_string();
    let canon_uuid = db::get_item_by_handle(&conn, &canon_label).unwrap().uuid;
    super::learn_value(
        &conn,
        &cfg,
        "seed application",
        &["p".into()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        std::slice::from_ref(&canon_label),
        &[],
    )
    .unwrap();

    // Author explicitly names the canonical: the explicit link wins, and the
    // auto-attach must NOT create a second, duplicate derived_from edge.
    let new_v = super::learn_value(
        &conn,
        &cfg,
        "another application, explicitly linked",
        &["p".into()],
        &[],
        &[],
        &[],
        false,
        false,
        &[],
        std::slice::from_ref(&canon_label),
        &[],
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
    // Exactly two applications (seed + this one), not three.
    assert_eq!(super::canonical_derived_count(&conn, &canon_uuid), 2);
}

#[test]
fn learn_does_not_auto_attach_to_a_plain_non_canonical_overlap() {
    use crate::infrastructure::{config::Config, db};
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    // A plain memory (no derived children) — not a pattern.
    super::learn_value(
        &conn,
        &cfg,
        "a plain finding",
        &["solo".into()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
    )
    .unwrap();

    // A near-duplicate by tag, but the overlap target is not canonical.
    let new_v = super::learn_value(
        &conn,
        &cfg,
        "another finding on the same topic",
        &["solo".into()],
        &[],
        &[],
        &[],
        false,
        false,
        &[],
        &[],
        &[],
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
    // The default config leaves the legacy `recall.semantic` toggle OFF.
    // Semantic *querying* is always enabled (SemanticOpts::from_cfg), so the
    // write side must ALWAYS embed too — otherwise recall silently degrades
    // to keyword-only in every project that never flipped the deprecated flag.
    let cfg = Config::default();
    assert!(
        !cfg.recall.semantic,
        "guard: default config keeps the deprecated toggle off"
    );

    let v = super::learn_value(
        &conn,
        &cfg,
        "dependabot bump broke the restore step; pin the lockfile version",
        &["ci".to_string()],
        &[],
        &[],
        &[],
        false,
        true,
        &[],
        &[],
        &[],
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
