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
        let folded = map.remove("folded").expect("telemetry builds fold a trace");
        assert!(folded.as_array().is_some_and(|ops| !ops.is_empty()));
        assert!(map.remove("begin_id").is_some_and(|id| id.is_string()));
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
