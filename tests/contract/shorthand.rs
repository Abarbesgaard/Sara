use crate::harness::Sara;

#[test]
fn bare_id_and_id_first_forms_show_the_task_like_info() {
    let s = Sara::new();
    s.run(&["add", "-y", "First task"]);
    let info = s.run(&["info", "1"]);

    assert_eq!(s.run(&["1"]), info, "`sara 1` should mean `sara info 1`");
    assert_eq!(
        s.run(&["1", "info"]),
        info,
        "`sara 1 info` should mean `sara info 1`"
    );
}
