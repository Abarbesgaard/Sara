use super::*;

#[test]
fn help_bindings_cover_shared_vocab_and_domain_letters() {
    let bindings = help_bindings();
    let labels: Vec<&str> = bindings.iter().map(|(k, _)| *k).collect();
    for shared in [
        "j/k, ↓/↑",
        "gg / G",
        "K/J, Shift+↓/↑",
        "Space",
        "Enter",
        "Ctrl+S",
        "q / Esc",
        "?",
    ] {
        assert!(labels.contains(&shared), "missing shared binding {shared}");
    }
    for domain in ["e", "a", "c", "r", "x", "o"] {
        assert!(labels.contains(&domain), "missing domain binding {domain}");
    }
}
