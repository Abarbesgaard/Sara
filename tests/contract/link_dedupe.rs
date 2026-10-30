use crate::harness::Sara;

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
