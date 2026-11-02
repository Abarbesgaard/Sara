use super::*;
use crate::domain::AnchorState;

fn body(lines: std::ops::Range<usize>) -> String {
    lines
        .map(|i| format!("let value_{i} = compute({i});"))
        .collect::<Vec<_>>()
        .join("\n")
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
    use crate::domain::Item;
    use crate::infrastructure::db;

    let conn = db::testing::open_in_memory_for_test();
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
