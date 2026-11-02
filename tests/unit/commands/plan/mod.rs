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
    let conn = db::testing::open_in_memory_for_test();
    let cfg = Config::default();

    let v = import_raw(&conn, &cfg, plan_json()).unwrap();
    assert_eq!(v["created"], serde_json::json!(2));
    assert_eq!(v["project"], serde_json::json!("planned"));

    let tasks = db::list_tasks(&conn, Some("planned")).unwrap();
    assert_eq!(tasks.len(), 2, "both plan tasks must be inserted");
}

#[test]
fn import_raw_wires_up_dependencies_by_key() {
    let conn = db::testing::open_in_memory_for_test();
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
    let conn = db::testing::open_in_memory_for_test();
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
    let conn = db::testing::open_in_memory_for_test();
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
    let conn = db::testing::open_in_memory_for_test();
    let cfg = Config::default();
    let err = import_raw(&conn, &cfg, "not json at all")
        .expect_err("malformed plan JSON must be refused");
    assert!(
        err.to_string().to_lowercase().contains("invalid"),
        "error should flag the invalid JSON: {err}"
    );
}

mod generated {
    use super::*;
    use crate::commands::shared::normalize_list;
    use crate::domain::Priority;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    fn task() -> impl Strategy<Value = Value> {
        (
            prop::option::of("k[0-3]"),
            "( |[a-z]{1,5}){0,3}",
            prop::option::of(prop_oneof!["H", "m", "low", "nope", "", " "]),
            prop::collection::vec("( |[a-z]){0,4}", 0..3),
            prop::collection::vec("k[0-4]", 0..2),
        )
            .prop_map(|(key, description, priority, tags, depends_on)| {
                json!({"key": key, "description": description, "priority": priority,
                       "tags": tags, "depends_on": depends_on})
            })
    }

    fn valid(tasks: &[Value]) -> bool {
        let keys: Vec<&str> = tasks.iter().filter_map(|t| t["key"].as_str()).collect();
        let unique = keys
            .iter()
            .all(|k| keys.iter().filter(|o| *o == k).count() == 1);
        unique
            && tasks.iter().all(|t| {
                !t["description"].as_str().unwrap().trim().is_empty()
                    && t["priority"]
                        .as_str()
                        .is_none_or(|p| p.trim().is_empty() || p.parse::<Priority>().is_ok())
                    && deps(t).iter().all(|d| keys.contains(&d.as_str()))
            })
            && !cyclic(tasks)
    }

    fn deps(t: &Value) -> Vec<String> {
        serde_json::from_value(t["depends_on"].clone()).unwrap()
    }

    fn index_of(tasks: &[Value], key: &str) -> usize {
        tasks.iter().position(|t| t["key"] == key).unwrap()
    }

    fn cyclic(tasks: &[Value]) -> bool {
        fn reaches(tasks: &[Value], from: usize, to: usize, seen: &mut Vec<usize>) -> bool {
            deps(&tasks[from]).iter().any(|d| {
                let next = index_of(tasks, d);
                next == to
                    || (!seen.contains(&next) && {
                        seen.push(next);
                        reaches(tasks, next, to, seen)
                    })
            })
        }
        (0..tasks.len()).any(|i| reaches(tasks, i, i, &mut Vec::new()))
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::Direct("tests/proptest-regressions/plan_import.txt"),
            )),
            ..ProptestConfig::default()
        })]

        #[test]
        fn never_panics(raw in "\\PC{0,80}") {
            let conn = db::testing::open_in_memory_for_test();
            let _ = import_raw(&conn, &Config::default(), &raw);
        }

        #[test]
        fn imports_exactly_or_saves_nothing(tasks in prop::collection::vec(task(), 1..5)) {
            let conn = db::testing::open_in_memory_for_test();
            let raw = json!({"project": "p", "tasks": tasks}).to_string();
            let result = import_raw(&conn, &Config::default(), &raw);
            let saved = db::list_tasks(&conn, Some("p")).unwrap();
            if !valid(&tasks) {
                prop_assert!(result.is_err(), "accepted an invalid plan: {raw}");
                prop_assert!(saved.is_empty(), "a rejected plan saved tasks");
                return Ok(());
            }
            prop_assert!(result.is_ok(), "rejected a valid plan {raw}: {result:?}");
            prop_assert_eq!(saved.len(), tasks.len());
            for (i, t) in tasks.iter().enumerate() {
                let got = db::get_task_by_id(&conn, i as i64 + 1).unwrap().unwrap();
                prop_assert_eq!(got.description, t["description"].as_str().unwrap().trim());
                let tags: Vec<String> = serde_json::from_value(t["tags"].clone()).unwrap();
                prop_assert_eq!(got.tags, normalize_list(&tags));
                let want = t["priority"].as_str().and_then(|p| p.parse::<Priority>().ok());
                prop_assert_eq!(got.priority, want);
                let mut wired: Vec<_> = db::get_dependency_uuids(&conn, &got.uuid).unwrap()
                    .into_iter().map(|u| saved.iter().find(|s| s.uuid == u).unwrap().id.unwrap())
                    .collect();
                let mut expected: Vec<_> = deps(t).iter().map(|d| index_of(&tasks, d) as i64 + 1).collect();
                wired.sort(); expected.sort(); expected.dedup();
                prop_assert_eq!(wired, expected);
            }
        }
    }
}
