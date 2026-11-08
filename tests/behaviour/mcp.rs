use insta::assert_json_snapshot;
use serde_json::json;

use crate::harness::{Sara, redact};

#[test]
fn tool_response_envelope_is_wellformed() {
    let s = Sara::new();
    let project = s.project().to_string_lossy().to_string();
    let mut mcp = s.mcp();

    let env = mcp.call(
        "learn",
        json!({"text": "Envelope check", "tag": ["testing"], "project_path": project}),
    );
    assert_eq!(env["jsonrpc"], "2.0", "must be JSON-RPC 2.0");
    assert!(env["id"].is_number(), "response carries its request id");
    assert_eq!(
        env["result"]["content"][0]["type"], "text",
        "tool result is a text content block",
    );
    assert!(
        env["result"]["content"][0]["text"].is_string(),
        "text block carries the JSON payload as a string",
    );
}

#[test]
fn tool_failure_is_an_is_error_result_not_a_protocol_error() {
    let s = Sara::new();
    let project = s.project().to_string_lossy().to_string();
    let mut mcp = s.mcp();

    let env = mcp.call("info", json!({"id": "zzzzzzzz", "project_path": project}));
    assert!(
        env.get("error").is_none(),
        "a domain failure must not be a JSON-RPC error: {env}",
    );
    assert_eq!(env["result"]["isError"], true, "{env}");
    let text = env["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains("zzzzzzzz"),
        "error text reaches the model: {env}"
    );
}

#[test]
fn begin_learn_recall_payloads() {
    let s = Sara::new();
    let project = s.project().to_string_lossy().to_string();
    let mut mcp = s.mcp();

    let begin = mcp.call_result(
        "begin",
        json!({
            "description": "Wire up the payments client",
            "tags": ["payments"],
            "check": "client compiles and unit tests pass",
            "project_path": project,
        }),
    );
    #[cfg(feature = "telemetry")]
    let begin = {
        let mut begin = begin;
        let map = begin.as_object_mut().expect("begin returns an object");
        let trace = map.remove("trace").expect("telemetry builds fold a trace");
        assert!(trace["ops"].as_array().is_some_and(|ops| !ops.is_empty()));
        assert!(trace["id"].is_string());
        assert!(!map.contains_key("begin_id") && !map.contains_key("folded"));
        begin
    };
    assert_json_snapshot!("mcp_begin", redact(begin));

    let learn = mcp.call_result(
        "learn",
        json!({
            "text": "The payments client takes amounts in minor units",
            "tag": ["payments"],
            "project_path": project,
        }),
    );
    assert_json_snapshot!("mcp_learn", redact(learn));

    let recall = mcp.call_result(
        "recall",
        json!({"query": "payments minor units", "project_path": project}),
    );
    assert_json_snapshot!("mcp_recall", redact(recall));
}

#[test]
fn find_tool_searches_tasks_by_partial_uuid() {
    let s = Sara::new();
    let project = s.project().to_string_lossy().to_string();
    let mut mcp = s.mcp();

    let begin = mcp.call_result(
        "begin",
        json!({
            "description": "Locate me by a uuid fragment",
            "project_path": project,
        }),
    );
    let uuid = begin["uuid"].as_str().expect("begin returns a uuid");

    // A hyphen-free fragment spanning the first hyphen boundary (chars 6..8 + 9..11),
    // which the stored uuid never contains literally.
    let fragment: String = format!("{}{}", &uuid[6..8], &uuid[9..11]);
    assert!(!fragment.contains('-'));

    let found = mcp.call_result(
        "find",
        json!({"fragment": fragment, "project_path": project}),
    );
    let matches = found["matches"].as_array().expect("matches is an array");
    assert_eq!(
        matches.len(),
        1,
        "the hyphen-spanning fragment matches the task: {found}"
    );
    assert_eq!(matches[0]["uuid"], uuid);
    assert_eq!(matches[0]["description"], "Locate me by a uuid fragment");
    assert_json_snapshot!("mcp_find", redact(found));

    // Fragments shorter than the minimum are a domain error, not a protocol error.
    let short = mcp.call("find", json!({"fragment": "ab", "project_path": project}));
    assert!(
        short.get("error").is_none(),
        "domain failure, not JSON-RPC error: {short}"
    );
    assert_eq!(short["result"]["isError"], true, "{short}");
    let text = short["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains("too short"),
        "clear message reaches the model: {short}"
    );

    // LIKE wildcards in the fragment are literal, so a '%'-bearing fragment matches nothing.
    let wild = mcp.call_result("find", json!({"fragment": "ab%d", "project_path": project}));
    assert!(
        wild["matches"]
            .as_array()
            .expect("matches array")
            .is_empty(),
        "'%' is literal, not a wildcard: {wild}"
    );
}

#[test]
fn read_tools_answer_with_json() {
    let s = Sara::new();
    let project = s.project().to_string_lossy().to_string();
    let mut mcp = s.mcp();

    let begin = mcp.call_result(
        "begin",
        json!({"description": "Listed over MCP", "project_path": project}),
    );
    let id = begin["uuid"]
        .as_str()
        .expect("begin returns a uuid")
        .to_string();
    let listed = mcp.call_result("list", json!({"project_path": project}));
    assert!(
        listed["tasks"]
            .as_array()
            .expect("list returns its tasks")
            .iter()
            .any(|t| t["description"] == "Listed over MCP"),
        "the begun task is listed: {listed}"
    );

    let label = |v: serde_json::Value| {
        v["label"]
            .as_str()
            .expect("learn returns a label")
            .to_string()
    };
    let a = label(mcp.call_result(
        "learn",
        json!({"text": "Read tools answer in JSON", "tags": ["mcp"], "project_path": project}),
    ));
    let b = label(mcp.call_result(
        "learn",
        json!({"text": "Tool payloads are pretty JSON", "tags": ["mcp"], "project_path": project}),
    ));

    for (tool, args) in [
        ("resume", json!({"id": id})),
        ("next", json!({"id": id})),
        ("steps", json!({"id": id})),
        ("verify", json!({"id": id})),
        ("feedback", json!({"id": id})),
        ("plan_show", json!({"id": id})),
        ("tags", json!({})),
        ("memories", json!({})),
        ("projects", json!({})),
        (
            "link_memory",
            json!({"from": b, "relation": "similar_to", "to": a}),
        ),
        ("maintain", json!({"action": "diagnose"})),
    ] {
        let mut args = args;
        args["project_path"] = json!(project);
        let env = mcp.call(tool, args);
        let text = env["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        assert_ne!(env["result"]["isError"], true, "{tool} fails: {env}");
        assert!(
            serde_json::from_str::<serde_json::Value>(text).is_ok(),
            "{tool} answers with JSON: {env}"
        );
    }
}

#[test]
fn lifecycle_tools_answer_with_json() {
    let s = Sara::new();
    s.commit("init");
    let project = s.project().to_string_lossy().to_string();
    let mut mcp = s.mcp();
    let mut call = |tool: &str, mut args: serde_json::Value| {
        args["project_path"] = json!(project);
        mcp.call_result(tool, args)
    };

    let id = call("begin", json!({"description": "Lifecycle over MCP"}))["uuid"]
        .as_str()
        .expect("begin returns a uuid")
        .to_string();
    call(
        "check",
        json!({"id": id, "text": "it holds", "kind": "acceptance", "verify": "true"}),
    );
    let validated = call("validate", json!({"id": id}));
    assert_eq!(validated["criteria_ran"], 1, "{validated}");
    let deps = call("dep", json!({"id": id, "action": "list"}));
    assert_eq!(deps["blocked_by"], json!([]), "{deps}");

    let modified = call(
        "modify",
        json!({"id": id, "assignment": "Do the thing", "priority": "H"}),
    );
    assert_eq!(
        modified["priority"], "H",
        "fields change with the assignment: {modified}"
    );
    let recurring = call(
        "modify",
        json!({"id": id, "rationale": "It repeats", "every": "weekly"}),
    );
    assert!(
        !recurring["recur"].is_null(),
        "a recurrence alone still changes the task: {recurring}"
    );
    assert_eq!(
        modified["assignment"]["assignment"], "Do the thing",
        "{modified}"
    );

    let run = call(
        "record_run",
        json!({"id": id, "kind": "review", "prompt_tokens": 3, "completion_tokens": 4}),
    );
    assert_eq!(
        run["total_tokens"], 7,
        "total is prompt plus completion: {run}"
    );

    call("annotate", json!({"id": id, "text": "a note"}));
    call("annotate", json!({"id": id, "text": "another note"}));
    let notes = call("info", json!({"id": id}))["notes"].clone();
    let note = |i: usize| notes[i]["id"].as_i64().expect("a note has an id");
    let resolved = call(
        "resolve",
        json!({"feedback_id": note(0), "run_id": run["run_id"]}),
    );
    assert_eq!(resolved["resolved"], true, "{resolved}");
    call("denotate", json!({"annotation_id": note(1)}));

    let moved = call("move_task", json!({"id": id, "project": "Other"}));
    assert_eq!(moved["to"], "Other", "{moved}");
    let finished = call(
        "finish",
        json!({"id": id, "pr_url": "https://github.com/o/r/pull/1", "merged": false}),
    );
    assert_eq!(finished["status"], "waiting_for_merge", "{finished}");
}
