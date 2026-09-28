//! Snapshot the stable `--json` CLI surfaces agents consume: `list`, `info`,
//! and `recall`. A field rename or shape change in any of these makes the
//! corresponding snapshot fail — which is exactly the silent-drift regression
//! this suite exists to catch.

use insta::assert_json_snapshot;

use crate::harness::{Sara, redact};

/// Seed one project with a single tagged task and one memory, deterministically.
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
