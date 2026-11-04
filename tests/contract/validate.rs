use std::process::Command;

use crate::harness::Sara;

fn git(s: &Sara, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(s.project())
        .args(args)
        .output()
        .expect("spawn git")
        .status
        .success();
    assert!(ok, "git {args:?} failed");
}

fn begin(s: &Sara) -> String {
    let v = s.json(&[
        "begin", "demo", "--check", "ok", "--verify", "true", "--json",
    ]);
    v["uuid"].as_str().expect("uuid").to_string()
}

fn validate_err(s: &Sara, args: &[&str]) -> String {
    let out = s
        .cmd()
        .current_dir(s.project().parent().unwrap())
        .arg("validate")
        .args(args)
        .output()
        .expect("spawn sara");
    assert!(!out.status.success(), "validate should refuse");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
#[impish::contract("validate/A1", "validate/A2")]
fn validate_explains_a_repo_without_commits() {
    let s = Sara::new();
    git(&s, &["init", "-q"]);
    let uuid = begin(&s);

    let err = validate_err(&s, &[&uuid]);
    assert!(err.contains("acceptance gate is green"), "{err}");
    assert!(err.contains("project has no commits yet"), "{err}");

    let err = validate_err(&s, &[&uuid, "--no-run"]);
    assert!(!err.contains("gate is green"), "{err}");
    assert!(err.contains("project has no commits yet"), "{err}");
}

#[test]
#[impish::contract("validate/A3")]
fn validate_explains_a_moved_project() {
    let s = Sara::new();
    let uuid = begin(&s);
    std::fs::remove_dir_all(s.project()).unwrap();

    let err = validate_err(&s, &[&uuid, "--no-run"]);
    assert!(err.contains("no longer exists"), "{err}");
    assert!(err.contains("re-run `sara init`"), "{err}");
}

#[test]
#[impish::contract("validate/A4")]
fn validate_explains_a_folder_outside_git() {
    let s = Sara::new();
    let uuid = begin(&s);

    let err = validate_err(&s, &[&uuid, "--no-run"]);
    assert!(err.contains("not in a git repo"), "{err}");
}
