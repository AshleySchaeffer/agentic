use rusqlite::{params, Connection, Result};

/// Insert an alert record into the alerts table. Returns the row ID.
pub fn insert_alert(
    conn: &Connection,
    kind: &str,
    severity: &str,
    agent_name: Option<&str>,
    task_id: Option<&str>,
    message_id: Option<i64>,
    description: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO alerts (kind, severity, agent_name, task_id, message_id, description)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![kind, severity, agent_name, task_id, message_id, description],
    )?;
    Ok(conn.last_insert_rowid())
}
