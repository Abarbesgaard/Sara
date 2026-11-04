use serde_json::Value;

use crate::harness::Sara;

#[test]
fn doctor_json_reports_a_healthy_fresh_store() {
    let s = Sara::new();
    let v = s.json(&["doctor", "--json"]);
    assert_eq!(v["healthy"], true, "{v}");
    assert_eq!(v["summary"]["warn"], 0);
    assert_eq!(v["checks"].as_array().map(Vec::len), Some(7));
    s.run(&["doctor", "--strict"]);
}

#[test]
fn doctor_human_report_lists_checks_and_verdict() {
    let s = Sara::new();
    let out = s.run(&["doctor"]);
    for id in ["embedding_coverage", "orphaned_links", "duplicates"] {
        assert!(out.contains(id), "missing {id} in:\n{out}");
    }
    assert!(out.contains("healthy"), "{out}");
}

#[test]
fn doctor_strict_exits_1_when_a_check_warns() {
    let s = Sara::new();
    let file = s.project().join("dup.rs").to_string_lossy().to_string();
    for body in [
        "dependabot bumped NSubstitute to 6.2.0 which broke dotnet restore with NU1608",
        "the dependabot NSubstitute 6.2.0 bump broke the dotnet restore build with NU1608",
    ] {
        s.run(&["learn", "--file", &file, body]);
    }
    let v = s.json(&["doctor", "--json"]);
    assert_eq!(v["healthy"], false, "{v}");

    let out = s
        .cmd()
        .args(["doctor", "--strict"])
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("warning"));
}

#[test]
fn doctor_mcp_tool_returns_the_cli_report() {
    let s = Sara::new();
    let project = s.project().to_string_lossy().to_string();
    let cli: Value = s.json(&["doctor", "--json"]);
    let mut mcp = s.mcp();
    let v = mcp.call_result("doctor", serde_json::json!({ "project_path": project }));
    assert_eq!(v, cli);
}

#[test]
fn doctor_knowledge_reuse_scopes_to_the_current_project() {
    let s = Sara::new();
    let v = s.json(&["doctor", "--json"]);
    let kpi = &v["knowledge_reuse"];
    assert_eq!(kpi["global"]["sufficient"], false, "{v}");
    assert!(kpi["project"]["name"].is_string(), "{v}");

    let out = s.run(&["doctor"]);
    assert!(out.contains("Knowledge reuse"), "{out}");
    assert!(
        out.contains("insufficient data (0 of 5 verified tasks)"),
        "{out}"
    );
}
