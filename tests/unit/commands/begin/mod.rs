use super::*;
use crate::infrastructure::db;

fn cfg() -> Config {
    Config::default()
}

#[test]
fn begin_founds_a_task_with_assignment_and_next_cursor() {
    let conn = db::open_in_memory_for_test();
    let v = begin_value(
        &conn,
        &cfg(),
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
    // Assignment defaults to the description when not given explicitly.
    assert_eq!(v["assignment"].as_str().unwrap(), "Fix the failing build");
    assert_eq!(
        v["rationale"].as_str().unwrap(),
        "the restore is red on NU1608"
    );
    // The acceptance criterion is registered and carries its verify command.
    assert_eq!(
        v["acceptance"]["kind"].as_str().unwrap(),
        db::STEP_KIND_ACCEPTANCE
    );
    // begin does NOT recall — it seeds a first recall STEP instead.
    assert!(
        v.get("recall").is_none() && v.get("finding").is_none(),
        "begin no longer auto-recalls: no recall/finding keys, got {v}"
    );
    assert_eq!(
        v["recall_step"]["kind"].as_str().unwrap(),
        db::STEP_KIND_STEP,
        "a checklist step is seeded"
    );
    // The seeded recall step is the execution cursor `next` returns, so
    // prior art is the first thing the agent is directed to do.
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
