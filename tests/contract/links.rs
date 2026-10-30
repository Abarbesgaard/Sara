use serde_json::json;

use crate::harness::Sara;

#[test]
fn link_then_unlink_round_trips_through_info() {
    let s = Sara::new();
    s.run(&["add", "-y", "Ship it"]);

    s.run(&["link", "1", "https://example.com/pr/1", "--label", "PR"]);
    assert_eq!(
        s.json(&["info", "1", "--json"])["links"],
        json!([{ "id": 1, "label": "PR", "url": "https://example.com/pr/1" }]),
    );

    s.run(&["unlink", "1"]);
    assert_eq!(s.json(&["info", "1", "--json"])["links"], json!([]));
}

#[test]
fn unlink_unknown_id_fails() {
    let s = Sara::new();
    let out = s.cmd().args(["unlink", "99"]).output().expect("spawn sara");
    assert!(!out.status.success(), "unlinking a missing link must fail");
    assert!(String::from_utf8_lossy(&out.stderr).contains("No link with id 99"));
}
