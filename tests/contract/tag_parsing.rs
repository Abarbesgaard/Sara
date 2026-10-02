use serde_json::{Value, json};

use crate::harness::Sara;

fn tags_of(s: &Sara, id: &str) -> Vec<String> {
    let v = s.json(&["info", id, "--json"]);
    let task = v.get("task").unwrap_or(&v);
    task["tags"]
        .as_array()
        .expect("tags array")
        .iter()
        .map(|t| t.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn add_trims_tags_and_drops_empty_ones() {
    let s = Sara::new();
    s.run(&["add", "-t", " a ", "-t", "", "-t", "b", "-y", "Tagged task"]);
    assert_eq!(tags_of(&s, "1"), ["a", "b"]);
}

#[test]
fn modify_trims_tags_and_drops_empty_ones() {
    let s = Sara::new();
    s.run(&["add", "-y", "Task"]);
    s.run(&["modify", "1", "--tag", " c ", "--tag", " ", "--tag", "d"]);
    assert_eq!(tags_of(&s, "1"), ["c", "d"]);
}

#[test]
fn mcp_accepts_comma_separated_tags() {
    let s = Sara::new();
    let mut mcp = s.mcp();
    let res: Value = mcp.call_result(
        "add",
        json!({
            "description": "Via MCP",
            "tags": " x , ,y ",
            "project_path": s.project().to_string_lossy(),
        }),
    );
    assert!(res.get("isError").is_none_or(|e| e == false), "{res}");
    drop(mcp);
    assert_eq!(tags_of(&s, "1"), ["x", "y"]);
}
