use crate::harness::Sara;

fn fails(s: &Sara, args: &[&str]) -> (Option<i32>, String) {
    let out = s.cmd().args(args).output().expect("spawn sara");
    assert!(
        !out.status.success(),
        "expected `sara {}` to fail but it succeeded",
        args.join(" "),
    );
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into(),
    )
}

#[test]
fn unknown_task_id_exits_1() {
    let s = Sara::new();
    let (code, stderr) = fails(&s, &["info", "999"]);
    assert_eq!(code, Some(1), "unknown id should exit 1");
    assert!(
        stderr.contains("999"),
        "stderr should name the missing id: {stderr}"
    );
}

#[test]
fn secret_guardrail_rejects_and_exits_1() {
    let s = Sara::new();
    let (code, stderr) = fails(
        &s,
        &["learn", "--tag", "x", "AKIAIOSFODNN7EXAMPLE is the key"],
    );
    assert_eq!(code, Some(1), "guardrail rejection should exit 1");
    assert!(
        stderr.to_lowercase().contains("secret"),
        "stderr should explain the guardrail: {stderr}"
    );
}

#[test]
fn unknown_subcommand_exits_2() {
    let s = Sara::new();
    let (code, _) = fails(&s, &["frobnicate"]);
    assert_eq!(code, Some(2), "clap usage error should exit 2");
}

#[test]
fn add_rejects_an_unknown_priority() {
    let s = Sara::new();
    for args in [
        &["add", "-y", "pri:zzz", "Thing"][..],
        &["add", "-y", "--priority", "zzz", "Thing"],
    ] {
        let (code, stderr) = fails(&s, args);
        assert_eq!(code, Some(1));
        assert!(stderr.contains("Unknown priority: ZZZ"), "{stderr}");
    }
    assert!(
        s.run(&["list"]).contains("No pending tasks"),
        "nothing is saved"
    );
}
