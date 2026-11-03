use std::process::{Command, Output};

use crate::harness::Sara;

const DIGIT_UUID: &str = "65909084-18e5-4724-8a7e-296fbbe8387a";

fn git(s: &Sara, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(s.project())
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .expect("spawn git")
        .status
        .success();
    assert!(ok, "git {args:?} failed");
}

fn sara(s: &Sara, args: &[&str]) -> Output {
    s.cmd().args(args).output().expect("spawn sara")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A task begun on branch `a`, with the project now on branch `b`.
fn task_on_other_branch(s: &Sara) -> String {
    git(s, &["init", "-q", "-b", "a"]);
    git(s, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let v = s.json(&["begin", "demo", "--json"]);
    git(s, &["checkout", "-q", "-b", "b"]);
    v["task"].as_i64().expect("display id").to_string()
}

/// Rewrites the only task's uuid so its 8-char prefix is all digits.
fn give_digit_uuid(s: &Sara) {
    let conn = s.db();
    let old: String = conn
        .query_row("SELECT uuid FROM tasks", [], |r| r.get(0))
        .unwrap();
    conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let columns: Vec<(String, String)> = conn
        .prepare(
            "SELECT m.name, c.name FROM sqlite_master m, pragma_table_info(m.name) c
             WHERE m.type='table' AND c.name IN ('uuid', 'task_uuid', 'depends_on_uuid')",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for (table, col) in columns {
        conn.execute(
            &format!("UPDATE \"{table}\" SET {col}=?1 WHERE {col}=?2"),
            [DIGIT_UUID, &old],
        )
        .unwrap_or_else(|e| panic!("{table}.{col}: {e}"));
    }
}

#[impish::contract("ids/A1")]
fn a_short_number_is_a_display_id() {
    let s = Sara::new();
    s.run(&["add", "demo"]);
    let out = sara(&s, &["info", "6590"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("display ids are renumbered"),
        "{}",
        stderr(&out)
    );
}

#[impish::contract("ids/A2")]
fn an_all_digit_uuid_prefix_is_a_uuid() {
    let s = Sara::new();
    s.run(&["add", "demo"]);
    give_digit_uuid(&s);
    let v = s.json(&["info", "65909084", "--json"]);
    assert_eq!(v["uuid"], DIGIT_UUID);
}

#[impish::contract("ids/A3")]
fn the_branch_guard_refuses_a_display_id() {
    let s = Sara::new();
    let id = task_on_other_branch(&s);
    let out = sara(&s, &["validate", &id, "--no-run"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("by display id"), "{}", stderr(&out));
}

#[impish::contract("ids/A2", "ids/A3")]
fn the_branch_guard_lets_an_all_digit_uuid_prefix_through() {
    let s = Sara::new();
    task_on_other_branch(&s);
    give_digit_uuid(&s);
    let out = sara(&s, &["validate", "65909084", "--no-run"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[impish::contract("ids/A4")]
fn the_suggested_id_works() {
    let s = Sara::new();
    let id = task_on_other_branch(&s);
    give_digit_uuid(&s);
    let err = stderr(&sara(&s, &["validate", &id, "--no-run"]));
    let suggested = err
        .split("stable uuid `")
        .nth(1)
        .and_then(|rest| rest.split('`').next())
        .unwrap_or_else(|| panic!("no suggestion in: {err}"));
    assert!(
        suggested.len() >= 8 && DIGIT_UUID.starts_with(suggested),
        "{suggested} is not a prefix of {DIGIT_UUID}"
    );
    assert!(
        suggested.parse::<i64>().is_err(),
        "{suggested} reads as a display id"
    );
    let out = sara(&s, &["validate", suggested, "--no-run"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[impish::contract("ids/A5")]
fn force_acts_on_any_branch() {
    let s = Sara::new();
    let id = task_on_other_branch(&s);
    let out = sara(&s, &["done", &id, "--force"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[impish::contract("ids/A6")]
fn a_blank_id_matches_nothing() {
    let s = Sara::new();
    s.run(&["add", "-y", "Only task"]);
    s.run(&["learn", "the only memory"]);
    for blank in ["", " "] {
        let out = sara(&s, &["done", blank]);
        assert!(!out.status.success(), "done {blank:?} succeeded");
        assert!(stderr(&out).contains("cannot be empty"), "{}", stderr(&out));
        let out = sara(&s, &["forget", blank]);
        assert!(!out.status.success(), "forget {blank:?} succeeded");
        assert!(stderr(&out).contains("cannot be empty"), "{}", stderr(&out));
    }
    assert!(s.run(&["info", "1"]).contains("Only task"));
    assert!(s.run(&["memories"]).contains("the only memory"));
}
