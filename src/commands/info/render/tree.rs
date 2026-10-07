use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::commands::shared::truncate;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;
use crate::infrastructure::tui::theme::{Ink, ink};

use crate::commands::info::handler::{TREE_COMPACT_CHILDREN, TREE_COMPACT_DEPTH};
use crate::commands::info::types::{Detail, EditState, GraphNode};

const TREE_PANEL_WIDTH: usize = 40;

pub(super) fn task_tree_lines(d: &Detail, st: &EditState) -> Vec<Line<'static>> {
    let (max_depth, max_children) = if st.tree_expanded {
        (usize::MAX, usize::MAX)
    } else {
        (TREE_COMPACT_DEPTH, TREE_COMPACT_CHILDREN)
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(tree_section_header(&format!(
        "← blocked by ({})",
        d.tree.blockers.len()
    )));
    push_tree_side_lines(
        &mut lines,
        &d.tree.blockers,
        d.tree.blockers_hidden,
        max_depth,
        max_children,
    );

    let rule = "─".repeat(TREE_PANEL_WIDTH);
    lines.push(Line::from(Span::styled(
        rule.clone(),
        Style::default().fg(ink(Ink::Muted)),
    )));
    lines.push(current_task_tree_line(&d.task));
    lines.push(Line::from(Span::styled(
        rule,
        Style::default().fg(ink(Ink::Muted)),
    )));

    lines.push(tree_section_header(&format!(
        "blocks → ({})",
        d.tree.dependents.len()
    )));
    push_tree_side_lines(
        &mut lines,
        &d.tree.dependents,
        d.tree.dependents_hidden,
        max_depth,
        max_children,
    );
    lines
}

fn push_tree_side_lines(
    lines: &mut Vec<Line<'static>>,
    nodes: &[GraphNode],
    hidden: usize,
    max_depth: usize,
    max_children: usize,
) {
    if nodes.is_empty() && hidden == 0 {
        lines.push(Line::from(Span::styled(
            "   — none —",
            Style::default().fg(ink(Ink::Muted)),
        )));
        return;
    }
    push_tree_node_lines(lines, nodes, hidden, "  ", 1, max_depth, max_children);
}

fn tree_section_header(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {text}"),
        Style::default()
            .fg(ink(Ink::Soft))
            .add_modifier(Modifier::BOLD),
    ))
}

fn current_task_tree_line(task: &Task) -> Line<'static> {
    let id_str = task
        .id
        .map(|n| format!("{n:>3}"))
        .unwrap_or_else(|| "  -".to_string());
    let style = Style::default()
        .fg(ink(Ink::Text))
        .bg(ink(Ink::Select))
        .add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(" ▶ ", style),
        Span::styled(format!("{id_str} "), style),
        Span::styled(truncate(&task.description, 30), style),
    ])
}

fn push_tree_node_lines(
    lines: &mut Vec<Line<'static>>,
    nodes: &[GraphNode],
    hidden_here: usize,
    prefix: &str,
    depth: usize,
    max_depth: usize,
    max_children: usize,
) {
    let total = nodes.len();
    let visible = total.min(max_children);
    let overflow = hidden_here + total.saturating_sub(visible);
    for (i, node) in nodes.iter().take(visible).enumerate() {
        let is_last = overflow == 0 && i + 1 == visible;
        let connector = if is_last { "└─" } else { "├─" };
        lines.push(tree_node_line(node, prefix, connector));
        let child_prefix = format!("{prefix}{}", if is_last { "   " } else { "│  " });
        let has_children = !node.children.is_empty() || node.hidden_children > 0;
        if has_children {
            if depth < max_depth {
                push_tree_node_lines(
                    lines,
                    &node.children,
                    node.hidden_children,
                    &child_prefix,
                    depth + 1,
                    max_depth,
                    max_children,
                );
            } else {
                lines.push(Line::from(Span::styled(
                    format!("{child_prefix}└─ … (d to expand)"),
                    Style::default()
                        .fg(ink(Ink::Muted))
                        .add_modifier(Modifier::ITALIC),
                )));
            }
        }
    }
    if overflow > 0 {
        lines.push(Line::from(Span::styled(
            format!("{prefix}└─ +{overflow} more  (d to expand)"),
            Style::default()
                .fg(ink(Ink::Muted))
                .add_modifier(Modifier::ITALIC),
        )));
    }
}

fn tree_node_line(node: &GraphNode, prefix: &str, connector: &str) -> Line<'static> {
    let completed = node.status == crate::infrastructure::model::Status::Completed;
    let glyph = if completed { "✓" } else { "○" };
    let id_str = node.id.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
    let style = if completed {
        Style::default()
            .fg(ink(Ink::Muted))
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::default().fg(ink(Ink::Accent))
    };
    let desc_budget = TREE_PANEL_WIDTH
        .saturating_sub(prefix.chars().count() + connector.chars().count() + id_str.len() + 4);
    let mut spans = vec![
        Span::styled(
            format!("{prefix}{connector}"),
            Style::default().fg(ink(Ink::Muted)),
        ),
        Span::styled(format!("{glyph} "), style),
        Span::styled(format!("{id_str} "), style),
        Span::styled(truncate(&node.description, desc_budget.max(6)), style),
    ];
    if let Some(label) = link_badge_label(node.badge.as_ref()) {
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(ink(Ink::Warn)),
        ));
    }
    Line::from(spans)
}

fn link_badge_label(flags: Option<&db::LinkFlags>) -> Option<&'static str> {
    let f = flags?;
    if f.pr {
        Some("PR")
    } else if f.issue {
        Some("ISS")
    } else if f.any {
        Some("●")
    } else {
        None
    }
}
