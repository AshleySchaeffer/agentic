use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::{
    store::{agents::Agent, messages::get_messages_for_agent, tasks::Task},
    tui::views::tree::TreeNodeKind,
    tui::{format_relative_time, truncate, App},
};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let selected = app.tree.get(app.cursor);
    let lines = match selected.map(|n| &n.kind) {
        Some(TreeNodeKind::Team { .. }) => build_team_detail(app),
        Some(TreeNodeKind::Agent(agent)) => build_agent_detail(app, agent),
        Some(TreeNodeKind::Task(task)) => build_task_detail(app, task),
        None => vec![Line::from(Span::styled(
            "No selection",
            Style::default().fg(Color::DarkGray),
        ))],
    };

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Inspector "))
        .wrap(Wrap { trim: false })
        .scroll((app.inspector_scroll, 0));
    f.render_widget(paragraph, area);
}

fn header(title: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        title.into(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

fn kv(key: impl Into<String>, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{}: ", key.into()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(value.into(), Style::default().fg(Color::White)),
    ])
}

fn blank() -> Line<'static> {
    Line::from("")
}

fn build_team_detail(app: &App) -> Vec<Line<'static>> {
    let active = app.agents.iter().filter(|a| a.status == "active").count();
    let idle = app.agents.iter().filter(|a| a.status == "idle").count();
    let dead = app.agents.iter().filter(|a| a.status == "dead").count();

    let pending = app.tasks.iter().filter(|t| t.status == "pending").count();
    let in_progress = app
        .tasks
        .iter()
        .filter(|t| t.status == "in_progress")
        .count();
    let completed = app.tasks.iter().filter(|t| t.status == "completed").count();

    vec![
        header("Team Overview"),
        blank(),
        header("Agents"),
        kv("  active", active.to_string()),
        kv("  idle", idle.to_string()),
        kv("  dead", dead.to_string()),
        blank(),
        header("Tasks"),
        kv("  pending", pending.to_string()),
        kv("  in_progress", in_progress.to_string()),
        kv("  completed", completed.to_string()),
        blank(),
        header("Activity"),
        kv("  active alerts", app.alerts.len().to_string()),
        kv("  total events", app.event_count.to_string()),
    ]
}

fn build_agent_detail(app: &App, agent: &Agent) -> Vec<Line<'static>> {
    let mut lines = vec![
        header("Agent"),
        blank(),
        kv("name", agent.name.clone()),
        kv(
            "type",
            agent.agent_type.as_deref().unwrap_or("agent").to_string(),
        ),
        kv("status", agent.status.clone()),
        Line::from(vec![
            Span::styled("session: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate(&agent.session_id, 24),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];

    if let Some(spawner_sid) = &agent.spawner_session_id {
        let spawner_name = app
            .agents
            .iter()
            .find(|a| &a.session_id == spawner_sid)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| truncate(spawner_sid, 24));
        lines.push(kv("spawner", spawner_name));
    }

    lines.push(kv("first seen", format_relative_time(&agent.first_seen)));
    lines.push(kv(
        "last activity",
        format_relative_time(&agent.last_activity),
    ));

    // Recent messages
    lines.push(blank());
    lines.push(header("Messages (last 10)"));
    let messages = get_messages_for_agent(&app.conn, &agent.name).unwrap_or_default();
    if messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for msg in messages.iter().take(10) {
            let (arrow, peer, arrow_color) = if msg.sender == agent.name {
                ("→", msg.recipient.clone(), Color::Green)
            } else {
                ("←", msg.sender.clone(), Color::Cyan)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {arrow} "), Style::default().fg(arrow_color)),
                Span::styled(truncate(&peer, 20), Style::default().fg(Color::White)),
                Span::styled(
                    format!(" [{}]", msg.status),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(" {}", format_relative_time(&msg.sent_at)),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    }

    // Recent events
    lines.push(blank());
    lines.push(header("Events (last 20)"));
    let events = query_agent_events(&app.conn, &agent.name);
    if events.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (event_type, tool_name, timestamp) in &events {
            let label = match tool_name {
                Some(tool) => format!("{event_type}: {tool}"),
                None => event_type.clone(),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", format_relative_time(timestamp)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(truncate(&label, 40), Style::default().fg(Color::White)),
            ]));
        }
    }

    // Active alerts for this agent
    let agent_alerts: Vec<_> = app
        .alerts
        .iter()
        .filter(|a| a.agent_name.as_deref() == Some(agent.name.as_str()))
        .collect();
    if !agent_alerts.is_empty() {
        lines.push(blank());
        lines.push(header("Alerts"));
        for alert in agent_alerts.iter().take(5) {
            let sev_color = match alert.severity.as_str() {
                "critical" => Color::Red,
                "warning" => Color::Yellow,
                _ => Color::DarkGray,
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  [{}] ", alert.severity),
                    Style::default().fg(sev_color),
                ),
                Span::styled(
                    truncate(&alert.description, 40),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
    }

    // Live output from agent's tmux pane
    if agent.status != "dead" {
        lines.push(blank());
        lines.push(header("Live Output"));
        if app.pane_map.contains_key(&agent.name) {
            if let Some((pane_agent, output)) = &app.pane_output {
                if pane_agent == &agent.name {
                    for line_text in output.lines() {
                        lines.push(Line::from(Span::styled(
                            line_text.to_string(),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                } else {
                    lines.push(Line::from(Span::styled(
                        "  (loading\u{2026})",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            } else {
                lines.push(Line::from(Span::styled(
                    "  (loading\u{2026})",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  (no tmux pane)",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    lines
}

fn build_task_detail(app: &App, task: &Task) -> Vec<Line<'static>> {
    let mut lines = vec![
        header("Task"),
        blank(),
        kv("id", task.task_id.clone()),
        kv("status", task.status.clone()),
        kv(
            "owner",
            task.owner.as_deref().unwrap_or("(unassigned)").to_string(),
        ),
        kv("updated", format_relative_time(&task.updated_at)),
    ];

    if let Some(subject) = &task.subject {
        lines.push(blank());
        lines.push(header("Subject"));
        lines.push(Line::from(Span::styled(
            subject.clone(),
            Style::default().fg(Color::White),
        )));
    }

    if let Some(desc) = &task.description {
        lines.push(blank());
        lines.push(header("Description"));
        for line_text in desc.lines() {
            lines.push(Line::from(Span::styled(
                line_text.to_string(),
                Style::default().fg(Color::Gray),
            )));
        }
    }

    // Blocked by
    if let Some(blockers) = app.task_blockers.get(&task.task_id) {
        if !blockers.is_empty() {
            lines.push(blank());
            lines.push(header("Blocked By"));
            for blocker_id in blockers {
                let status = app
                    .tasks
                    .iter()
                    .find(|t| &t.task_id == blocker_id)
                    .map(|t| t.status.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let status_color = match status.as_str() {
                    "completed" => Color::Green,
                    "in_progress" => Color::Yellow,
                    _ => Color::DarkGray,
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  #{blocker_id} "),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(format!("[{status}]"), Style::default().fg(status_color)),
                ]));
            }
        }
    }

    lines
}

fn query_agent_events(
    conn: &rusqlite::Connection,
    agent_name: &str,
) -> Vec<(String, Option<String>, String)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT event_type, tool_name, timestamp FROM events \
         WHERE agent_name = ?1 ORDER BY timestamp DESC LIMIT 20",
    ) else {
        return vec![];
    };
    let Ok(rows) = stmt.query_map(rusqlite::params![agent_name], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) else {
        return vec![];
    };
    rows.flatten().collect()
}
