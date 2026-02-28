CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    session_id TEXT NOT NULL,
    agent_name TEXT,
    tool_name TEXT,
    payload TEXT,  -- JSON
    timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%f', 'now'))
);

CREATE TABLE IF NOT EXISTS agents (
    name TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    agent_type TEXT,
    status TEXT NOT NULL DEFAULT 'active',  -- active, idle, dead
    spawner_session_id TEXT,
    first_seen TEXT NOT NULL,
    last_activity TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
    task_id TEXT PRIMARY KEY,
    subject TEXT,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, in_progress, completed
    owner TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%f', 'now'))
);

CREATE TABLE IF NOT EXISTS task_deps (
    task_id TEXT NOT NULL,
    blocked_by TEXT NOT NULL,
    resolved INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (task_id, blocked_by)
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sender TEXT NOT NULL,
    recipient TEXT NOT NULL,
    content_hash TEXT,
    content_file TEXT,  -- path if offloaded
    status TEXT NOT NULL DEFAULT 'sent',  -- sent, delivered, blocked
    response_received INTEGER NOT NULL DEFAULT 0,
    sent_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%f', 'now')),
    delivered_at TEXT
);

CREATE TABLE IF NOT EXISTS alerts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,  -- dead_agent, stalled_task, undelivered_msg, unanswered_msg, unblocked_pending
    severity TEXT NOT NULL DEFAULT 'warning',  -- info, warning, critical
    agent_name TEXT,
    task_id TEXT,
    message_id INTEGER,
    description TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%f', 'now')),
    resolved_at TEXT
);

CREATE TABLE IF NOT EXISTS notifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_agent TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 1,  -- 0=unblock, 1=dead_agent, 2=unanswered
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%f', 'now')),
    delivered_at TEXT
);

CREATE TABLE IF NOT EXISTS file_changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_name TEXT NOT NULL,
    file_path TEXT NOT NULL,
    event_id INTEGER REFERENCES events(id),
    timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%f', 'now'))
);
