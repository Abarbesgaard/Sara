use crate::harness::Sara;

// Contract: undo/A1
#[test]
fn undo_reverts_the_last_command() {
    let s = Sara::new();
    s.run(&["add", "-y", "Keep me"]);
    s.run(&["add", "-y", "Undo me"]);

    s.run(&["undo"]);

    let list = s.run(&["list"]);
    assert!(
        list.contains("Keep me"),
        "earlier task must survive: {list}"
    );
    assert!(
        !list.contains("Undo me"),
        "last add must be reverted: {list}"
    );
}
