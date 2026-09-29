pub mod fzf;
pub mod keymap;
pub mod review_form;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::panic;

pub fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;

    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original(info);
    }));

    Ok(terminal)
}

pub fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

pub fn suspend() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

pub fn resume() -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    Ok(())
}

pub fn centered_rect(pct_x: u16, pct_y: u16, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    use ratatui::layout::{Constraint, Direction, Layout};
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(vertical[1])[1]
}

pub fn render_help_overlay(f: &mut ratatui::Frame, title: &str, bindings: &[(&str, &str)]) {
    use ratatui::{
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, Clear, Paragraph},
    };

    let area = centered_rect(60, 60, f.area());
    f.render_widget(Clear, area);

    let key_w = bindings
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    let mut lines: Vec<Line> = bindings
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(
                    format!("{key:<key_w$}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(*desc),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "(any key closes this)",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} — keybindings "))
        .border_style(Style::default().fg(Color::Yellow));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
