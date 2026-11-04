use serde_json::json;

use crate::harness::Sara;

fn seed(s: &Sara) {
    s.run(&["add", "Ship the retry policy"]);
    s.run(&["check", "1", "Write the policy"]);
    s.run(&["check", "1", "Wire it in"]);
    s.run(&[
        "learn",
        "--tag",
        "retry",
        "retries back off exponentially with jitter",
    ]);
    s.run(&[
        "learn",
        "--force",
        "--tag",
        "retry",
        "idempotency keys make retries safe",
    ]);
}

fn cited(s: &Sara) -> Vec<(i64, String)> {
    s.memory_uses()
        .into_iter()
        .filter(|(_, kind)| kind == "cited")
        .collect()
}

#[test]
fn cite_cli_step_done_and_done_record_cited_uses() {
    let s = Sara::new();
    seed(&s);

    let out = s.run(&["step", "done", "1", "1", "--used", "m1", "--used", "m2"]);
    assert!(out.contains("Cited: m1, m2"), "{out}");
    assert_eq!(cited(&s).len(), 2);

    s.run(&["step", "done", "1", "2"]);
    let out = s.run(&["done", "1", "--used", "m1"]);
    assert!(out.contains("Cited: m1"), "{out}");
    assert_eq!(cited(&s).len(), 2, "re-citing is idempotent");
}

#[test]
fn cite_cli_bad_label_fails_atomically() {
    let s = Sara::new();
    seed(&s);

    let out = s
        .cmd()
        .args(["step", "done", "1", "1", "--used", "m1", "--used", "m99"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("m99"));
    assert!(cited(&s).is_empty(), "nothing is cited");
    let steps = s.json(&["steps", "1", "--json"]);
    assert!(
        !steps.to_string().contains("\"done\":true"),
        "the step is not ticked: {steps}"
    );
}

#[test]
fn cite_info_lists_cited_memories() {
    let s = Sara::new();
    seed(&s);
    s.run(&["step", "done", "1", "1", "--used", "m2"]);

    let out = s.run(&["info", "1"]);
    assert!(
        out.lines()
            .any(|l| l.starts_with("Cited") && l.contains("m2") && l.contains("idempotency")),
        "{out}"
    );
}

#[test]
fn cite_mcp_step_done_and_done_record_and_reject_atomically() {
    let s = Sara::new();
    seed(&s);
    let project = s.project().to_string_lossy().to_string();
    let mut mcp = s.mcp();

    let env = mcp.call(
        "step_done",
        json!({"id": "1", "used": ["m1", "n1"], "project_path": project}),
    );
    assert_eq!(
        env["result"]["isError"], true,
        "a non-memory label fails: {env}"
    );
    assert!(cited(&s).is_empty());

    let v = mcp.call_result(
        "step_done",
        json!({"id": "1", "used": ["m1"], "project_path": project}),
    );
    assert_eq!(v["cited"], json!(["m1"]));
    assert_eq!(v["index"], 1, "the first step was still open");

    mcp.call_result("step_done", json!({"id": "1", "project_path": project}));
    let v = mcp.call_result(
        "done",
        json!({"id": "1", "used": "m2", "project_path": project}),
    );
    assert_eq!(v["cited"], json!(["m2"]));
    assert_eq!(cited(&s).len(), 2);

    let info = mcp.call_result(
        "info",
        json!({"id": v["uuid"].as_str().map(|u| &u[..8]).unwrap_or("1"), "project_path": project}),
    );
    let labels: Vec<&str> = info["cited"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["memory"].as_str().unwrap())
        .collect();
    assert_eq!(labels, vec!["m1", "m2"]);
}
