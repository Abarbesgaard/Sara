use super::*;

#[test]
fn done_auto_memory_is_embedded_so_semantic_recall_can_find_it() {
    let conn = db::open_in_memory_for_test();
    let task = crate::test_support::seed_task(&conn, "rotate the signing keys", "proj");
    let step = db::add_step(
        &conn,
        &task.uuid,
        "Rotate keys in the vault",
        None,
        db::STEP_KIND_STEP,
        "human",
        None,
    )
    .unwrap();
    db::set_step_done(&conn, step, true, Some("rotated both keys"), None).unwrap();

    let v = done_value(&conn, &Config::default(), &task.uuid.to_string(), true).unwrap();

    let label = v["auto_memory"]
        .as_str()
        .expect("done writes an auto-memory");
    let item = db::get_item_by_handle(&conn, label).unwrap();
    assert_eq!(item.status, "provisional");
    assert!(
        db::get_embedding(&conn, &item.uuid.to_string())
            .unwrap()
            .is_some(),
        "the auto-memory must get a vector when it is written"
    );
}
