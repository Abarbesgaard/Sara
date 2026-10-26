use std::sync::Mutex;

use rmcp::ServerHandler;
use rusqlite::Connection;

use crate::commands;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;

use super::server::{CwdGuard, SaraServer};

static CWD_LOCK: Mutex<()> = Mutex::new(());

fn server_with(conn: Connection) -> SaraServer {
    SaraServer::new(conn, Config::default())
}

fn seed_task(conn: &Connection, project: &str, desc: &str) {
    crate::test_support::seed_task(conn, desc, project);
}

#[test]
fn exposes_the_agent_loop_tools() {
    let names: Vec<String> = SaraServer::all_router()
        .list_all()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(names.len(), 41, "expected 41 tools, got {names:?}");
    for expected in [
        "list",
        "find",
        "info",
        "next",
        "steps",
        "verify",
        "recall",
        "doctor",
        "feedback",
        "plan_show",
        "tags",
        "projects",
        "add",
        "begin",
        "step_done",
        "annotate",
        "plan_import",
        "check",
        "step_undone",
        "step_remove",
        "attach",
        "learn",
        "forget",
        "promote",
        "relearn",
        "done",
        "link",
        "unlink",
        "denotate",
        "dep",
        "validate",
        "modify",
        "resolve",
        "record_run",
        "move_task",
        "maintain",
        "link_memory",
        "memories",
        "finish",
        "resume",
        "doing",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing tool `{expected}` in {names:?}"
        );
    }
}

#[test]
fn every_tool_accepts_project_path() {
    let missing: Vec<String> = SaraServer::all_router()
        .list_all()
        .iter()
        .filter(|t| {
            let props = t
                .input_schema
                .get("properties")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            !props.contains_key("project_path")
        })
        .map(|t| t.name.to_string())
        .collect();

    assert!(
        missing.is_empty(),
        "these tools cannot be pointed at a project — add `project_path` to their \
         params struct: {missing:?}"
    );
}

#[test]
fn get_info_advertises_sara_tools_and_instructions() {
    let server = server_with(db::open_in_memory_for_test());
    let info = server.get_info();
    assert_eq!(info.server_info.name, "sara");
    assert!(
        info.capabilities.tools.is_some(),
        "tools capability missing"
    );
    assert!(
        info.instructions
            .as_deref()
            .unwrap_or_default()
            .contains("project_path"),
        "instructions should mention project_path"
    );
}

#[test]
fn cwd_guard_sets_and_restores_working_dir() {
    let _lock = CWD_LOCK.lock().unwrap();
    let start = std::env::current_dir().unwrap();
    let tmp = std::env::temp_dir().canonicalize().unwrap();
    {
        let _g = CwdGuard::enter(Some(tmp.to_str().unwrap())).unwrap();
        assert_eq!(
            std::env::current_dir().unwrap().canonicalize().unwrap(),
            tmp
        );
    }
    assert_eq!(std::env::current_dir().unwrap(), start, "cwd not restored");

    {
        let _g = CwdGuard::enter(None).unwrap();
        assert_eq!(std::env::current_dir().unwrap(), start);
    }
}

#[test]
fn cwd_guard_rejects_a_relative_project_path() {
    let _lock = CWD_LOCK.lock().unwrap();
    let start = std::env::current_dir().unwrap();
    for rel in ["..", ".", "some/sub/dir"] {
        let err = match CwdGuard::enter(Some(rel)) {
            Ok(_) => panic!("relative path {rel:?} must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("absolute"),
            "error should name the requirement, got: {err}"
        );
    }
    assert_eq!(
        std::env::current_dir().unwrap(),
        start,
        "cwd changed despite rejection"
    );
}

#[test]
fn cwd_guard_errors_on_missing_project_path() {
    let _lock = CWD_LOCK.lock().unwrap();
    let start = std::env::current_dir().unwrap();
    assert!(CwdGuard::enter(Some("/no/such/sara/dir/xyz")).is_err());
    assert_eq!(
        std::env::current_dir().unwrap(),
        start,
        "cwd changed on error"
    );
}

#[test]
fn with_project_runs_closure_against_the_connection() {
    let server = server_with(db::open_in_memory_for_test());
    seed_task_via(&server, "alpha", "first");
    seed_task_via(&server, "alpha", "second");

    let v = server
        .with_project(None, "test", |conn, cfg| {
            commands::list::list_value(conn, cfg, false, Some("alpha"))
        })
        .expect("with_project");
    let tasks = v["tasks"].as_array().expect("tasks array");
    assert_eq!(tasks.len(), 2);
}

fn seed_task_via(server: &SaraServer, project: &str, desc: &str) {
    server
        .with_project(None, "seed", |conn, _cfg| {
            seed_task(conn, project, desc);
            Ok(())
        })
        .expect("seed");
}

fn seed_returning(server: &SaraServer, project: &str, desc: &str) -> String {
    server
        .with_project(None, "seed", |conn, _cfg| {
            let mut t = Task::new(desc.to_string(), project.to_string());
            db::insert_task(conn, &mut t)?;
            Ok(t.uuid.to_string())
        })
        .expect("seed")
}

#[test]
fn done_value_marks_task_completed() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "finish me");
    let v = server
        .with_project(None, "done", |conn, cfg| {
            commands::done::done_value(conn, cfg, &uuid, false)
        })
        .expect("done");
    assert_eq!(v["status"], "completed");
    assert_eq!(v["recurrence"], serde_json::Value::Null);
}

#[test]
fn link_value_attaches_a_url() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let v = server
        .with_project(None, "link", |conn, _cfg| {
            commands::annotate::link_value(conn, &uuid, "https://example/pr/1", Some("PR"))
        })
        .expect("link");
    assert_eq!(v["url"], "https://example/pr/1");
}

#[test]
fn dep_on_then_list_reports_the_blocker() {
    let server = server_with(db::open_in_memory_for_test());
    let a = seed_returning(&server, "p", "dependent");
    let b = seed_returning(&server, "p", "blocker");
    server
        .with_project(None, "dep on", |conn, cfg| {
            commands::dep::dep_on_value(conn, cfg, &a, &b)
        })
        .expect("dep on");
    let v = server
        .with_project(None, "dep list", |conn, _cfg| {
            commands::dep::dep_list_value(conn, &a)
        })
        .expect("dep list");
    assert_eq!(v["blocked_by"].as_array().map(|a| a.len()), Some(1));
}

#[test]
fn check_value_adds_a_step() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let v = server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(conn, &uuid, "do the thing", None, None, None, None)
        })
        .expect("check");
    assert_eq!(v["kind"], db::STEP_KIND_STEP);
    let steps = server
        .with_project(None, "steps", |conn, _cfg| {
            commands::guide::steps_value(conn, &uuid, None)
        })
        .expect("steps");
    assert_eq!(steps["steps"].as_array().map(|a| a.len()), Some(1));
}

#[test]
fn verify_value_rejects_step_zero() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(conn, &uuid, "step one", None, None, None, None)
        })
        .expect("check");
    let zero = server.with_project(None, "verify", |conn, _cfg| {
        commands::guide::verify_value(conn, &uuid, Some(0))
    });
    assert!(zero.is_err(), "verify_value(step=0) should error");
    let one = server.with_project(None, "verify", |conn, _cfg| {
        commands::guide::verify_value(conn, &uuid, Some(1))
    });
    assert!(one.is_ok(), "verify_value(step=1) should succeed");
}

#[test]
fn modify_value_sets_priority_and_requires_a_field() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let v = server
        .with_project(None, "modify", |conn, cfg| {
            commands::modify::modify_value(
                conn,
                cfg,
                &uuid,
                None,
                Some("H"),
                None,
                false,
                &[],
                false,
                None,
                false,
                None,
                false,
            )
        })
        .expect("modify");
    assert_eq!(v["priority"], "H");

    let empty = server.with_project(None, "modify-empty", |conn, cfg| {
        commands::modify::modify_value(
            conn,
            cfg,
            &uuid,
            None,
            None,
            None,
            false,
            &[],
            false,
            None,
            false,
            None,
            false,
        )
    });
    assert!(empty.is_err(), "modify with no fields should error");
}

#[test]
fn step_done_current_ticks_the_first_not_done_step() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    for text in ["step one", "step two"] {
        server
            .with_project(None, "check", |conn, _cfg| {
                commands::guide::check_value(conn, &uuid, text, None, None, None, None)
            })
            .expect("check");
    }
    let first = server
        .with_project(None, "done-current", |conn, _cfg| {
            commands::guide::step_done_current_value(conn, &uuid, Some("did one"), None)
        })
        .expect("step_done_current");
    assert_eq!(first["index"], 1);
    assert_eq!(first["done"], true);
    let second = server
        .with_project(None, "done-current", |conn, _cfg| {
            commands::guide::step_done_current_value(conn, &uuid, Some("did two"), None)
        })
        .expect("step_done_current");
    assert_eq!(second["index"], 2);
    let none_left = server.with_project(None, "done-current", |conn, _cfg| {
        commands::guide::step_done_current_value(conn, &uuid, None, None)
    });
    assert!(none_left.is_err(), "no incomplete step should error");
}

#[test]
fn step_undone_then_remove_edit_the_checklist() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(conn, &uuid, "step one", None, None, None, None)
        })
        .expect("check");
    server
        .with_project(None, "done", |conn, _cfg| {
            commands::guide::step_done_value(conn, &uuid, 1, None, None)
        })
        .expect("step_done");
    let undone = server
        .with_project(None, "undone", |conn, _cfg| {
            commands::guide::step_undone_value(conn, &uuid, 1, None)
        })
        .expect("step_undone");
    assert_eq!(undone["done"], false);
    let removed = server
        .with_project(None, "remove", |conn, _cfg| {
            commands::guide::step_remove_value(conn, &uuid, 1, None)
        })
        .expect("step_remove");
    assert_eq!(removed["removed"], "step one");
    let steps = server
        .with_project(None, "steps", |conn, _cfg| {
            commands::guide::steps_value(conn, &uuid, None)
        })
        .expect("steps");
    assert_eq!(steps["steps"].as_array().map(|a| a.len()), Some(0));
}

#[test]
fn assignment_and_rationale_set_guide_text() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let a = server
        .with_project(None, "assignment", |conn, _cfg| {
            commands::guide::assignment_value(conn, &uuid, "build the thing")
        })
        .expect("assignment");
    assert_eq!(a["assignment"], "build the thing");
    let r = server
        .with_project(None, "rationale", |conn, _cfg| {
            commands::guide::rationale_value(conn, &uuid, "because reasons")
        })
        .expect("rationale");
    assert_eq!(r["rationale"], "because reasons");
}

#[test]
fn attach_value_records_a_file_and_an_anchor() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let file = server
        .with_project(None, "attach", |conn, _cfg| {
            commands::annotate::attach_value(conn, &uuid, "src/main.rs", None, None, None, None)
        })
        .expect("attach file");
    assert_eq!(file["kind"], "file");
    let anchor = server
        .with_project(None, "attach anchor", |conn, _cfg| {
            commands::annotate::attach_value(
                conn,
                &uuid,
                "src/lib.rs",
                Some("core logic"),
                None,
                Some("10:20"),
                None,
            )
        })
        .expect("attach anchor");
    assert_eq!(anchor["kind"], "anchor");
    assert_eq!(anchor["line_start"], 10);
    assert_eq!(anchor["line_end"], 20);
}

#[test]
fn attach_value_tags_a_url_as_link() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let v = server
        .with_project(None, "attach url", |conn, _cfg| {
            commands::annotate::attach_value(
                conn,
                &uuid,
                "https://example.com/pr/1",
                None,
                None,
                None,
                None,
            )
        })
        .expect("attach url");
    assert_eq!(v["kind"], "link");
    assert_eq!(v["url"], "https://example.com/pr/1");
}

#[test]
fn feedback_and_resolve_round_trip() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    server
        .with_project(None, "annotate", |conn, _cfg| {
            commands::annotate::annotate_value(
                conn,
                &uuid,
                &["please fix".to_string()],
                None,
                Some("human"),
                None,
                true,
            )
        })
        .expect("annotate");
    let fb = server
        .with_project(None, "feedback", |conn, _cfg| {
            commands::guide::feedback_value(conn, &uuid)
        })
        .expect("feedback");
    let items = fb["open_feedback"].as_array().expect("open_feedback array");
    assert_eq!(items.len(), 1);
    let fb_id = items[0]["id"].as_i64().expect("feedback id");
    let resolved = server
        .with_project(None, "resolve", |conn, _cfg| {
            commands::guide::resolve_value(conn, fb_id, None)
        })
        .expect("resolve");
    assert_eq!(resolved["resolved"], true);
}

#[test]
fn record_run_then_resolve_links_the_run_id() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let run = server
        .with_project(None, "record_run", |conn, _cfg| {
            commands::guide::record_run_value(
                conn,
                &uuid,
                "enrich",
                Some("claude-sonnet-5-thinking-high"),
                Some("cursor"),
                None,
                None,
                None,
                None,
                None,
            )
        })
        .expect("record_run");
    let run_id = run["run_id"].as_i64().expect("run_id");

    server
        .with_project(None, "annotate", |conn, _cfg| {
            commands::annotate::annotate_value(
                conn,
                &uuid,
                &["please fix".to_string()],
                None,
                Some("human"),
                None,
                true,
            )
        })
        .expect("annotate");
    let fb = server
        .with_project(None, "feedback", |conn, _cfg| {
            commands::guide::feedback_value(conn, &uuid)
        })
        .expect("feedback");
    let fb_id = fb["open_feedback"][0]["id"].as_i64().expect("feedback id");

    server
        .with_project(None, "resolve", |conn, _cfg| {
            commands::guide::resolve_value(conn, fb_id, Some(run_id))
        })
        .expect("resolve");

    server
        .with_project(None, "check", |conn, _cfg| {
            let stored: i64 = conn.query_row(
                "SELECT resolved_by_run FROM annotations WHERE id=?1",
                [fb_id],
                |r| r.get(0),
            )?;
            Ok(serde_json::json!({ "resolved_by_run": stored }))
        })
        .map(|v| assert_eq!(v["resolved_by_run"].as_i64(), Some(run_id)))
        .expect("resolved_by_run should be persisted");
}

#[test]
fn plan_show_value_returns_a_briefing() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "solo task");
    let v = server
        .with_project(None, "plan_show", |conn, _cfg| {
            commands::plan::show_value(conn, &uuid)
        })
        .expect("plan_show");
    assert_eq!(v["briefing"].as_array().map(|a| a.len()), Some(1));
}

use super::params::{CheckParams, StepDoneParams, StepEditParams};

#[test]
fn step_done_params_accept_task_and_index_aliases() {
    let p: StepDoneParams = serde_json::from_value(serde_json::json!({ "task": 14, "index": 1 }))
        .expect("{task,index} must deserialize into StepDoneParams");
    assert_eq!(p.id, "14");
    assert_eq!(p.n, Some(1));
}

#[test]
fn step_done_params_accept_string_id_and_step_id() {
    let p: StepDoneParams =
        serde_json::from_value(serde_json::json!({ "task": "14", "step_id": 1614 }))
            .expect("{task,step_id} must deserialize into StepDoneParams");
    assert_eq!(p.id, "14");
    assert_eq!(p.step_id, Some(1614));
}

#[test]
fn step_edit_params_accept_index_and_step_id_aliases() {
    let by_index: StepEditParams =
        serde_json::from_value(serde_json::json!({ "task": 14, "index": 2 }))
            .expect("{task,index} must deserialize into StepEditParams");
    assert_eq!(by_index.id, "14");
    assert_eq!(by_index.n, Some(2));
    let by_step_id: StepEditParams =
        serde_json::from_value(serde_json::json!({ "id": "14", "step_id": 99 }))
            .expect("{id,step_id} must deserialize into StepEditParams");
    assert_eq!(by_step_id.step_id, Some(99));
}

#[test]
fn check_params_accept_task_alias_as_number() {
    let p: CheckParams =
        serde_json::from_value(serde_json::json!({ "task": 14, "text": "criterion" }))
            .expect("{task,text} must deserialize into CheckParams");
    assert_eq!(p.id, "14");
    assert_eq!(p.text, "criterion");
}

#[test]
fn begin_params_accept_comma_separated_tags_string() {
    use super::params::BeginParams;
    let from_string: BeginParams = serde_json::from_value(serde_json::json!({
        "description": "restore a red PR",
        "tags": "ci,build,restitutio,pr-1384",
    }))
    .expect("comma-separated tags string must deserialize into BeginParams");
    assert_eq!(
        from_string.tags,
        Some(vec![
            "ci".to_string(),
            "build".to_string(),
            "restitutio".to_string(),
            "pr-1384".to_string(),
        ]),
        "the comma string is split, trimmed, into a tag vec"
    );
    let spaced: BeginParams = serde_json::from_value(serde_json::json!({
        "description": "d",
        "tags": "a, b ,,c",
    }))
    .expect("spaced/empty segments tolerated");
    assert_eq!(
        spaced.tags,
        Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
    );
    let from_array: BeginParams = serde_json::from_value(serde_json::json!({
        "description": "d",
        "tags": ["ci", "build"],
    }))
    .expect("array tags still deserialize");
    assert_eq!(
        from_array.tags,
        Some(vec!["ci".to_string(), "build".to_string()])
    );
    let none: BeginParams = serde_json::from_value(serde_json::json!({ "description": "d" }))
        .expect("absent tags default to None");
    assert_eq!(none.tags, None);
}

#[test]
fn check_value_returns_the_new_items_position() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let first = server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(conn, &uuid, "a", None, None, None, None)
        })
        .expect("check");
    assert_eq!(first["index"].as_u64(), Some(1), "first step is position 1");
    let second = server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(conn, &uuid, "b", None, None, None, None)
        })
        .expect("check");
    assert_eq!(
        second["index"].as_u64(),
        Some(2),
        "second step is position 2"
    );
}

#[test]
fn acceptance_without_verify_warns_but_a_plain_step_does_not() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let acc = server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(conn, &uuid, "done", None, Some("acceptance"), None, None)
        })
        .expect("check");
    assert!(
        acc["warning"]
            .as_str()
            .unwrap_or("")
            .contains("no verify command"),
        "acceptance without verify warns: {acc}"
    );
    let acc_ok = server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(
                conn,
                &uuid,
                "done",
                None,
                Some("acceptance"),
                None,
                Some("cargo test"),
            )
        })
        .expect("check");
    assert!(acc_ok["warning"].is_null(), "verify present, no warning");
    let step = server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(conn, &uuid, "a", None, None, None, None)
        })
        .expect("check");
    assert!(step["warning"].is_null(), "plain step never warns");
}

#[test]
fn step_done_by_step_id_ticks_the_right_item() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(conn, &uuid, "c1", None, Some("acceptance"), None, None)
        })
        .expect("check c1");
    let c2 = server
        .with_project(None, "check", |conn, _cfg| {
            commands::guide::check_value(conn, &uuid, "c2", None, Some("acceptance"), None, None)
        })
        .expect("check c2");
    let step_id = c2["step_id"].as_i64().expect("check returns step_id");
    let done = server
        .with_project(None, "done", |conn, _cfg| {
            commands::guide::step_done_by_id_value(conn, step_id, Some("proved"))
        })
        .expect("step_done_by_id");
    assert_eq!(done["kind"], db::STEP_KIND_ACCEPTANCE);
    assert_eq!(done["index"].as_u64(), Some(2));
    assert_eq!(done["done"], true);
}

#[test]
fn tools_carry_annotations() {
    const READ_ONLY: &[&str] = &[
        "list",
        "find",
        "info",
        "next",
        "steps",
        "verify",
        "recall",
        "feedback",
        "plan_show",
        "tags",
        "memories",
        "projects",
        "doctor",
        "resume",
    ];
    const DESTRUCTIVE: &[&str] = &[
        "done",
        "forget",
        "step_remove",
        "unlink",
        "denotate",
        "link_memory",
        "maintain",
        "modify",
        "relearn",
        "dep",
        "finish",
    ];
    let tools = SaraServer::all_router().list_all();
    assert_eq!(tools.len(), 41);
    for t in &tools {
        let name = t.name.as_ref();
        let a = t
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("tool `{name}` has no annotations"));
        let read_only = READ_ONLY.contains(&name);
        assert_eq!(a.read_only_hint, Some(read_only), "`{name}` readOnlyHint");
        assert_eq!(
            a.open_world_hint,
            Some(matches!(name, "validate" | "finish")),
            "`{name}` openWorldHint"
        );
        if read_only {
            assert_eq!(a.destructive_hint, None, "`{name}` is read-only");
        } else {
            assert_eq!(
                a.destructive_hint,
                Some(DESTRUCTIVE.contains(&name)),
                "`{name}` destructiveHint"
            );
            assert!(a.idempotent_hint.is_some(), "`{name}` idempotentHint");
        }
    }
}

fn tool_props(name: &str) -> serde_json::Map<String, serde_json::Value> {
    SaraServer::all_router()
        .list_all()
        .iter()
        .find(|t| t.name == name)
        .and_then(|t| {
            t.input_schema
                .get("properties")
                .and_then(|v| v.as_object())
                .cloned()
        })
        .unwrap_or_default()
}

#[test]
fn mcp_doing_tool_requires_id_and_text() {
    let props = tool_props("doing");
    assert!(props.contains_key("id") && props.contains_key("text"));
}

#[test]
fn mcp_doing_param_is_offered_on_step_done_and_annotate_only() {
    assert!(tool_props("step_done").contains_key("doing"));
    assert!(tool_props("annotate").contains_key("doing"));
    assert!(!tool_props("next").contains_key("doing"));
}

#[test]
fn mcp_doing_records_the_line_for_the_task() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let v = server
        .with_project(None, "doing", |conn, _cfg| {
            commands::doing::doing_value(conn, &uuid[..8], "  running   tests ", Some("copilot"))
        })
        .expect("doing");
    assert_eq!(v["doing"], "running tests");
    let latest = server
        .with_project(None, "read", |conn, _cfg| {
            db::latest_doing(conn, &uuid::Uuid::parse_str(&uuid)?)
        })
        .expect("read")
        .expect("an entry");
    assert_eq!(latest.client.as_deref(), Some("copilot"));
}

#[test]
fn mcp_doing_rejects_empty_text() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let r = server.with_project(None, "doing", |conn, _cfg| {
        commands::doing::doing_value(conn, &uuid, "   ", None)
    });
    assert!(r.is_err());
}

#[test]
fn mcp_doing_report_piggybacks_on_another_result() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let v = server
        .with_project(None, "step", |conn, _cfg| {
            server.report_doing(
                conn,
                &uuid,
                Some("next: wire the CLI"),
                serde_json::json!({"done": true}),
            )
        })
        .expect("report");
    assert_eq!(v["done"], true);
    assert_eq!(v["doing"], "next: wire the CLI");

    let untouched = server
        .with_project(None, "step", |conn, _cfg| {
            server.report_doing(conn, &uuid, Some("  "), serde_json::json!({"done": true}))
        })
        .expect("report");
    assert!(untouched.get("doing").is_none());
}

fn action_tools(v: &serde_json::Value) -> Vec<String> {
    v["next_actions"]
        .as_array()
        .expect("next_actions array")
        .iter()
        .map(|a| a["tool"].as_str().unwrap_or("").to_string())
        .collect()
}

fn actions_for(server: &SaraServer, id: &str) -> Vec<String> {
    let v = server
        .with_project(None, "t", |conn, _cfg| {
            let task = db::resolve_task(conn, id)?;
            Ok(serde_json::json!({
                "next_actions": commands::agent::next_actions(conn, &task)?
            }))
        })
        .unwrap();
    action_tools(&v)
}

#[test]
fn next_actions_walk_the_task_lifecycle() {
    let server = server_with(db::open_in_memory_for_test());
    let id = seed_returning(&server, "proj", "ship the thing");
    assert_eq!(actions_for(&server, &id), ["check"], "no plan yet");

    server
        .with_project(None, "t", |conn, _cfg| {
            commands::guide::check_value(conn, &id, "write it", None, None, None, None)
        })
        .unwrap();
    assert_eq!(actions_for(&server, &id), ["step_done"], "open step");

    let ticked = server
        .with_task(None, "t", &id, |conn, _cfg| {
            commands::guide::step_done_current_value(conn, &id, Some("written"), None)
        })
        .unwrap();
    assert_eq!(
        action_tools(&ticked),
        ["check", "link"],
        "with_task attaches next_actions: no acceptance criterion, no PR"
    );

    server
        .with_project(None, "t", |conn, _cfg| {
            commands::guide::check_value(
                conn,
                &id,
                "tests pass",
                None,
                Some("acceptance"),
                None,
                Some("true"),
            )
        })
        .unwrap();
    assert_eq!(
        actions_for(&server, &id),
        ["step_done"],
        "unticked criterion"
    );

    server
        .with_project(None, "t", |conn, _cfg| {
            commands::guide::step_done_current_value(conn, &id, Some("green"), Some("acceptance"))
        })
        .unwrap();
    assert_eq!(actions_for(&server, &id), ["validate", "link"]);

    server
        .with_project(None, "t", |conn, _cfg| {
            commands::annotate::link_value(conn, &id, "https://github.com/o/r/pull/7", None)
        })
        .unwrap();
    assert_eq!(actions_for(&server, &id), ["validate", "finish"]);

    server
        .with_project(None, "t", |conn, cfg| {
            commands::done::done_value(conn, cfg, &id, true)
        })
        .unwrap();
    assert!(actions_for(&server, &id).is_empty(), "completed task");
}

#[test]
fn finish_waits_for_the_merge_and_links_the_pr_once() {
    let server = server_with(db::open_in_memory_for_test());
    let id = seed_returning(&server, "proj", "finish me");
    let url = "https://github.com/o/r/pull/9";
    for _ in 0..2 {
        let v = server
            .with_project(None, "t", |conn, cfg| {
                commands::agent::finish_value(
                    conn,
                    cfg,
                    &commands::agent::FinishRequest {
                        id: &id,
                        pr_url: Some(url),
                        merged: Some(false),
                        used: &[],
                    },
                )
            })
            .unwrap();
        assert_eq!(v["status"], "waiting_for_merge");
        assert_eq!(v["next_actions"][0]["tool"], "finish");
    }
    let (links, status) = server
        .with_project(None, "t", |conn, _cfg| {
            let t = db::resolve_task(conn, &id)?;
            Ok((db::get_links(conn, &t.uuid)?.len(), t.status))
        })
        .unwrap();
    assert_eq!(links, 1, "the PR is linked exactly once");
    assert_eq!(status, crate::infrastructure::model::Status::Pending);

    let err = server
        .with_project(None, "t", |conn, cfg| {
            commands::agent::finish_value(
                conn,
                cfg,
                &commands::agent::FinishRequest {
                    id: &id,
                    merged: Some(true),
                    ..Default::default()
                },
            )
        })
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("stays open"),
        "a red gate keeps the task open: {err:#}"
    );
}

#[test]
fn finish_closes_a_merged_task_validated_at_head() {
    let server = server_with(db::open_in_memory_for_test());
    let repo = crate::test_support::git_repo();
    std::fs::write(repo.path().join("f.txt"), "hi").unwrap();
    crate::test_support::commit_all(repo.path(), "init");
    let id = seed_returning(&server, "proj", "merged work");
    let v = server
        .with_project(None, "t", |conn, cfg| {
            db::upsert_project_seen(conn, "proj", Some(repo.path().to_str().unwrap()))?;
            commands::guide::check_value(
                conn,
                &id,
                "true holds",
                None,
                Some("acceptance"),
                None,
                Some("true"),
            )?;
            commands::guide::step_done_current_value(conn, &id, Some("ok"), Some("acceptance"))?;
            commands::agent::finish_value(
                conn,
                cfg,
                &commands::agent::FinishRequest {
                    id: &id,
                    pr_url: Some("https://github.com/o/r/pull/3"),
                    merged: Some(true),
                    used: &[],
                },
            )
        })
        .unwrap();
    assert_eq!(v["status"], "done", "{v}");
    assert!(v["validated_commit"].is_string());
    assert_eq!(v["done"]["uuid"], id);
}

#[test]
fn resume_briefs_the_most_recent_task() {
    let server = server_with(db::open_in_memory_for_test());
    let empty = server
        .with_project(None, "t", |conn, cfg| {
            commands::agent::resume_value(conn, cfg, None)
        })
        .unwrap();
    assert!(empty["task"].is_null());
    assert_eq!(empty["next_actions"][0]["tool"], "begin");

    let project = server
        .with_project(None, "t", |conn, cfg| {
            Ok(crate::infrastructure::project::detect_current_project(conn, cfg)?.0)
        })
        .unwrap();
    let id = seed_returning(&server, &project, "pick me up");
    let v = server
        .with_project(None, "t", |conn, cfg| {
            commands::guide::check_value(conn, &id, "first", None, None, None, None)?;
            commands::guide::check_value(conn, &id, "second", None, None, None, None)?;
            commands::guide::step_done_current_value(conn, &id, Some("did first"), None)?;
            commands::doing::doing_value(conn, &id, "reading the code", None)?;
            commands::agent::resume_value(conn, cfg, None)
        })
        .unwrap();
    assert_eq!(v["task"]["uuid"], id);
    assert_eq!(v["progress"], "1/2 steps, 0/0 acceptance");
    assert_eq!(v["done_steps"][0]["result"], "did first");
    assert_eq!(v["open_steps"][0]["text"], "second");
    assert_eq!(v["doing"]["text"], "reading the code");
    assert_eq!(v["next_actions"][0]["tool"], "step_done");
}

fn learn(server: &SaraServer, text: &str, tags: &[&str], projects: &[&str]) -> String {
    let tags: Vec<String> = tags.iter().map(|s| s.to_string()).collect();
    let projects: Vec<String> = projects.iter().map(|s| s.to_string()).collect();
    server
        .with_project(None, "t", |conn, cfg| {
            commands::learn::learn_value(
                conn,
                cfg,
                &commands::learn::LearnRequest {
                    text,
                    tags: &tags,
                    projects: &projects,
                    ..Default::default()
                },
            )
        })
        .unwrap()["label"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn tags_and_memories_are_scoped_to_the_project() {
    let server = server_with(db::open_in_memory_for_test());
    let project = server
        .with_project(None, "t", |conn, cfg| {
            Ok(crate::infrastructure::project::detect_current_project(conn, cfg)?.0)
        })
        .unwrap();
    learn(&server, "mine", &["mcp-here"], &[&project]);
    learn(&server, "theirs", &["elsewhere"], &["other-project"]);
    let global = learn(&server, "everywhere", &["mcp-global"], &[]);
    server
        .with_project(None, "t", |conn, _cfg| {
            let item = db::get_item_by_handle(conn, &global)?;
            db::set_item_projects(conn, &item.uuid, &[])
        })
        .unwrap();

    let (tags, all_tags, prefixed, memories, capped) = server
        .with_project(None, "t", |conn, cfg| {
            use commands::agent::{Scope, memories_value, tags_value};
            Ok((
                tags_value(conn, cfg, Scope::Project, None, None)?,
                tags_value(conn, cfg, Scope::All, None, None)?,
                tags_value(conn, cfg, Scope::All, Some("MCP"), Some(1))?,
                memories_value(conn, cfg, Scope::Project, None)?,
                memories_value(conn, cfg, Scope::All, Some(1))?,
            ))
        })
        .unwrap();
    let names = |v: &serde_json::Value| -> Vec<String> {
        let mut n: Vec<String> = v["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["tag"].as_str().unwrap().to_string())
            .collect();
        n.sort();
        n
    };
    assert_eq!(names(&tags), ["mcp-global", "mcp-here"]);
    assert_eq!(tags["scope"], project);
    assert_eq!(names(&all_tags).len(), 3);
    assert_eq!(all_tags["scope"], "all");
    assert_eq!(prefixed["tags"].as_array().unwrap().len(), 1);
    assert_eq!(prefixed["total"], 2);
    assert_eq!(prefixed["truncated"], true);
    assert_eq!(
        memories["total"], 2,
        "own + global, not the other project's"
    );
    assert_eq!(capped["total"], 3);
    assert_eq!(capped["memories"].as_array().unwrap().len(), 1);
}

#[test]
fn scope_rejects_unknown_values() {
    assert!(commands::agent::Scope::parse(Some("everything")).is_err());
    assert_eq!(
        commands::agent::Scope::parse(None).unwrap(),
        commands::agent::Scope::Project
    );
}

#[test]
fn learn_reports_possible_duplicates_and_links_typed() {
    let server = server_with(db::open_in_memory_for_test());
    let first = learn(&server, "retry the flaky upload", &["upload", "retry"], &[]);
    let v = server
        .with_project(None, "t", |conn, cfg| {
            let tags = vec!["upload".to_string(), "retry".to_string()];
            commands::learn::learn_value(
                conn,
                cfg,
                &commands::learn::LearnRequest {
                    text: "uploads need a retry with backoff",
                    tags: &tags,
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let dupes = v["possible_duplicates"].as_array().unwrap();
    assert_eq!(dupes.len(), 1, "{v}");
    assert_eq!(dupes[0]["label"], first);
    assert!(
        dupes[0]["reasons"]
            .as_array()
            .unwrap()
            .contains(&"same_tags".into())
    );
    assert!(
        dupes[0]["suggestion"]
            .as_str()
            .unwrap()
            .contains("supersedes")
    );

    let linked = server
        .with_project(None, "t", |conn, cfg| {
            let tags = vec!["upload".to_string(), "retry".to_string()];
            let supersedes = vec![first.clone()];
            commands::learn::learn_value(
                conn,
                cfg,
                &commands::learn::LearnRequest {
                    text: "uploads retry three times with jittered backoff",
                    tags: &tags,
                    supersedes: &supersedes,
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert_eq!(linked["superseded"][0], first);
    assert!(
        !linked["possible_duplicates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["label"] == first),
        "a memory linked in the same call is not reported as a duplicate"
    );
}

#[test]
fn recall_matches_relative_file_filters() {
    let server = server_with(db::open_in_memory_for_test());
    let label = server
        .with_project(None, "t", |conn, cfg| {
            let files = vec!["src/mcp/read.rs".to_string()];
            commands::learn::learn_value(
                conn,
                cfg,
                &commands::learn::LearnRequest {
                    text: "read tools live in read.rs",
                    files: &files,
                    ..Default::default()
                },
            )
        })
        .unwrap()["label"]
        .as_str()
        .unwrap()
        .to_string();
    for filter in ["src/mcp/read.rs", "src/mcp/"] {
        let v = server
            .with_project(None, "t", |conn, cfg| {
                commands::recall::recall_value(
                    conn,
                    cfg,
                    "",
                    &[],
                    &[],
                    &[filter.to_string()],
                    10,
                    false,
                )
            })
            .unwrap();
        let hits = v["keyword"].as_array().unwrap();
        assert!(
            hits.iter().any(|h| h["label"] == label),
            "relative filter {filter:?} should match: {v}"
        );
    }
}

#[test]
fn maintain_dispatches_actions() {
    let server = server_with(db::open_in_memory_for_test());
    let v = server
        .with_project(None, "t", |conn, _cfg| {
            commands::agent::maintain_value(
                conn,
                &commands::agent::MaintainRequest {
                    action: "consolidate",
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert_eq!(v["action"], "consolidate");
    assert!(v["result"]["reinforced"].is_number());
    let err = server
        .with_project(None, "t", |conn, _cfg| {
            commands::agent::maintain_value(
                conn,
                &commands::agent::MaintainRequest {
                    action: "explode",
                    ..Default::default()
                },
            )
        })
        .unwrap_err();
    assert!(err.to_string().contains("diagnose"));
}

#[test]
fn new_tool_params_are_exposed() {
    for (tool, prop) in [
        ("done", "validate"),
        ("modify", "assignment"),
        ("modify", "rationale"),
        ("link_memory", "remove"),
        ("learn", "supersedes"),
        ("learn", "derived_from"),
        ("learn", "similar_to"),
        ("tags", "scope"),
        ("tags", "prefix"),
        ("memories", "limit"),
        ("finish", "merged"),
        ("maintain", "action"),
    ] {
        assert!(
            tool_props(tool).contains_key(prop),
            "`{tool}` should accept `{prop}`"
        );
    }
}
