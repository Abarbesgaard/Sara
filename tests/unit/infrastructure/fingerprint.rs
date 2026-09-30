use super::*;

fn body(lines: std::ops::Range<usize>) -> String {
    lines
        .map(|i| format!("let value_{i} = compute({i});"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn fingerprint_hash_is_stable_across_builds() {
    assert_eq!(fnv1a(b"hello"), 0xa430_d846_80aa_bd0b);
}

#[test]
fn fingerprint_sketch_ignores_blank_and_brace_lines() {
    let s = sketch("fn main() {\n\n    }\n  }\nlet x = 1;\n");
    assert_eq!(s.len(), 2, "only `fn main() {{` and `let x = 1;` count");
}

#[test]
fn fingerprint_sketch_is_bounded_and_round_trips() {
    let s = sketch(&body(0..1000));
    assert_eq!(s.len(), SKETCH_SIZE);
    assert_eq!(decode(&encode(&s)), s);
}

#[test]
fn fingerprint_drift_is_share_of_vanished_lines() {
    let original = body(0..100);
    let s = sketch(&original);
    assert_eq!(drift(&s, &original), 0.0);

    let half_rewritten = format!("{}\n{}", body(0..50), body(500..550));
    let d = drift(&s, &half_rewritten);
    assert!((d - 0.5).abs() < 1e-9, "got {d}");

    let reordered_with_additions = format!("{}\n{}\nnew_line();", body(50..100), body(0..50));
    assert_eq!(drift(&s, &reordered_with_additions), 0.0);
}

#[test]
fn fingerprint_anchor_state_classifies_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    let p = path.to_str().unwrap();
    std::fs::write(&path, body(0..20)).unwrap();
    let fp = fingerprint_file(p).expect("text file is fingerprinted");

    assert_eq!(anchor_state(p, Some(&fp)), AnchorState::Fresh);

    std::fs::write(&path, body(100..120)).unwrap();
    assert_eq!(anchor_state(p, Some(&fp)), AnchorState::Drifted(1.0));
    assert!(anchor_state(p, Some(&fp)).is_stale());

    assert_eq!(anchor_state(p, None), AnchorState::Unknown, "legacy anchor");

    std::fs::remove_file(&path).unwrap();
    assert_eq!(anchor_state(p, Some(&fp)), AnchorState::Missing);
    assert_eq!(anchor_state(p, None), AnchorState::Missing);

    let elsewhere = dir.path().join("gone-dir/b.rs");
    assert_eq!(
        anchor_state(elsewhere.to_str().unwrap(), Some(&fp)),
        AnchorState::Unknown,
        "an anchor into a directory absent on this machine is not flagged"
    );
}

#[test]
fn fingerprint_file_skips_unreadable_and_binary() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("x.bin");
    std::fs::write(&bin, [0xffu8, 0xfe, 0x00, 0x01]).unwrap();
    assert_eq!(fingerprint_file(bin.to_str().unwrap()), None);
    assert_eq!(
        fingerprint_file(dir.path().join("nope.rs").to_str().unwrap()),
        None
    );
}

#[test]
fn fingerprint_stored_by_set_item_files_and_refreshed() {
    use crate::infrastructure::db;
    use crate::infrastructure::model::Item;

    let conn = db::open_in_memory_for_test();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("anchored.rs");
    let p = path.to_str().unwrap().to_string();
    std::fs::write(&path, body(0..30)).unwrap();

    let mut item = Item::new_memory("m".into(), "body".into(), None);
    item.path = Some(String::new());
    db::insert_item(&conn, &mut item).unwrap();
    let missing = dir.path().join("never-existed.rs");
    db::set_item_files(
        &conn,
        &item.uuid,
        &[p.clone(), missing.to_str().unwrap().to_string()],
    )
    .unwrap();

    let stored = db::get_item_file_fingerprints(&conn, &item.uuid).unwrap();
    let fp_of = |rows: &[(String, Option<Vec<u8>>)], f: &str| {
        rows.iter().find(|(x, _)| x == f).unwrap().1.clone()
    };
    let first = fp_of(&stored, &p).expect("readable file is fingerprinted");
    assert_eq!(fp_of(&stored, missing.to_str().unwrap()), None);

    std::fs::write(&path, body(200..230)).unwrap();
    assert!(anchor_state(&p, Some(&first)).is_stale());

    db::refresh_item_file_fingerprints(&conn, &item.uuid).unwrap();
    let refreshed = fp_of(
        &db::get_item_file_fingerprints(&conn, &item.uuid).unwrap(),
        &p,
    )
    .unwrap();
    assert_ne!(refreshed, first);
    assert_eq!(anchor_state(&p, Some(&refreshed)), AnchorState::Fresh);

    let all = db::all_active_item_file_fingerprints(&conn).unwrap();
    assert_eq!(all.len(), 2);
}
