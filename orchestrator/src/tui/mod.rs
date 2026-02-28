use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    Frame, Terminal,
};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    config::Config,
    store::{agents::Agent, messages::Message, tasks::Task},
};
use views::tree::{TreeNode, TreeNodeKind};

pub mod views;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AlertRow {
    pub id: i64,
    pub kind: String,
    pub severity: String,
    pub agent_name: Option<String>,
    pub task_id: Option<String>,
    pub message_id: Option<i64>,
    pub description: String,
    pub created_at: String,
}

pub struct App {
    pub agents: Vec<Agent>,
    pub tasks: Vec<Task>,
    pub messages: Vec<Message>,
    pub alerts: Vec<AlertRow>,
    pub task_blockers: HashMap<String, Vec<String>>,

    pub tree: Vec<TreeNode>,
    pub collapsed: HashSet<String>,
    pub cursor: usize,
    pub show_dead_agents: bool,
    pub show_finished_tasks: bool,
    pub show_inspector: bool,
    pub inspector_scroll: u16,

    pub pane_map: HashMap<String, String>,
    pub pane_output: Option<(String, String)>,

    pub should_quit: bool,
    pub(crate) conn: Connection,
    last_refresh: Instant,
    pub event_count: i64,
    pub last_error: Option<String>,
}

impl App {
    fn new(db_path: &std::path::Path) -> anyhow::Result<Self> {
        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let mut app = App {
            agents: Vec::new(),
            tasks: Vec::new(),
            messages: Vec::new(),
            alerts: Vec::new(),
            task_blockers: HashMap::new(),
            tree: Vec::new(),
            collapsed: HashSet::new(),
            cursor: 0,
            show_dead_agents: false,
            show_finished_tasks: false,
            show_inspector: false,
            inspector_scroll: 0,
            pane_map: HashMap::new(),
            pane_output: None,
            should_quit: false,
            conn,
            last_refresh: Instant::now(),
            event_count: 0,
            last_error: None,
        };
        let _ = app.refresh();
        Ok(app)
    }

    fn refresh(&mut self) -> anyhow::Result<()> {
        self.agents = crate::store::agents::list_agents(&self.conn).unwrap_or_default();
        self.tasks = crate::store::tasks::list_tasks(&self.conn).unwrap_or_default();
        self.messages = query_recent_messages(&self.conn).unwrap_or_default();
        self.alerts = query_active_alerts(&self.conn).unwrap_or_default();
        self.task_blockers = query_task_blockers(&self.conn).unwrap_or_default();
        self.event_count = query_event_count(&self.conn).unwrap_or(0);
        self.last_error = query_last_error(&self.conn).unwrap_or(None);
        self.rebuild_tree();
        self.pane_map = discover_agent_panes();
        self.capture_selected_pane();
        self.last_refresh = Instant::now();
        Ok(())
    }

    fn capture_selected_pane(&mut self) {
        let agent_name = match self.tree.get(self.cursor).map(|n| &n.kind) {
            Some(TreeNodeKind::Agent(a)) if a.status != "dead" => Some(a.name.clone()),
            _ => None,
        };
        let Some(agent_name) = agent_name else {
            self.pane_output = None;
            return;
        };
        let pane_id = match self.pane_map.get(&agent_name) {
            Some(id) => id.clone(),
            None => {
                self.pane_output = None;
                return;
            }
        };
        let Ok(output) = std::process::Command::new("tmux")
            .args(["capture-pane", "-p", "-t", &pane_id, "-S", "-30"])
            .output()
        else {
            self.pane_output = None;
            return;
        };
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            self.pane_output = Some((agent_name, text));
        } else {
            self.pane_output = None;
        }
    }

    fn rebuild_tree(&mut self) {
        self.tree = build_tree(
            &self.agents,
            &self.tasks,
            &self.collapsed,
            self.show_dead_agents,
            self.show_finished_tasks,
        );
        if self.tree.is_empty() {
            self.cursor = 0;
        } else {
            self.cursor = self.cursor.min(self.tree.len() - 1);
        }
    }

    fn cursor_down(&mut self) {
        if self.cursor + 1 < self.tree.len() {
            self.cursor += 1;
            self.inspector_scroll = 0;
        }
    }

    fn cursor_up(&mut self) {
        let prev = self.cursor;
        self.cursor = self.cursor.saturating_sub(1);
        if self.cursor != prev {
            self.inspector_scroll = 0;
        }
    }

    fn expand_cursor(&mut self) {
        let key = self
            .tree
            .get(self.cursor)
            .filter(|n| !n.expanded)
            .and_then(|n| match &n.kind {
                TreeNodeKind::Team { name } => Some(name.clone()),
                TreeNodeKind::Agent(a) => Some(a.session_id.clone()),
                TreeNodeKind::Task(_) => None,
            });
        if let Some(key) = key {
            self.collapsed.remove(&key);
            self.rebuild_tree();
        }
    }

    fn collapse_cursor(&mut self) {
        let info = self.tree.get(self.cursor).map(|n| {
            let key = match &n.kind {
                TreeNodeKind::Team { name } => Some(name.clone()),
                TreeNodeKind::Agent(a) => Some(a.session_id.clone()),
                TreeNodeKind::Task(_) => None,
            };
            (n.expanded, n.depth, key)
        });
        let Some((expanded, depth, key)) = info else {
            return;
        };
        if expanded {
            if let Some(key) = key {
                self.collapsed.insert(key);
                self.rebuild_tree();
            }
        } else if depth > 0 {
            let target_depth = depth - 1;
            let cursor = self.cursor;
            if let Some(parent_idx) = self.tree[..cursor]
                .iter()
                .rposition(|n| n.depth == target_depth)
            {
                self.cursor = parent_idx;
            }
        }
    }
}

fn discover_agent_panes() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return map,
    };
    let teams_dir = format!("{home}/.claude/teams");
    let entries = match fs::read_dir(&teams_dir) {
        Ok(e) => e,
        Err(_) => return map,
    };
    for entry in entries.flatten() {
        let config_path = entry.path().join("config.json");
        let Ok(data) = fs::read_to_string(&config_path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) else {
            continue;
        };
        let Some(members) = json.get("members").and_then(|m| m.as_array()) else {
            continue;
        };
        for member in members {
            let Some(name) = member.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            let pane_id = member
                .get("tmuxPaneId")
                .and_then(|p| p.as_str())
                .unwrap_or("");
            if !pane_id.is_empty() {
                map.insert(name.to_string(), pane_id.to_string());
            }
        }
    }
    map
}

fn build_tree(
    agents: &[Agent],
    tasks: &[Task],
    collapsed: &HashSet<String>,
    show_dead: bool,
    show_finished: bool,
) -> Vec<TreeNode> {
    let session_to_idx: HashMap<String, usize> = agents
        .iter()
        .enumerate()
        .map(|(i, a)| (a.session_id.clone(), i))
        .collect();

    let mut children_map: HashMap<String, Vec<usize>> = HashMap::new();
    let mut top_level: Vec<usize> = Vec::new();
    for (i, agent) in agents.iter().enumerate() {
        match &agent.spawner_session_id {
            Some(spawner_id) if session_to_idx.contains_key(spawner_id) => {
                children_map.entry(spawner_id.clone()).or_default().push(i);
            }
            _ => top_level.push(i),
        }
    }

    let agent_names: HashSet<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    let mut owned_tasks: HashMap<String, Vec<usize>> = HashMap::new();
    let mut unowned_tasks: Vec<usize> = Vec::new();
    for (i, task) in tasks.iter().enumerate() {
        match &task.owner {
            Some(owner) if agent_names.contains(owner.as_str()) => {
                owned_tasks.entry(owner.clone()).or_default().push(i);
            }
            _ => unowned_tasks.push(i),
        }
    }

    let mut nodes: Vec<TreeNode> = Vec::new();
    let root_expanded = !collapsed.contains("team");
    nodes.push(TreeNode {
        kind: TreeNodeKind::Team {
            name: "team".to_string(),
        },
        depth: 0,
        expanded: root_expanded,
    });

    if root_expanded {
        let ctx = SubtreeCtx {
            agents,
            tasks,
            children_map: &children_map,
            owned_tasks: &owned_tasks,
            collapsed,
            show_dead,
            show_finished,
        };
        top_level.sort_by_key(|&i| agent_sort_key(&ctx.agents[i].status));
        for agent_idx in top_level {
            push_agent_subtree(&mut nodes, agent_idx, &ctx, 1);
        }
        unowned_tasks.sort_by(|&a, &b| {
            task_id_sort_key(&tasks[a].task_id)
                .cmp(&task_id_sort_key(&tasks[b].task_id))
                .then_with(|| tasks[a].task_id.cmp(&tasks[b].task_id))
        });
        for task_idx in unowned_tasks {
            let task = &tasks[task_idx];
            if !show_finished && task.status == "completed" {
                continue;
            }
            nodes.push(TreeNode {
                kind: TreeNodeKind::Task(task.clone()),
                depth: 1,
                expanded: false,
            });
        }
    }

    nodes
}

struct SubtreeCtx<'a> {
    agents: &'a [Agent],
    tasks: &'a [Task],
    children_map: &'a HashMap<String, Vec<usize>>,
    owned_tasks: &'a HashMap<String, Vec<usize>>,
    collapsed: &'a HashSet<String>,
    show_dead: bool,
    show_finished: bool,
}

fn push_agent_subtree(nodes: &mut Vec<TreeNode>, agent_idx: usize, ctx: &SubtreeCtx, depth: usize) {
    let agent = &ctx.agents[agent_idx];
    if !ctx.show_dead && agent.status == "dead" {
        return;
    }
    let agent_expanded = !ctx.collapsed.contains(&agent.session_id);
    nodes.push(TreeNode {
        kind: TreeNodeKind::Agent(agent.clone()),
        depth,
        expanded: agent_expanded,
    });
    if agent_expanded {
        if let Some(task_indices) = ctx.owned_tasks.get(&agent.name) {
            let mut sorted_tasks = task_indices.clone();
            sorted_tasks.sort_by(|&a, &b| {
                task_id_sort_key(&ctx.tasks[a].task_id)
                    .cmp(&task_id_sort_key(&ctx.tasks[b].task_id))
                    .then_with(|| ctx.tasks[a].task_id.cmp(&ctx.tasks[b].task_id))
            });
            for &task_idx in &sorted_tasks {
                let task = &ctx.tasks[task_idx];
                if !ctx.show_finished && task.status == "completed" {
                    continue;
                }
                nodes.push(TreeNode {
                    kind: TreeNodeKind::Task(task.clone()),
                    depth: depth + 1,
                    expanded: false,
                });
            }
        }
        if let Some(child_indices) = ctx.children_map.get(&agent.session_id) {
            let mut sorted_children = child_indices.clone();
            sorted_children.sort_by_key(|&i| agent_sort_key(&ctx.agents[i].status));
            for child_idx in sorted_children {
                push_agent_subtree(nodes, child_idx, ctx, depth + 1);
            }
        }
    }
}

fn agent_sort_key(status: &str) -> u8 {
    match status {
        "active" => 0,
        "idle" => 1,
        _ => 2,
    }
}

/// Returns a numeric sort key for a task_id string: parses as u64 when possible
/// so that "2" sorts before "10", falling back to u64::MAX for non-numeric ids.
fn task_id_sort_key(task_id: &str) -> u64 {
    task_id.parse().unwrap_or(u64::MAX)
}

fn query_recent_messages(conn: &Connection) -> rusqlite::Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, sender, recipient, content_hash, content_file, status,
                response_received, sent_at, delivered_at
         FROM messages ORDER BY sent_at DESC LIMIT 100",
    )?;
    let mut msgs = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        msgs.push(Message {
            id: row.get(0)?,
            sender: row.get(1)?,
            recipient: row.get(2)?,
            content_hash: row.get(3)?,
            content_file: row.get(4)?,
            status: row.get(5)?,
            response_received: row.get::<_, i32>(6)? != 0,
            sent_at: row.get(7)?,
            delivered_at: row.get(8)?,
        });
    }
    Ok(msgs)
}

fn query_active_alerts(conn: &Connection) -> rusqlite::Result<Vec<AlertRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, severity, agent_name, task_id, message_id, description, created_at
         FROM alerts WHERE resolved_at IS NULL ORDER BY created_at DESC",
    )?;
    let mut alerts = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        alerts.push(AlertRow {
            id: row.get(0)?,
            kind: row.get(1)?,
            severity: row.get(2)?,
            agent_name: row.get(3)?,
            task_id: row.get(4)?,
            message_id: row.get(5)?,
            description: row.get(6)?,
            created_at: row.get(7)?,
        });
    }
    Ok(alerts)
}

fn query_task_blockers(conn: &Connection) -> rusqlite::Result<HashMap<String, Vec<String>>> {
    let mut stmt = conn.prepare("SELECT task_id, blocked_by FROM task_deps WHERE resolved = 0")?;
    let mut blockers: HashMap<String, Vec<String>> = HashMap::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let task_id: String = row.get(0)?;
        let blocked_by: String = row.get(1)?;
        blockers.entry(task_id).or_default().push(blocked_by);
    }
    Ok(blockers)
}

fn query_event_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
}

fn query_last_error(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT description FROM alerts WHERE resolved_at IS NULL
         ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'warning' THEN 1 ELSE 2 END,
         created_at DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other),
    })
}

/// Truncate a string to `max_len` characters, appending "…" if truncated.
pub(crate) fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Convert an ISO 8601 timestamp (SQLite's `%Y-%m-%dT%H:%M:%f` format) to a
/// human-readable relative time string such as "2m ago".
/// Returns "?" for empty or malformed input.
pub fn format_relative_time(ts: &str) -> String {
    let Some(epoch) = parse_iso_to_epoch(ts) else {
        return "?".to_string();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let diff = now - epoch;
    if diff <= 0 {
        return "just now".to_string();
    }
    if diff < 60 {
        return format!("{diff}s ago");
    }
    if diff < 3600 {
        return format!("{}m ago", diff / 60);
    }
    if diff < 86400 {
        return format!("{}h ago", diff / 3600);
    }
    format!("{}d ago", diff / 86400)
}

fn parse_iso_to_epoch(ts: &str) -> Option<i64> {
    if ts.is_empty() {
        return None;
    }
    let (date_part, time_raw) = ts.split_once('T')?;
    let time_part = time_raw.split('.').next()?;
    let mut date_iter = date_part.split('-');
    let year: i64 = date_iter.next()?.parse().ok()?;
    let month: i64 = date_iter.next()?.parse().ok()?;
    let day: i64 = date_iter.next()?.parse().ok()?;
    let mut time_iter = time_part.split(':');
    let hour: i64 = time_iter.next()?.parse().ok()?;
    let min: i64 = time_iter.next()?.parse().ok()?;
    let sec: i64 = time_iter.next().unwrap_or("0").parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let years_days: i64 = (1970..year)
        .map(|y| if is_leap_year(y) { 366 } else { 365 })
        .sum();
    let month_days: i64 = (1..month).map(|m| days_in_month(m, year)).sum();
    let total_days = years_days + month_days + day - 1;
    Some(total_days * 86400 + hour * 3600 + min * 60 + sec)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(m: i64, year: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

pub fn run(config: &Config) -> anyhow::Result<()> {
    let mut app = App::new(&config.db_path)?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    let refresh_interval = Duration::from_secs(2);
    loop {
        terminal.draw(|f| draw(f, app))?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => handle_key(app, key.code),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if app.should_quit {
            return Ok(());
        }

        if app.last_refresh.elapsed() >= refresh_interval {
            let _ = app.refresh();
        }
    }
}

fn draw(f: &mut Frame, app: &App) {
    let size = f.size();
    if app.show_inspector {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(size);
        views::tree::render(f, chunks[0], app);
        views::inspector::render(f, chunks[1], app);
    } else {
        views::tree::render(f, size, app);
    }
}

fn handle_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('i') => {
            app.show_inspector = !app.show_inspector;
            if app.show_inspector {
                app.inspector_scroll = 0;
            }
        }
        KeyCode::Char('s') => {
            app.show_dead_agents = !app.show_dead_agents;
            app.rebuild_tree();
        }
        KeyCode::Char('S') => {
            let pane_id = {
                let agent_name = match app.tree.get(app.cursor).map(|n| &n.kind) {
                    Some(TreeNodeKind::Agent(a)) => Some(a.name.clone()),
                    _ => None,
                };
                agent_name.and_then(|name| app.pane_map.get(&name).cloned())
            };
            if let Some(pane_id) = pane_id {
                let _ = std::process::Command::new("tmux")
                    .args(["select-pane", "-t", &pane_id])
                    .status();
            }
        }
        KeyCode::Char('f') => {
            app.show_finished_tasks = !app.show_finished_tasks;
            app.rebuild_tree();
        }
        KeyCode::Down | KeyCode::Char('j') => app.cursor_down(),
        KeyCode::Up | KeyCode::Char('k') => app.cursor_up(),
        KeyCode::Enter | KeyCode::Right => app.expand_cursor(),
        KeyCode::Left => app.collapse_cursor(),
        KeyCode::PageDown => {
            if app.show_inspector {
                app.inspector_scroll = app.inspector_scroll.saturating_add(5);
            }
        }
        KeyCode::PageUp => {
            if app.show_inspector {
                app.inspector_scroll = app.inspector_scroll.saturating_sub(5);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::tests::make_store;

    #[test]
    fn format_relative_time_empty() {
        assert_eq!(format_relative_time(""), "?");
    }

    #[test]
    fn format_relative_time_malformed() {
        assert_eq!(format_relative_time("not-a-timestamp"), "?");
        assert_eq!(format_relative_time("2024-01-"), "?");
    }

    #[test]
    fn format_relative_time_far_past() {
        let result = format_relative_time("2020-01-01T00:00:00.000");
        assert!(result.ends_with("d ago"), "expected 'd ago', got: {result}");
    }

    #[test]
    fn parse_iso_epoch_unix_origin() {
        assert_eq!(parse_iso_to_epoch("1970-01-01T00:00:00.000"), Some(0));
    }

    #[test]
    fn parse_iso_epoch_one_minute() {
        assert_eq!(parse_iso_to_epoch("1970-01-01T00:01:00.000"), Some(60));
    }

    #[test]
    fn parse_iso_epoch_invalid_month() {
        assert_eq!(parse_iso_to_epoch("2024-13-01T00:00:00.000"), None);
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let result = truncate("hello world", 6);
        assert!(result.ends_with('\u{2026}'));
        assert!(result.chars().count() <= 6);
    }

    #[test]
    fn db_queries_return_correct_types() {
        let store = make_store();
        let agents = crate::store::agents::list_agents(store.conn()).expect("agents");
        let tasks = crate::store::tasks::list_tasks(store.conn()).expect("tasks");
        let messages = query_recent_messages(store.conn()).expect("messages");
        let alerts = query_active_alerts(store.conn()).expect("alerts");
        let blockers = query_task_blockers(store.conn()).expect("blockers");
        assert!(agents.is_empty());
        assert!(tasks.is_empty());
        assert!(messages.is_empty());
        assert!(alerts.is_empty());
        assert!(blockers.is_empty());
    }
}
