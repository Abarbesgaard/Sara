use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Frame, Terminal, backend::TestBackend};

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn render_to_string(w: u16, h: u16, draw: impl FnOnce(&mut Frame)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(draw).unwrap();
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let mut out = String::new();
    for y in 0..area.height {
        let line: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

pub fn render_to_styled_string(w: u16, h: u16, draw: impl FnOnce(&mut Frame)) -> String {
    use ratatui::style::Style;
    const KEYS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(draw).unwrap();
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let mut styles: Vec<Style> = Vec::new();
    let mut text = String::new();
    let mut grid = String::new();
    for y in 0..area.height {
        let line: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
        text.push_str(line.trim_end());
        text.push('\n');
        let mut row = String::new();
        for x in 0..area.width {
            let style = buf[(x, y)].style();
            if style == Style::reset() || style == Style::default() {
                row.push('.');
                continue;
            }
            let i = styles.iter().position(|s| *s == style).unwrap_or_else(|| {
                styles.push(style);
                styles.len() - 1
            });
            row.push(KEYS.chars().nth(i).unwrap_or('?'));
        }
        grid.push_str(row.trim_end_matches('.'));
        grid.push('\n');
    }
    let mut out = text;
    out.push_str("--- styles ---\n");
    out.push_str(&grid);
    out.push_str("--- legend ---\n");
    for (i, s) in styles.iter().enumerate() {
        let key = KEYS.chars().nth(i).unwrap_or('?');
        out.push_str(&format!(
            "{key} fg={:?} bg={:?} mod={:?}\n",
            s.fg, s.bg, s.add_modifier
        ));
    }
    out
}
