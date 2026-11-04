use crate::harness::Sara;

/// What fish would offer for the last (empty) word of `line`.
fn complete(s: &Sara, line: &[&str]) -> String {
    let out = s
        .cmd()
        .env("COMPLETE", "fish")
        .arg("--")
        .args(line)
        .arg("")
        .output()
        .expect("spawn sara");
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn completion_offers_task_ids_projects_and_memory_labels() {
    let s = Sara::new();
    s.run(&["add", "-y", "Write the docs"]);
    s.run(&["learn", "retries back off exponentially"]);

    let tasks = complete(&s, &["sara", "info"]);
    assert!(tasks.contains("1\tWrite the docs"), "{tasks}");
    let projects = complete(&s, &["sara", "list", "--project"]);
    assert!(projects.lines().any(|l| l == "Demo"), "{projects}");
    let memories = complete(&s, &["sara", "learn", "x", "--supersedes"]);
    assert!(
        memories.contains("m1\tretries back off exponentially"),
        "{memories}"
    );
}
