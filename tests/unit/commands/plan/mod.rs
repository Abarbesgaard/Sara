use super::*;
use crate::infrastructure::db::{STEP_KIND_ACCEPTANCE, STEP_KIND_STEP};

fn plan_json() -> &'static str {
    r#"{
        "project": "planned",
        "tasks": [
            {
                "key": "base",
                "description": "lay the foundation",
                "priority": "H",
                "tags": ["infra"],
                "assignment": "alice",
                "rationale": "everything else builds on this",
                "steps": ["dig", "pour"],
                "acceptance": ["foundation is level"],
                "findings": ["soil is soft"],
                "constraints": ["finish before rain"]
            },
            {
                "key": "walls",
                "description": "raise the walls",
                "depends_on": ["base"]
            }
        ]
    }"#
}

#[test]
fn import_raw_creates_tasks_in_the_named_project() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();

    let v = import_raw(&conn, &cfg, plan_json()).unwrap();
    assert_eq!(v["created"], serde_json::json!(2));
    assert_eq!(v["project"], serde_json::json!("planned"));

    let tasks = db::list_tasks(&conn, Some("planned")).unwrap();
    assert_eq!(tasks.len(), 2, "both plan tasks must be inserted");
}

#[test]
fn import_raw_wires_up_dependencies_by_key() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    import_raw(&conn, &cfg, plan_json()).unwrap();

    let base = db::get_task_by_id(&conn, 1).unwrap().unwrap();
    let walls = db::get_task_by_id(&conn, 2).unwrap().unwrap();
    assert_eq!(base.description, "lay the foundation");
    assert_eq!(walls.description, "raise the walls");

    let blockers = db::get_blockers(&conn, &walls.uuid).unwrap();
    assert_eq!(
        blockers,
        vec![base.uuid],
        "the `depends_on: [base]` edge must be remapped onto the inserted tasks"
    );
}

#[test]
fn import_raw_carries_steps_acceptance_and_annotations() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    import_raw(&conn, &cfg, plan_json()).unwrap();

    let base = db::get_task_by_id(&conn, 1).unwrap().unwrap();
    let steps = db::get_steps(&conn, &base.uuid, STEP_KIND_STEP).unwrap();
    let acceptance = db::get_steps(&conn, &base.uuid, STEP_KIND_ACCEPTANCE).unwrap();
    assert_eq!(steps.len(), 2, "both steps must be imported");
    assert_eq!(
        acceptance.len(),
        1,
        "the acceptance criterion must be imported"
    );

    let annotations = db::get_annotations(&conn, &base.uuid).unwrap();
    let kinds: Vec<&str> = annotations.iter().map(|a| a.kind.as_str()).collect();
    assert!(
        kinds.contains(&"finding") && kinds.contains(&"constraint"),
        "findings and constraints must be imported as annotations: {kinds:?}"
    );
}

#[test]
fn import_raw_rejects_an_empty_plan() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    let err = import_raw(&conn, &cfg, r#"{"project":"x","tasks":[]}"#)
        .expect_err("a plan with no tasks must be refused");
    assert!(
        err.to_string().to_lowercase().contains("no tasks"),
        "error should explain the empty plan: {err}"
    );
}

#[test]
fn import_raw_rejects_invalid_json() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    let err = import_raw(&conn, &cfg, "not json at all")
        .expect_err("malformed plan JSON must be refused");
    assert!(
        err.to_string().to_lowercase().contains("invalid"),
        "error should flag the invalid JSON: {err}"
    );
}
