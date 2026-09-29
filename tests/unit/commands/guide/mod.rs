use super::*;
use crate::infrastructure::config::Config;
use crate::infrastructure::model::Task;

fn cfg() -> Config {
    Config::default()
}

fn sh_arg(p: &std::path::Path) -> String {
    p.display().to_string().replace('\\', "/")
}

#[test]
fn next_surfaces_a_strong_relevant_memory() {
    use crate::infrastructure::model::Item;
    let conn = db::open_in_memory_for_test();

    let mut src = Task::new("source work".into(), "proj".into());
    src.status = crate::infrastructure::model::Status::Completed;
    db::insert_task(&conn, &mut src).unwrap();

    let mut mem = Item::new_memory(
        "dependabot bump restore pattern".into(),
        "dependabot bump broke restore; align versions".into(),
        Some(src.uuid),
    );
    mem.tags = vec!["dependabot".into()];
    mem.path = Some(String::new());
    db::insert_item(&conn, &mut mem).unwrap();
    db::set_item_tags(&conn, &mem.uuid, &["dependabot".into()]).unwrap();

    let mut task = Task::new("do the dependabot bump".into(), "proj".into());
    task.tags = vec!["dependabot".into()];
    db::insert_task(&conn, &mut task).unwrap();
    db::add_step(
        &conn,
        &task.uuid,
        "step one",
        None,
        db::STEP_KIND_STEP,
        "human",
        None,
    )
    .unwrap();

    let v = next_value(&conn, &task.uuid.to_string()).unwrap();
    let mems = v["relevant_memories"]
        .as_array()
        .expect("relevant_memories present when a Strong memory matches");
    assert_eq!(mems.len(), 1);
    assert!(mems[0]["label"].as_str().unwrap().starts_with('m'));
    assert!(!mems[0]["snippet"].as_str().unwrap().is_empty());
}

#[test]
fn next_omits_the_block_when_no_memory_matches() {
    let conn = db::open_in_memory_for_test();
    let mut task = Task::new("unrelated task".into(), "proj".into());
    task.tags = vec!["nothing-matches-this".into()];
    db::insert_task(&conn, &mut task).unwrap();
    db::add_step(
        &conn,
        &task.uuid,
        "step one",
        None,
        db::STEP_KIND_STEP,
        "human",
        None,
    )
    .unwrap();

    let v = next_value(&conn, &task.uuid.to_string()).unwrap();
    assert!(
        v.get("relevant_memories").is_none(),
        "no relevant_memories key when nothing Strong matches"
    );
}

#[test]
fn tick_on_pass_ticks_passing_criteria_and_activates_task() {
    let conn = db::open_in_memory_for_test();
    let mut task = Task::new("demo".into(), "proj".into());
    db::insert_task(&conn, &mut task).unwrap();
    db::add_step(
        &conn,
        &task.uuid,
        "passes",
        None,
        db::STEP_KIND_ACCEPTANCE,
        "human",
        Some("true"),
    )
    .unwrap();
    db::add_step(
        &conn,
        &task.uuid,
        "fails",
        None,
        db::STEP_KIND_ACCEPTANCE,
        "human",
        Some("false"),
    )
    .unwrap();

    let id = task.uuid.to_string();
    verify(&conn, &cfg(), &id, None, false, true).unwrap();

    let acc = db::get_steps(&conn, &task.uuid, db::STEP_KIND_ACCEPTANCE).unwrap();
    assert!(acc[0].done, "criterion with a passing verify_cmd is ticked");
    assert!(acc[0].result.as_deref().unwrap_or("").contains("passed"));
    assert!(
        !acc[1].done,
        "criterion whose verify_cmd fails stays unticked"
    );

    let reloaded = db::get_task_by_uuid_prefix(&conn, &id).unwrap().unwrap();
    assert!(reloaded.started_at.is_some());
}

fn task_with_acceptance(conn: &Connection, verify: Option<&str>) -> Task {
    let mut task = Task::new("gate demo".into(), "proj".into());
    db::insert_task(conn, &mut task).unwrap();
    db::add_step(
        conn,
        &task.uuid,
        "criterion",
        None,
        db::STEP_KIND_ACCEPTANCE,
        "human",
        verify,
    )
    .unwrap();
    task
}

#[test]
fn gate_is_green_only_when_every_criterion_has_a_passing_verify() {
    let conn = db::open_in_memory_for_test();
    let task = task_with_acceptance(&conn, Some("true"));
    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, false).unwrap();
    assert!(gate.is_green(), "one criterion, verify passes → green");
    assert_eq!(gate.passed, 1);
}

#[test]
fn gate_red_when_verify_command_fails() {
    let conn = db::open_in_memory_for_test();
    let task = task_with_acceptance(&conn, Some("false"));
    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, false).unwrap();
    assert!(!gate.is_green(), "failing verify → red");
    assert_eq!(gate.failures.len(), 1);
}

#[test]
fn gate_red_when_a_criterion_has_no_verify_command() {
    let conn = db::open_in_memory_for_test();
    let task = task_with_acceptance(&conn, None);
    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, false).unwrap();
    assert!(!gate.is_green(), "unprovable criterion → red");
    assert_eq!(gate.missing_verify.len(), 1);
}

#[test]
fn gate_red_when_no_acceptance_criteria_exist() {
    let conn = db::open_in_memory_for_test();
    let mut task = Task::new("no criteria".into(), "proj".into());
    db::insert_task(&conn, &mut task).unwrap();
    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, false).unwrap();
    assert!(!gate.is_green(), "no definition of done → red");
    assert_eq!(gate.total, 0);
}

#[test]
fn capture_mode_collects_command_output_instead_of_printing_it() {
    let conn = db::open_in_memory_for_test();
    let task = task_with_acceptance(&conn, Some("echo MARKER_ON_STDOUT; exit 1"));
    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, false).unwrap();

    assert!(!gate.is_green(), "exit 1 → red");
    assert_eq!(gate.transcript.len(), 1);
    assert!(
        gate.transcript[0].output.contains("MARKER_ON_STDOUT"),
        "child stdout must be captured, got {:?}",
        gate.transcript[0].output
    );
    assert_eq!(gate.transcript[0].exit_code, Some(1));
    assert!(
        gate.failure_detail().contains("MARKER_ON_STDOUT"),
        "failure detail must surface the captured output to the agent"
    );
}

#[test]
fn capture_mode_records_stderr_too() {
    let conn = db::open_in_memory_for_test();
    let task = task_with_acceptance(&conn, Some("echo OOPS 1>&2; exit 3"));
    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, false).unwrap();
    assert_eq!(gate.transcript[0].exit_code, Some(3));
    assert!(gate.transcript[0].output.contains("OOPS"));
}

fn init_git_repo() -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("sara-gate-git-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(&dir)
            .args(args)
            .output()
            .unwrap();
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "t@t.com"]);
    run(&["config", "user.name", "t"]);
    std::fs::write(dir.join("f.txt"), "hi").unwrap();
    run(&["add", "-A"]);
    run(&["commit", "-m", "init"]);
    let head = crate::infrastructure::git::head_commit(&dir).unwrap();
    (dir, head)
}

#[test]
fn gate_deduplicates_identical_verify_commands() {
    let conn = db::open_in_memory_for_test();
    let marker = std::env::temp_dir().join(format!("sara-gate-dedup-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_file(&marker);
    let cmd = format!("echo x >> {}", sh_arg(&marker));

    let mut task = Task::new("dedup".into(), "proj".into());
    db::insert_task(&conn, &mut task).unwrap();
    for _ in 0..3 {
        db::add_step(
            &conn,
            &task.uuid,
            "criterion",
            None,
            db::STEP_KIND_ACCEPTANCE,
            "human",
            Some(&cmd),
        )
        .unwrap();
    }

    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, false).unwrap();
    assert!(gate.is_green(), "all criteria pass → green");
    assert_eq!(gate.ran, 1, "identical command executes exactly once");
    assert_eq!(gate.passed, 3, "all three criteria counted green");
    let lines = std::fs::read_to_string(&marker).unwrap().lines().count();
    assert_eq!(lines, 1, "command body ran exactly once");
    let _ = std::fs::remove_file(&marker);
}

#[test]
fn gate_caches_criteria_already_proven_at_head() {
    let conn = db::open_in_memory_for_test();
    let (repo, head) = init_git_repo();
    let marker = std::env::temp_dir().join(format!("sara-gate-cache-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_file(&marker);
    let cmd = format!("echo ran >> {}", sh_arg(&marker));

    let mut task = Task::new("cache".into(), "proj".into());
    db::insert_task(&conn, &mut task).unwrap();
    db::upsert_project_seen(&conn, "proj", Some(repo.to_str().unwrap())).unwrap();
    let sid = db::add_step(
        &conn,
        &task.uuid,
        "criterion",
        None,
        db::STEP_KIND_ACCEPTANCE,
        "human",
        Some(&cmd),
    )
    .unwrap();
    db::set_step_done(&conn, sid, true, Some("pre"), Some(&head)).unwrap();

    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, false).unwrap();
    assert!(gate.is_green(), "cached pass keeps the gate green");
    assert_eq!(gate.cached, 1, "criterion served from cache");
    assert_eq!(gate.ran, 0, "cached criterion is not executed");
    assert!(!marker.exists(), "verify command must not run when cached");

    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, true).unwrap();
    assert_eq!(gate.cached, 0, "--fresh ignores the cache");
    assert_eq!(gate.ran, 1, "--fresh re-runs the verify command");
    assert!(marker.exists(), "fresh run executes the command");

    let _ = std::fs::remove_file(&marker);
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn gate_bypasses_cache_when_tree_is_dirty() {
    let conn = db::open_in_memory_for_test();
    let (repo, head) = init_git_repo();
    let marker = std::env::temp_dir().join(format!("sara-gate-dirty-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_file(&marker);
    let cmd = format!("echo ran >> {}", sh_arg(&marker));

    let mut task = Task::new("dirty".into(), "proj".into());
    db::insert_task(&conn, &mut task).unwrap();
    db::upsert_project_seen(&conn, "proj", Some(repo.to_str().unwrap())).unwrap();
    let sid = db::add_step(
        &conn,
        &task.uuid,
        "criterion",
        None,
        db::STEP_KIND_ACCEPTANCE,
        "human",
        Some(&cmd),
    )
    .unwrap();
    db::set_step_done(&conn, sid, true, Some("pre"), Some(&head)).unwrap();

    std::fs::write(repo.join("dirty.txt"), "x").unwrap();

    let gate =
        run_acceptance_gate(&conn, &task.uuid.to_string(), GateOutput::Capture, false).unwrap();
    assert_eq!(gate.cached, 0, "dirty tree disables the cache");
    assert_eq!(gate.ran, 1, "dirty tree forces a re-run");
    assert!(
        marker.exists(),
        "verify command runs when the tree is dirty"
    );

    let _ = std::fs::remove_file(&marker);
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn captured_output_is_truncated_on_a_char_boundary() {
    let long = "é".repeat(5000);
    let out = tail_limited(&long, 4000);
    assert!(out.contains("truncated"));
    assert!(out.chars().count() < 4100);
    assert_eq!(tail_limited("short", 4000), "short");
}

#[test]
fn step_done_value_reports_activation_on_first_work() {
    let conn = db::open_in_memory_for_test();
    let mut task = Task::new("demo".into(), "proj".into());
    db::insert_task(&conn, &mut task).unwrap();
    db::add_step(
        &conn,
        &task.uuid,
        "do it",
        None,
        db::STEP_KIND_STEP,
        "human",
        None,
    )
    .unwrap();

    let id = task.uuid.to_string();
    let v = step_done_value(&conn, &id, 1, None, None).unwrap();
    assert_eq!(
        v["activated"], true,
        "first recorded work activates the task"
    );

    db::add_step(
        &conn,
        &task.uuid,
        "again",
        None,
        db::STEP_KIND_STEP,
        "human",
        None,
    )
    .unwrap();
    let v2 = step_done_value(&conn, &id, 2, None, None).unwrap();
    assert_eq!(v2["activated"], false);
}
