use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
    Frame,
};

use crate::{
    store::{agents::Agent, tasks::Task},
    tui::{format_relative_time, truncate, App},
};

pub enum TreeNodeKind {
    Team { name: String },
    Agent(Agent),
    Task(Task),
}

pub struct TreeNode {
    pub kind: TreeNodeKind,
    pub depth: usize,
    pub expanded: bool,
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .tree
        .iter()
        .map(|node| ListItem::new(build_line(node)))
        .collect();
    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    if !app.tree.is_empty() {
        state.select(Some(app.cursor));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn build_line(node: &TreeNode) -> Line<'static> {
    let indent = "  ".repeat(node.depth);
    match &node.kind {
        TreeNodeKind::Team { name } => {
            let icon = if node.expanded { "▼ " } else { "▶ " };
            Line::from(vec![
                Span::styled(indent, Style::default().fg(Color::DarkGray)),
                Span::styled(icon, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    name.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
        }
        TreeNodeKind::Agent(agent) => {
            let icon = if node.expanded { "▼ " } else { "▶ " };
            let (name_style, dot_color) = match agent.status.as_str() {
                "active" => (
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                    Color::Green,
                ),
                "idle" => (Style::default().fg(Color::Gray), Color::Yellow),
                _ => (Style::default().fg(Color::DarkGray), Color::Red),
            };
            let type_str = agent.agent_type.as_deref().unwrap_or("agent").to_string();
            let time_str = format_relative_time(&agent.last_activity);
            Line::from(vec![
                Span::styled(indent, Style::default().fg(Color::DarkGray)),
                Span::styled(icon, Style::default().fg(Color::DarkGray)),
                Span::styled(agent.name.clone(), name_style),
                Span::styled(
                    format!(" ({type_str})"),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(" ●", Style::default().fg(dot_color)),
                Span::styled(format!(" {time_str}"), Style::default().fg(Color::DarkGray)),
            ])
        }
        TreeNodeKind::Task(task) => {
            let (icon, icon_color) = match task.status.as_str() {
                "completed" => ("✓", Color::Green),
                "in_progress" => ("●", Color::Yellow),
                _ => ("○", Color::DarkGray),
            };
            let subject = truncate(task.subject.as_deref().unwrap_or("(untitled)"), 50);
            let subject_style = match task.status.as_str() {
                "completed" => Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
                "in_progress" => Style::default().fg(Color::White),
                _ => Style::default().fg(Color::Gray),
            };
            Line::from(vec![
                Span::styled(indent, Style::default().fg(Color::DarkGray)),
                Span::styled("\u{2514}\u{2500} ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
                Span::styled(subject, subject_style),
            ])
        }
    }
}
