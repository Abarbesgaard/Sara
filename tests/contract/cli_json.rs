use insta::assert_json_snapshot;

use crate::harness::{Sara, redact};

fn seeded() -> Sara {
    let s = Sara::new();
    s.run(&["add", "--priority", "H", "-t", "demo", "-y", "First task"]);
    s.run(&[
        "learn",
        "--tag",
        "demo",
        "Prefer explicit imports in split modules",
    ]);
    s
}

#[test]
fn list_json_shape() {
    let s = seeded();
    assert_json_snapshot!(redact(s.json(&["list", "--json"])));
}

#[test]
fn info_json_shape() {
    let s = seeded();
    assert_json_snapshot!(redact(s.json(&["info", "1", "--json"])));
}

#[test]
fn recall_json_shape() {
    let s = seeded();
    assert_json_snapshot!(redact(s.json(&["recall", "--json", "imports"])));
}
