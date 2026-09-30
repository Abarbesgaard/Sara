use super::*;
use crate::infrastructure::db;

use crate::test_support::cfg;

#[test]
fn begin_founds_a_task_with_assignment_and_next_cursor() {
    let conn = db::open_in_memory_for_test();
    let v = begin_value(
        &conn,
        &cfg(),
        Source::Cli,
        "Fix the failing build",
        &["ci".to_string()],
        &[],
        Some("proj"),
        None,
        None,
        Some("the restore is red on NU1608"),
        Some("dotnet build is green"),
        Some("dotnet build"),
    )
    .expect("begin succeeds");

    assert!(v["task"].as_i64().unwrap() > 0, "a task id is minted");
    assert_eq!(v["assignment"].as_str().unwrap(), "Fix the failing build");
    assert_eq!(
        v["rationale"].as_str().unwrap(),
        "the restore is red on NU1608"
    );
    assert_eq!(
        v["acceptance"]["kind"].as_str().unwrap(),
        db::STEP_KIND_ACCEPTANCE
    );
    assert!(
        v.get("recall").is_none() && v.get("finding").is_none(),
        "begin no longer auto-recalls: no recall/finding keys, got {v}"
    );
    assert_eq!(
        v["recall_step"]["kind"].as_str().unwrap(),
        db::STEP_KIND_STEP,
        "a checklist step is seeded"
    );
    assert_eq!(
        v["next"]["text"].as_str().unwrap(),
        RECALL_STEP_TEXT,
        "the recall step is the next cursor, got {}",
        v["next"]
    );
}

#[test]
fn missing_acceptance_warns_but_still_founds_the_task() {
    let conn = db::open_in_memory_for_test();
    let v = begin_value(
        &conn,
        &cfg(),
        Source::Cli,
        "Add a config flag",
        &[],
        &[],
        Some("proj"),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("begin succeeds without acceptance");

    assert!(v["task"].as_i64().unwrap() > 0);
    assert!(
        v["acceptance"].is_null(),
        "no acceptance criterion recorded"
    );
    let warnings: Vec<String> = v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w.as_str().map(str::to_string))
        .collect();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("no acceptance criterion")),
        "an absent definition of done is warned, not blocked: {warnings:?}"
    );
}

#[cfg(feature = "telemetry")]
#[test]
fn begin_records_every_folded_operation_as_an_ordered_event() {
    // `begin` is a composition: internally it runs add, assignment,
    // rationale, check, the seeded recall step, and next. Each folded
    // operation is surfaced as its own ordered event (event-sourcing
    // inspired) so the whole internal fan-out of a single `begin` is
    // observable, both in the returned `folded` log and — for real runs —
    // in telemetry.
    let conn = db::open_in_memory_for_test();
    let v = begin_value(
        &conn,
        &cfg(),
        Source::Cli,
        "Fix the failing build",
        &["ci".to_string()],
        &[],
        Some("proj"),
        None,
        None,
        Some("the restore is red on NU1608"),
        Some("dotnet build is green"),
        Some("dotnet build"),
    )
    .expect("begin succeeds");

    // begin does not recall — no recall block is emitted.
    assert!(
        v.get("recall").is_none(),
        "begin no longer auto-recalls: {v}"
    );

    // With assignment, rationale and check all supplied, every folded
    // operation is recorded, in composition order. The seeded recall `step`
    // replaces the former `recall`/`annotate` ops.
    let ops: Vec<String> = v["folded"]
        .as_array()
        .expect("folded event log is an array")
        .iter()
        .map(|e| e["op"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        ops,
        vec!["add", "assignment", "rationale", "check", "step", "next",],
        "every folded internal operation that ran is logged in order: {ops:?}"
    );
    // Each event carries a duration.
    for e in v["folded"].as_array().unwrap() {
        assert!(
            e["duration_ms"].is_u64(),
            "each folded event records its duration: {e}"
        );
    }

    // The whole composition shares one correlation id and the events carry a
    // contiguous 0-based `seq`, so the fan-out reconstructs as one ordered
    // trace rather than timestamp-adjacent records.
    assert!(
        v["begin_id"].as_str().is_some_and(|s| !s.is_empty()),
        "begin returns a trace/correlation id: {}",
        v["begin_id"]
    );
    let seqs: Vec<u64> = v["folded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["seq"].as_u64().expect("each folded event has a seq"))
        .collect();
    assert_eq!(
        seqs,
        (0..seqs.len() as u64).collect::<Vec<_>>(),
        "folded events carry a contiguous 0-based ordinal: {seqs:?}"
    );
    // The seeded recall step is folded in as a `step` op.
    let step = v["folded"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["op"] == "step")
        .expect("the seeded recall step is folded");
    assert!(
        step["duration_ms"].is_u64(),
        "the folded step reports its own duration: {step}"
    );
}

#[cfg(feature = "telemetry")]
#[test]
fn begin_folded_log_skips_operations_that_did_not_run() {
    // When rationale and check are absent, their folded events are not
    // fabricated — the log reflects only what actually ran. The recall
    // `step` is always seeded, so it is always present.
    let conn = db::open_in_memory_for_test();
    let v = begin_value(
        &conn,
        &cfg(),
        Source::Cli,
        "Add a config flag",
        &[],
        &[],
        Some("proj"),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("begin succeeds without acceptance");

    let ops: Vec<String> = v["folded"]
        .as_array()
        .expect("folded event log is an array")
        .iter()
        .map(|e| e["op"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        ops.contains(&"add".to_string()) && ops.contains(&"step".to_string()),
        "the always-run operations are logged: {ops:?}"
    );
    assert!(
        !ops.contains(&"rationale".to_string()) && !ops.contains(&"check".to_string()),
        "operations that did not run are not logged: {ops:?}"
    );
}

#[cfg(not(feature = "telemetry"))]
#[test]
fn begin_output_has_no_trace_without_the_telemetry_feature() {
    let conn = db::open_in_memory_for_test();
    let v = begin_value(
        &conn,
        &cfg(),
        Source::Mcp,
        "Add a config flag",
        &[],
        &[],
        Some("proj"),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("begin succeeds");
    assert!(
        v.get("folded").is_none() && v.get("begin_id").is_none(),
        "default builds keep begin's result free of trace keys: {v}"
    );
}
