use super::wants_form;

#[test]
fn the_form_opens_only_on_a_terminal_without_yes() {
    let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(wants_form(true, false, &words(&["Fix", "it"])));
    assert!(!wants_form(false, false, &words(&["Fix", "it"])));
    assert!(!wants_form(true, true, &words(&["Fix", "it"])));
    assert!(!wants_form(true, false, &words(&["Fix", "it", "--yes"])));
    assert!(!wants_form(true, false, &words(&["-y", "Fix", "it"])));
}
