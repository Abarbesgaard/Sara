use super::*;
use crate::infrastructure::db::STEP_KIND_STEP;
use crate::infrastructure::model::Task;

fn seed(conn: &Connection, desc: &str, project: &str) -> Task {
    let mut task = Task::new(desc.into(), project.into());
    db::insert_task(conn, &mut task).unwrap();
    task
}

fn tmp_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("sara-bundle-{}.txt", Uuid::new_v4()))
}

fn find_by_desc<'a>(tasks: &'a [Task], desc: &str) -> &'a Task {
    tasks
        .iter()
        .find(|t| t.description == desc)
        .unwrap_or_else(|| panic!("no imported task named {desc:?}"))
}

#[test]
fn export_then_import_round_trips_a_dependency_graph() {
    let src = db::open_in_memory_for_test();
    let root = seed(&src, "root task", "origin");
    let dep = seed(&src, "blocker task", "origin");
    db::add_dependency(&src, &root.uuid, &dep.uuid).unwrap();
    db::add_step(&src, &root.uuid, "do the thing", None, STEP_KIND_STEP, "human", None).unwrap();
    db::add_annotation_full(&src, &root.uuid, "watch out", "finding", "human", None, None, false)
        .unwrap();
    db::add_link(&src, &root.uuid, "https://example.com", Some("docs")).unwrap();
    db::set_task_files(&src, &root.uuid, &["src/main.rs".into()]).unwrap();

    let path = tmp_path();
    crate::commands::export::run(&src, &root.uuid.to_string(), Some(path.as_path())).unwrap();

    let mut dst = db::open_in_memory_for_test();
    let cfg = Config::default();
    run(&mut dst, &cfg, Some(path.to_str().unwrap()), None).unwrap();
    let _ = std::fs::remove_file(&path);

    let tasks = db::list_tasks(&dst, None).unwrap();
    assert_eq!(tasks.len(), 2, "both the root and its blocker must be imported");

    let imported_root = find_by_desc(&tasks, "root task");
    let imported_dep = find_by_desc(&tasks, "blocker task");
    assert_eq!(imported_root.project, "origin", "the project must be preserved");

    let blockers = db::get_blockers(&dst, &imported_root.uuid).unwrap();
    assert_eq!(
        blockers,
        vec![imported_dep.uuid],
        "the dependency edge must be remapped onto the freshly inserted uuids"
    );

    assert_eq!(
        db::get_steps(&dst, &imported_root.uuid, STEP_KIND_STEP).unwrap().len(),
        1,
        "the step must survive the round trip"
    );
    assert_eq!(
        db::get_annotations(&dst, &imported_root.uuid).unwrap().len(),
        1,
        "the annotation must survive the round trip"
    );
    assert_eq!(
        db::get_links(&dst, &imported_root.uuid).unwrap().len(),
        1,
        "the link must survive the round trip"
    );
    let files = db::get_task_files_sourced(&dst, &imported_root.uuid).unwrap();
    assert_eq!(
        files.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
        vec!["src/main.rs"],
        "the relevant files must survive the round trip"
    );
}

#[test]
fn import_reassigns_every_task_when_project_is_overridden() {
    let src = db::open_in_memory_for_test();
    let root = seed(&src, "root task", "origin");
    let dep = seed(&src, "blocker task", "origin");
    db::add_dependency(&src, &root.uuid, &dep.uuid).unwrap();

    let path = tmp_path();
    crate::commands::export::run(&src, &root.uuid.to_string(), Some(path.as_path())).unwrap();

    let mut dst = db::open_in_memory_for_test();
    let cfg = Config::default();
    run(&mut dst, &cfg, Some(path.to_str().unwrap()), Some("relocated")).unwrap();
    let _ = std::fs::remove_file(&path);

    let tasks = db::list_tasks(&dst, None).unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(
        tasks.iter().all(|t| t.project == "relocated"),
        "every imported task must be reassigned to the override project"
    );
    assert!(
        db::list_tasks(&dst, Some("origin")).unwrap().is_empty(),
        "no task should retain its original project after an override import"
    );
}

#[test]
fn import_rejects_a_blob_that_is_not_a_bundle() {
    let mut dst = db::open_in_memory_for_test();
    let cfg = Config::default();
    let err = run(&mut dst, &cfg, Some("this is not a sara task blob"), None)
        .expect_err("a non-bundle blob must be refused");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("blob") || msg.contains("base64") || msg.contains("bundle"),
        "error should explain the bad blob: {err}"
    );
}
