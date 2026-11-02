use super::*;

fn body(lines: std::ops::Range<usize>) -> String {
    lines
        .map(|i| format!("let value_{i} = compute({i});"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn fingerprint_hash_is_stable_across_builds() {
    assert_eq!(fnv1a(b"hello"), 0xa430_d846_80aa_bd0b);
}

#[test]
fn fingerprint_sketch_ignores_blank_and_brace_lines() {
    let s = sketch("fn main() {\n\n    }\n  }\nlet x = 1;\n");
    assert_eq!(s.len(), 2, "only `fn main() {{` and `let x = 1;` count");
}

#[test]
fn fingerprint_sketch_is_bounded_and_round_trips() {
    let s = sketch(&body(0..1000));
    assert_eq!(s.len(), SKETCH_SIZE);
    assert_eq!(decode(&encode(&s)), s);
}

#[test]
fn fingerprint_drift_is_share_of_vanished_lines() {
    let original = body(0..100);
    let s = sketch(&original);
    assert_eq!(drift(&s, &original), 0.0);

    let half_rewritten = format!("{}\n{}", body(0..50), body(500..550));
    let d = drift(&s, &half_rewritten);
    assert!((d - 0.5).abs() < 1e-9, "got {d}");

    let reordered_with_additions = format!("{}\n{}\nnew_line();", body(50..100), body(0..50));
    assert_eq!(drift(&s, &reordered_with_additions), 0.0);
}

#[test]
fn fingerprint_judge_flags_drift_at_threshold() {
    let fp = encode(&sketch(&body(0..10)));
    assert_eq!(judge(&fp, &body(0..10)), AnchorState::Fresh);
    assert_eq!(
        judge(&fp, &format!("{}\n{}", body(0..6), body(50..54))),
        AnchorState::Drifted(0.4)
    );
}
