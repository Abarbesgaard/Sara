use insta::assert_snapshot;

use crate::harness::Sara;

fn seed(s: &Sara) {
    s.run(&["add", "Fix flaky build"]);
    s.run(&[
        "learn",
        "--tag",
        "dependabot",
        "dependabot bumped serde in repo a and broke the build",
    ]);
    s.run(&[
        "learn",
        "--force",
        "--tag",
        "dependabot",
        "--derived-from",
        "m1",
        "dependabot bumped serde in repo b and broke the build",
    ]);
    s.run(&[
        "learn",
        "--force",
        "--tag",
        "dependabot",
        "--derived-from",
        "m1",
        "dependabot bumped serde in repo c and broke the build",
    ]);
    s.run(&[
        "learn",
        "--task",
        "1",
        "--tag",
        "ci",
        "flaky build was caused by a tokio timeout in the integration harness",
    ]);
    s.run(&[
        "learn",
        "--tag",
        "style",
        "prefer early returns over nesting",
    ]);
    s.run(&[
        "learn",
        "--supersedes",
        "m5",
        "--tag",
        "style",
        "prefer early returns and guard clauses over deep nesting",
    ]);
    std::fs::write(s.project().join("parser.rs"), "fn parse() { todo!() }\n").unwrap();
    s.run(&[
        "learn",
        "--file",
        "parser.rs",
        "--tag",
        "parser",
        "the parser handles unicode identifiers",
    ]);
    std::fs::write(
        s.project().join("parser.rs"),
        "struct Lexer;\nimpl Lexer { fn next(&mut self) -> Option<char> { None } }\n",
    )
    .unwrap();
}

fn recall(s: &Sara, args: &[&str]) -> String {
    let mut full = vec!["recall"];
    full.extend_from_slice(args);
    let out = s.run(&full);
    let project = s.project().to_string_lossy().to_string();
    let canonical = s
        .project()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();
    out.replace(&canonical, "[PROJECT]")
        .replace(&project, "[PROJECT]")
}

#[test]
fn recall_text_on_an_empty_store() {
    let s = Sara::new();
    assert_snapshot!("recall_empty_recent", recall(&s, &[]));
    assert_snapshot!("recall_empty_tag", recall(&s, &["--tag", "nope"]));
}

#[test]
fn recall_text_covers_every_output_branch() {
    let s = Sara::new();
    seed(&s);
    assert_snapshot!("recall_recent", recall(&s, &[]));
    assert_snapshot!("recall_keyword_cluster", recall(&s, &["dependabot"]));
    assert_snapshot!("recall_label", recall(&s, &["m1"]));
    assert_snapshot!("recall_linked_task", recall(&s, &["tokio timeout"]));
    assert_snapshot!("recall_superseded", recall(&s, &["--tag", "style"]));
    assert_snapshot!("recall_stale", recall(&s, &["unicode"]));
    assert_snapshot!("recall_loose", recall(&s, &["serde quantum"]));
    assert_snapshot!("recall_spread", recall(&s, &["--spread", "flaky"]));
    assert_snapshot!(
        "recall_spread_explicit",
        recall(&s, &["--spread", "dependabot"])
    );
    assert_snapshot!("recall_no_match", recall(&s, &["zzzqqqxxv"]));
    assert_snapshot!("recall_tag_no_match", recall(&s, &["--tag", "nope"]));
    assert_snapshot!(
        "recall_file_no_match",
        recall(&s, &["--file", "missing.rs"])
    );
}
