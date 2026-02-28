use rusqlite::{params, Connection, Result};

/// Insert a notification for a target agent. Returns the row ID.
pub fn insert_notification(
    conn: &Connection,
    target_agent: &str,
    priority: i32,
    content: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO notifications (target_agent, priority, content) VALUES (?1, ?2, ?3)",
        params![target_agent, priority, content],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Return all undelivered notifications for `target_agent`, ordered by priority ascending.
pub fn drain_notifications(conn: &Connection, target_agent: &str) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, content FROM notifications
         WHERE target_agent = ?1 AND delivered_at IS NULL
         ORDER BY priority ASC",
    )?;
    let items = stmt
        .query_map(params![target_agent], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>>>()?;
    Ok(items)
}

/// Stamp `delivered_at` on a batch of notification IDs.
pub fn mark_notifications_delivered(conn: &Connection, ids: &[i64]) -> Result<()> {
    for id in ids {
        conn.execute(
            "UPDATE notifications
             SET delivered_at = strftime('%Y-%m-%dT%H:%M:%f', 'now')
             WHERE id = ?1",
            params![id],
        )?;
    }
    Ok(())
}
