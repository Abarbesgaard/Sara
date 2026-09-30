use crate::harness::Sara;

#[cfg(not(feature = "telemetry"))]
#[test]
fn telemetry_subcommand_reports_not_compiled_and_exits_0() {
    let s = Sara::new();
    for args in [
        &["telemetry"][..],
        &["telemetry", "status"],
        &["telemetry", "on"],
        &["telemetry", "--show"],
    ] {
        let out = s.run(args);
        assert_eq!(
            out.trim(),
            "telemetry not compiled into this build",
            "`sara {}` without the telemetry feature",
            args.join(" ")
        );
    }
}

#[cfg(feature = "telemetry")]
#[test]
fn telemetry_status_honours_the_env_opt_out() {
    let s = Sara::new();
    let out = s.run(&["telemetry", "status"]);
    assert!(
        out.starts_with("Telemetry: off"),
        "SARA_NO_TELEMETRY=1 must switch telemetry off: {out}"
    );
}
