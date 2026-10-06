use crate::harness::Sara;

#[test]
fn default_build_compiles_telemetry_in() {
    let manifest: toml::Value =
        toml::from_str(include_str!("../../Cargo.toml")).expect("Cargo.toml");
    let default = manifest["features"]["default"]
        .as_array()
        .expect("[features] default is an array");
    assert!(
        default.iter().any(|f| f.as_str() == Some("telemetry")),
        "telemetry must be a default feature (opt-out at runtime), got {default:?}"
    );
}

#[cfg(feature = "telemetry")]
#[test]
fn every_mcp_tool_call_is_captured() {
    use serde_json::json;

    let s = Sara::new();
    let project = s.project().to_string_lossy().to_string();
    let queue = s.project().join("telemetry-queue.jsonl");
    let mut mcp = s.mcp_with_telemetry(&queue);

    let ok = mcp.call(
        "learn",
        json!({"text": "Telemetry coverage probe", "tag": ["testing"], "project_path": project}),
    );
    assert!(ok["result"]["isError"] != true, "{ok}");
    let domain_err = mcp.call("info", json!({"id": "zzzzzzzz", "project_path": project}));
    assert_eq!(domain_err["result"]["isError"], true, "{domain_err}");
    let bad_params = mcp.call("next", json!({"project_path": project}));
    let unknown = mcp.call("no_such_tool", json!({}));
    drop(mcp);

    let records: Vec<serde_json::Value> = std::fs::read_to_string(&queue)
        .unwrap_or_else(|e| panic!("no telemetry queue at {}: {e}", queue.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("queue line is json"))
        .filter(|r: &serde_json::Value| r["source"] == "mcp")
        .collect();
    let find = |name: &str| {
        records
            .iter()
            .find(|r| r["name"] == name)
            .unwrap_or_else(|| panic!("no record for `{name}` in {records:#?}"))
    };

    let learn = find("mcp learn");
    assert_eq!(learn["ok"], true);
    assert_eq!(learn["client"], "contract-tests");
    assert_eq!(find("mcp info")["ok"], false);
    assert_eq!(find("mcp next")["ok"], false, "bad params: {bad_params}");
    assert_eq!(find("mcp no_such_tool")["ok"], false, "unknown: {unknown}");
    assert_eq!(records.len(), 4, "one record per MCP call: {records:#?}");
}

#[cfg(not(feature = "telemetry"))]
#[test]
fn telemetry_subcommand_reports_not_compiled_and_exits_0() {
    let s = Sara::new();
    for args in [
        &["telemetry"][..],
        &["telemetry", "status"],
        &["telemetry", "on"],
        &["telemetry", "--show"],
    ] {
        let out = s.run(args);
        assert_eq!(
            out.trim(),
            "telemetry not compiled into this build",
            "`sara {}` without the telemetry feature",
            args.join(" ")
        );
    }
}

#[cfg(feature = "telemetry")]
#[test]
fn telemetry_status_honours_the_env_opt_out() {
    let s = Sara::new();
    let out = s.run(&["telemetry", "status"]);
    assert!(
        out.starts_with("Telemetry: off"),
        "SARA_NO_TELEMETRY=1 must switch telemetry off: {out}"
    );
}

#[cfg(feature = "telemetry")]
fn queued(queue: &std::path::Path, name: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(queue)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("queue line is json"))
        .filter(|r| r["name"] == name)
        .collect()
}

/// Two memories learned before the task; the task recalls one and cites both.
#[cfg(feature = "telemetry")]
fn seed_outcome(s: &Sara) {
    s.run(&["learn", "--tag", "retry", "retries back off exponentially"]);
    s.run(&[
        "learn",
        "--force",
        "--tag",
        "retry",
        "idempotency keys make retries safe",
    ]);
    s.run(&["add", "Ship the retry policy"]);
    s.run(&["recall", "m1", "--task", "1"]);
}

#[cfg(feature = "telemetry")]
#[test]
fn outcome_cli_done_records_prior_memory_count() {
    let s = Sara::new();
    seed_outcome(&s);
    let queue = s.project().join("telemetry-queue.jsonl");
    let out = s
        .cmd()
        .args(["done", "1", "--used", "m2"])
        .env_remove("SARA_NO_TELEMETRY")
        .env("SARA_TELEMETRY_QUEUE", &queue)
        .env("SARA_TELEMETRY_ENDPOINT", "http://127.0.0.1:9/insert")
        .env("SARA_TELEMETRY_FLUSH_INTERVAL", "86400")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");

    let outcome = queued(&queue, "outcome");
    assert_eq!(outcome.len(), 1, "{outcome:#?}");
    assert_eq!(outcome[0]["n"], 2, "{outcome:#?}");
    assert_eq!(outcome[0]["verified"], false, "{outcome:#?}");
    assert_eq!(
        queued(&queue, "done").len(),
        1,
        "the done call is still captured"
    );
}

#[cfg(feature = "telemetry")]
#[test]
fn outcome_mcp_done_records_prior_memory_count() {
    use serde_json::json;

    let s = Sara::new();
    seed_outcome(&s);
    let project = s.project().to_string_lossy().to_string();
    let queue = s.project().join("telemetry-queue.jsonl");
    let mut mcp = s.mcp_with_telemetry(&queue);
    let r = mcp.call(
        "done",
        json!({"id": "1", "used": ["m1"], "project_path": project}),
    );
    assert!(r["result"]["isError"] != true, "{r}");
    drop(mcp);

    let outcome = queued(&queue, "mcp outcome");
    assert_eq!(outcome.len(), 1, "{outcome:#?}");
    assert_eq!(outcome[0]["n"], 1, "{outcome:#?}");
    assert_eq!(outcome[0]["verified"], false, "{outcome:#?}");
    assert_eq!(
        queued(&queue, "mcp done").len(),
        1,
        "the tool call is still captured"
    );
}
