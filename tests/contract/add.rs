use crate::harness::Sara;

#[test]
#[impish::contract("add/A1")]
fn an_added_task_is_pending() {
    let s = Sara::new();

    s.run(&["add", "-y", "Water the plants"]);

    let list = s.run(&["list"]);
    assert!(
        list.contains("Water the plants"),
        "added task must be listed: {list}"
    );
    let status: String = s
        .db()
        .query_row(
            "SELECT status FROM tasks WHERE description = 'Water the plants'",
            [],
            |r| r.get(0),
        )
        .expect("the task is stored");
    assert_eq!(status, "pending");
}

#[test]
#[impish::contract("add/A2")]
fn an_empty_description_is_refused() {
    let s = Sara::new();

    for words in [&[][..], &["   "][..], &["+onlytag"][..]] {
        let out = s
            .cmd()
            .args(["add", "-y"])
            .args(words)
            .output()
            .expect("spawn sara");
        assert!(
            !out.status.success(),
            "`sara add -y {}` must fail",
            words.join(" ")
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("description cannot be empty"),
            "stderr names the problem"
        );
    }

    let tasks: i64 = s
        .db()
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .expect("count tasks");
    assert_eq!(tasks, 0, "no task is created");
}

#[test]
#[impish::contract("add/A3")]
fn modifiers_set_the_tasks_fields() {
    let s = Sara::new();

    s.run(&[
        "add",
        "-y",
        "project:garden",
        "+home",
        "Water",
        "the",
        "plants",
        "pri:h",
        "+weekly",
    ]);

    let (description, project, priority, tags): (String, String, String, String) = s
        .db()
        .query_row(
            "SELECT description, project, priority, tags_json FROM tasks",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("the task is stored");
    assert_eq!(description, "Water the plants");
    assert_eq!(project, "garden");
    assert_eq!(priority, "H");
    let mut tags: Vec<String> = serde_json::from_str(&tags).expect("tags are JSON");
    tags.sort();
    assert_eq!(tags, ["home", "weekly"]);
}

#[test]
#[impish::contract("add/A4")]
fn a_matching_open_task_is_named() {
    let s = Sara::new();
    s.run(&["add", "-y", "project:garden", "Water the plants"]);
    s.run(&["add", "-y", "project:garden", "Prune the roses"]);
    s.run(&["done", "2"]);

    let same = s.run(&["add", "-y", "project:garden", "water", "THE", "plants"]);
    assert!(
        same.contains("already matches") && same.contains("task 1"),
        "a matching open task is named: {same}"
    );

    let elsewhere = s.run(&["add", "-y", "project:kitchen", "Water the plants"]);
    let finished = s.run(&["add", "-y", "project:garden", "Prune the roses"]);
    for out in [elsewhere, finished] {
        assert!(!out.contains("already matches"), "no warning: {out}");
    }

    let tasks: i64 = s
        .db()
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .expect("count tasks");
    assert_eq!(tasks, 5, "every add still creates a task");
}

#[test]
#[impish::contract("add/A5")]
fn flags_win_over_inline_modifiers() {
    let s = Sara::new();

    s.run(&[
        "add",
        "-y",
        "--project",
        "kitchen",
        "--priority",
        "l",
        "--tag",
        "weekly",
        "project:garden",
        "pri:h",
        "+home",
        "Water the plants",
    ]);

    let (description, project, priority, tags): (String, String, String, String) = s
        .db()
        .query_row(
            "SELECT description, project, priority, tags_json FROM tasks",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("the task is stored");
    assert_eq!(description, "Water the plants");
    assert_eq!(project, "kitchen");
    assert_eq!(priority, "L");
    let mut tags: Vec<String> = serde_json::from_str(&tags).expect("tags are JSON");
    tags.sort();
    assert_eq!(tags, ["home", "weekly"]);
}

#[test]
#[impish::contract("add/A6")]
fn an_unknown_priority_is_refused() {
    let s = Sara::new();

    for args in [
        &["add", "-y", "pri:zzz", "Thing"][..],
        &["add", "-y", "--priority", "zzz", "Thing"],
    ] {
        let out = s.cmd().args(args).output().expect("spawn sara");
        assert_eq!(out.status.code(), Some(1), "`sara {}`", args.join(" "));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("Unknown priority: ZZZ"), "{stderr}");
    }
    assert!(
        s.run(&["list"]).contains("No pending tasks"),
        "no task is created"
    );

    s.run(&["add", "-y", "pri:m", "Thing"]);
    s.run(&["add", "-y", "--priority", "Low", "Other"]);
}
