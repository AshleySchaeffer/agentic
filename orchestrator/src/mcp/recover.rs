use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::Value;

/// Return the tool definition for the tools/list response.
pub fn tool_definition() -> Value {
    serde_json::json!({
        "name": "orchestrator:recover",
        "description": "Reconstruct state for a compacted or respawned agent. Returns current task, recent messages, files changed, and active alerts.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "The calling agent's session ID"
                }
            },
            "required": ["session_id"]
        }
    })
}

/// Execute the recover tool. Returns a formatted text string with recovery context.
pub fn call(conn: &Connection, args: Value) -> Result<String> {
    let session_id = args
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required parameter: session_id"))?;

    let agent = crate::store::agents::get_agent_by_session(conn, session_id)?;
    let agent_name = match agent {
        Some(a) => a.name,
        None => return Ok(format!("No agent found with session_id: {session_id}")),
    };

    // Current in-progress task owned by this agent.
    let current_task = crate::store::tasks::list_tasks(conn)?
        .into_iter()
        .find(|t| t.owner.as_deref() == Some(&agent_name) && t.status == "in_progress");

    // Messages sent TO this agent that have not been responded to.
    let mut stmt = conn.prepare(
        "SELECT sender, sent_at FROM messages
         WHERE recipient = ?1 AND response_received = 0
         ORDER BY sent_at",
    )?;
    let unresponded_msgs: Vec<(String, String)> = stmt
        .query_map(params![agent_name], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    // Files changed by this agent.
    let mut stmt = conn.prepare(
        "SELECT DISTINCT file_path FROM file_changes
         WHERE agent_name = ?1
         ORDER BY file_path",
    )?;
    let file_changes: Vec<String> = stmt
        .query_map(params![agent_name], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;

    // Active alerts for this agent.
    let mut stmt = conn.prepare(
        "SELECT severity, description FROM alerts
         WHERE agent_name = ?1 AND resolved_at IS NULL
         ORDER BY created_at",
    )?;
    let active_alerts: Vec<(String, String)> = stmt
        .query_map(params![agent_name], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    // All team members.
    let team_members = crate::store::agents::list_agents(conn)?;

    let mut out = format!("## Recovery Context for {agent_name}\n\n");

    out.push_str("### Current Task\n");
    match current_task {
        Some(task) => {
            out.push_str(&format!("- ID: {}\n", task.task_id));
            out.push_str(&format!(
                "- Subject: {}\n",
                task.subject.as_deref().unwrap_or("(none)")
            ));
            out.push_str(&format!("- Status: {}\n", task.status));
            out.push_str(&format!(
                "- Description: {}\n",
                task.description.as_deref().unwrap_or("(none)")
            ));
        }
        None => out.push_str("- (none)\n"),
    }

    out.push_str("\n### Unanswered Messages\n");
    if unresponded_msgs.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for (sender, sent_at) in &unresponded_msgs {
            out.push_str(&format!("- From {sender} at {sent_at}\n"));
        }
    }

    out.push_str("\n### Files Changed\n");
    if file_changes.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for path in &file_changes {
            out.push_str(&format!("- {path}\n"));
        }
    }

    out.push_str("\n### Active Alerts\n");
    if active_alerts.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for (severity, description) in &active_alerts {
            out.push_str(&format!("- [{severity}] {description}\n"));
        }
    }

    out.push_str("\n### Team Members\n");
    if team_members.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for member in &team_members {
            out.push_str(&format!("- {}: {}\n", member.name, member.status));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        conn
    }

    #[test]
    fn tool_definition_has_correct_schema() {
        let def = tool_definition();
        assert_eq!(def["name"], "orchestrator:recover");
        assert_eq!(def["inputSchema"]["required"][0], "session_id");
        assert!(def["inputSchema"]["properties"]["session_id"].is_object());
    }

    #[test]
    fn call_missing_session_id_errors() {
        let conn = setup_db();
        assert!(call(&conn, serde_json::json!({})).is_err());
    }

    #[test]
    fn call_unknown_session_returns_informative_message() {
        let conn = setup_db();
        let result = call(&conn, serde_json::json!({"session_id": "nonexistent"})).unwrap();
        assert!(result.contains("No agent found"));
    }

    #[test]
    fn call_full_context() {
        let conn = setup_db();

        conn.execute(
            "INSERT INTO agents (name, session_id, agent_type, status, first_seen, last_activity)
             VALUES ('dev-1', 'sess-1', 'dev', 'active', datetime('now'), datetime('now'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (task_id, subject, description, status, owner)
             VALUES ('t-1', 'Do thing', 'Details here', 'in_progress', 'dev-1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (sender, recipient, status, response_received)
             VALUES ('architect', 'dev-1', 'delivered', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO file_changes (agent_name, file_path)
             VALUES ('dev-1', 'src/foo.rs')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO alerts (kind, severity, agent_name, description)
             VALUES ('stalled_task', 'warning', 'dev-1', 'Task stalled')",
            [],
        )
        .unwrap();

        let result = call(&conn, serde_json::json!({"session_id": "sess-1"})).unwrap();

        assert!(result.contains("## Recovery Context for dev-1"));
        assert!(result.contains("Do thing"));
        assert!(result.contains("From architect"));
        assert!(result.contains("src/foo.rs"));
        assert!(result.contains("[warning] Task stalled"));
        assert!(result.contains("dev-1: active"));
    }

    #[test]
    fn call_empty_sections_show_none() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO agents (name, session_id, agent_type, status, first_seen, last_activity)
             VALUES ('solo', 'sess-2', 'dev', 'active', datetime('now'), datetime('now'))",
            [],
        )
        .unwrap();

        let result = call(&conn, serde_json::json!({"session_id": "sess-2"})).unwrap();

        assert!(result.contains("### Current Task\n- (none)"));
        assert!(result.contains("### Unanswered Messages\n- (none)"));
        assert!(result.contains("### Files Changed\n- (none)"));
        assert!(result.contains("### Active Alerts\n- (none)"));
        assert!(result.contains("solo: active"));
    }
}
