use insta::assert_snapshot;

use crate::harness::Sara;

fn seed_cluster(s: &Sara) {
    for body in [
        "dependabot bumped serde in repo a",
        "dependabot bumped serde in repo b",
        "dependabot bumped serde in repo c",
    ] {
        s.run(&["learn", "--tag", "dependabot", body]);
    }
    s.run(&["link-memory", "m1", "similar_to", "m2"]);
    s.run(&["link-memory", "m2", "similar_to", "m3"]);
}

#[test]
fn reflect_text_reports_nothing_on_an_empty_store() {
    let s = Sara::new();
    assert_snapshot!("reflect_empty", s.run(&["reflect"]));
    assert_snapshot!("reflect_apply_empty", s.run(&["reflect", "--apply"]));
}

#[test]
fn reflect_text_proposes_then_applies_a_cluster() {
    let s = Sara::new();
    seed_cluster(&s);
    assert_snapshot!("reflect_proposal", s.run(&["reflect"]));
    assert_snapshot!("reflect_apply", s.run(&["reflect", "--apply"]));
}
