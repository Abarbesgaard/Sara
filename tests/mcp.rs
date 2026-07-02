//! Integration tests for `sara_tasks::commands::mcp`.
//! Moved out of src/commands/mcp/tests.rs (already its own file, just not
//! under tests/).
//!
//! Mutates the process-wide working directory via CwdGuard, guarded by a
//! local mutex -- keep all such tests in this one file so the mutex keeps
//! working (each tests/*.rs file is compiled as its own separate test
//! binary/process).

use std::sync::Mutex;

use rmcp::ServerHandler;
use rusqlite::Connection;

use sara_tasks::commands;
use sara_tasks::infrastructure::config::Config;
use sara_tasks::infrastructure::db;
use sara_tasks::infrastructure::model::Task;

use sara_tasks::commands::mcp::server::{CwdGuard, SaraServer};

// cwd is process-global; serialize the tests that mutate it.
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn server_with(conn: Connection) -> SaraServer {
    SaraServer::new(conn, Config::default())
}

fn seed_task(conn: &Connection, project: &str, desc: &str) {
    let mut t = Task::new(desc.to_string(), project.to_string());
    db::insert_task(conn, &mut t).expect("insert task");
}

#[test]
fn exposes_the_agent_loop_tools() {
    let names: Vec<String> = SaraServer::all_router()
        .list_all()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(names.len(), 26, "expected 26 tools, got {names:?}");
    for expected in [
        // read
        "list",
        "info",
        "next",
        "steps",
        "verify",
        "recall",
        "feedback",
        "plan_show",
        // mutate (create / guide)
        "add",
        "step_done",
        "annotate",
        "plan_import",
        "check",
        "step_undone",
        "step_remove",
        "assignment",
        "rationale",
        "attach",
        // completion / edit / lifecycle
        "done",
        "link",
        "dep",
        "validate",
        "modify",
        "resolve",
        "start",
        "stop",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing tool `{expected}` in {names:?}"
        );
    }
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

    // None / empty leaves cwd untouched.
    {
        let _g = CwdGuard::enter(None).unwrap();
        assert_eq!(std::env::current_dir().unwrap(), start);
    }
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

    // project_path=None avoids touching process cwd; filter by explicit project.
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

/// Seed a task and return its uuid string (for targeting mutate tools).
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
    // step is 1-based: 0 must error, not silently return step 1.
    let zero = server.with_project(None, "verify", |conn, _cfg| {
        commands::guide::verify_value(conn, &uuid, Some(0))
    });
    assert!(zero.is_err(), "verify_value(step=0) should error");
    // step 1 is valid.
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
            )
        })
        .expect("modify");
    assert_eq!(v["priority"], "H");

    // No field flags → must error, never open the TUI.
    let empty = server.with_project(None, "modify-empty", |conn, cfg| {
        commands::modify::modify_value(conn, cfg, &uuid, None, None, None, false, &[], false)
    });
    assert!(empty.is_err(), "modify with no fields should error");
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
    // done → undone flips it back to not-done.
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
    // remove drops the item.
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
    // PR #58 review: the URL branch delegates to link_value but must still carry a
    // `kind`, so every attach result shape is discriminable (file / anchor / link).
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
fn start_then_stop_tracks_a_session() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    let started = server
        .with_project(None, "start", |conn, cfg| {
            commands::timer::start_value(conn, cfg, &uuid)
        })
        .expect("start");
    assert_eq!(started["started"], true);
    let stopped = server
        .with_project(None, "stop", |conn, cfg| {
            commands::timer::stop_value(conn, cfg, &uuid)
        })
        .expect("stop");
    assert_eq!(stopped["stopped"], true);
    assert!(stopped["session_seconds"].as_i64().is_some());
}

#[test]
fn feedback_and_resolve_round_trip() {
    let server = server_with(db::open_in_memory_for_test());
    let uuid = seed_returning(&server, "p", "task");
    // Seed an open feedback item via a human annotation flagged for revision.
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
            commands::guide::resolve_value(conn, fb_id)
        })
        .expect("resolve");
    assert_eq!(resolved["resolved"], true);
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
