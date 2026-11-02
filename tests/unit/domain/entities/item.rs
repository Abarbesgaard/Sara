use super::*;

#[test]
fn handle_prefixes_links_with_l_and_everything_else_with_n() {
    let mut link = Item::new_link("https://x.dev".into(), "x".into(), String::new());
    link.display_id = Some(7);
    assert_eq!(link.handle(), "l7");

    let mut memory = Item::new_memory("m".into(), "body".into(), None);
    memory.display_id = Some(12);
    assert_eq!(memory.handle(), "n12");

    assert_eq!(Item::new_note("n".into(), String::new()).handle(), "n0");
}
