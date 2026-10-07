use super::*;
use crate::test_support::render_to_string;
use ratatui::layout::Rect;

#[test]
fn theme_chrome_reserves_header_and_footer_rows() {
    let c = chrome(Rect::new(0, 0, 40, 10));
    assert_eq!((c.header.y, c.header.height), (0, 1));
    assert_eq!((c.body.y, c.body.height), (1, 8));
    assert_eq!((c.footer.y, c.footer.height), (9, 1));
}

#[test]
fn theme_header_and_footer_render_title_status_and_hints() {
    let theme = Theme::new(false);
    let out = render_to_string(40, 3, |f| {
        let c = chrome(f.area());
        render_header(f, c.header, &theme, "follow", &[Span::raw("LIVE")]);
        render_footer(f, c.footer, &theme, &[("q", "quit"), ("p", "project")]);
    });
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with(" sara  ▸ follow"), "{out}");
    assert!(lines[0].ends_with("LIVE"), "{out}");
    assert_eq!(lines[2], "q quit  p project");
}
