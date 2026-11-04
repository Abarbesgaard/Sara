use serde_json::json;

use crate::harness::Sara;

#[test]
fn link_then_unlink_round_trips_through_info() {
    let s = Sara::new();
    s.run(&["add", "-y", "Ship it"]);

    s.run(&["link", "1", "https://example.com/pr/1", "--label", "PR"]);
    assert_eq!(
        s.json(&["info", "1", "--json"])["links"],
        json!([{ "id": 1, "label": "PR", "url": "https://example.com/pr/1" }]),
    );

    s.run(&["unlink", "1"]);
    assert_eq!(s.json(&["info", "1", "--json"])["links"], json!([]));
}

#[test]
fn unlink_unknown_id_fails() {
    let s = Sara::new();
    let out = s.cmd().args(["unlink", "99"]).output().expect("spawn sara");
    assert!(!out.status.success(), "unlinking a missing link must fail");
    assert!(String::from_utf8_lossy(&out.stderr).contains("No link with id 99"));
}

#[test]
fn linking_the_same_url_twice_keeps_one_link() {
    let s = Sara::new();
    s.run(&["add", "-y", "Ship it"]);

    let first = s.run(&["link", "1", "https://example.com/pr/1"]);
    assert!(first.contains("Linked task 1"), "{first}");
    let second = s.run(&["link", "1", "https://example.com/pr/1"]);
    assert!(second.contains("already linked"), "{second}");
    s.run(&["link", "1", "https://example.com/pr/2"]);

    let links = s.json(&["info", "1", "--json"])["links"].clone();
    let urls: Vec<&str> = links
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["url"].as_str().unwrap())
        .collect();
    assert_eq!(
        urls,
        ["https://example.com/pr/1", "https://example.com/pr/2"]
    );
}

#[test]
fn mcp_link_reports_already_linked() {
    let s = Sara::new();
    s.run(&["add", "-y", "Ship it"]);
    let mut mcp = s.mcp();
    let args = serde_json::json!({ "id": "1", "url": "https://example.com/pr/1" });

    assert_eq!(
        mcp.call_result("link", args.clone())["already_linked"],
        false
    );
    assert_eq!(mcp.call_result("link", args)["already_linked"], true);
}

#[test]
fn mcp_unlink_by_link_id() {
    let s = Sara::new();
    s.run(&["add", "-y", "Ship it"]);
    s.run(&["link", "1", "https://example.com/pr/1"]);
    let mut mcp = s.mcp();

    let removed = mcp.call_result("unlink", json!({ "link_id": 1 }));
    assert_eq!(removed, json!({ "link_id": 1, "removed": true }));
    assert_eq!(s.json(&["info", "1", "--json"])["links"], json!([]));
}

#[test]
fn mcp_unlink_by_task_and_url() {
    let s = Sara::new();
    s.run(&["add", "-y", "Ship it"]);
    s.run(&["link", "1", "https://example.com/pr/1"]);
    s.run(&["link", "1", "https://example.com/pr/2"]);
    let mut mcp = s.mcp();

    let removed = mcp.call_result(
        "unlink",
        json!({ "id": "1", "url": "https://example.com/pr/1" }),
    );
    assert_eq!(removed["removed"], true);
    assert_eq!(removed["link_id"], 1);
    assert_eq!(
        s.json(&["info", "1", "--json"])["links"],
        json!([{ "id": 2, "label": null, "url": "https://example.com/pr/2" }]),
    );

    let missing = mcp.call(
        "unlink",
        json!({ "id": "1", "url": "https://example.com/pr/1" }),
    );
    assert!(
        missing
            .to_string()
            .contains("has no link to https://example.com/pr/1"),
        "{missing}"
    );
}

#[test]
fn mcp_unlink_needs_link_id_or_task_and_url() {
    let s = Sara::new();
    s.run(&["add", "-y", "Ship it"]);
    let mut mcp = s.mcp();

    for args in [
        json!({}),
        json!({ "id": "1" }),
        json!({ "link_id": 1, "url": "https://example.com" }),
    ] {
        let env = mcp.call("unlink", args.clone());
        assert!(
            env.to_string().contains("Pass either `link_id`"),
            "{args}: {env}"
        );
    }
}
