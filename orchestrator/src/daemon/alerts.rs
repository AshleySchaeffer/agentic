#![allow(dead_code)]

use anyhow::Result;
use rusqlite::{params, Connection};

/// Alert record as stored in the database.
pub struct Alert {
    pub id: i64,
    pub kind: String,
    pub severity: String,
    pub agent_name: Option<String>,
    pub task_id: Option<String>,
    pub message_id: Option<i64>,
    pub description: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

/// Return all alerts that have not yet been resolved.
pub fn get_active_alerts(conn: &Connection) -> Result<Vec<Alert>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, severity, agent_name, task_id, message_id, description, created_at, resolved_at
         FROM alerts WHERE resolved_at IS NULL ORDER BY created_at",
    )?;
    let alerts = stmt
        .query_map([], |row| {
            Ok(Alert {
                id: row.get(0)?,
                kind: row.get(1)?,
                severity: row.get(2)?,
                agent_name: row.get(3)?,
                task_id: row.get(4)?,
                message_id: row.get(5)?,
                description: row.get(6)?,
                created_at: row.get(7)?,
                resolved_at: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(alerts)
}

/// Return all alerts (active and resolved) for a given agent, newest first.
pub fn get_alerts_for_agent(conn: &Connection, agent_name: &str) -> Result<Vec<Alert>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, severity, agent_name, task_id, message_id, description, created_at, resolved_at
         FROM alerts WHERE agent_name = ?1 ORDER BY created_at DESC",
    )?;
    let alerts = stmt
        .query_map(params![agent_name], |row| {
            Ok(Alert {
                id: row.get(0)?,
                kind: row.get(1)?,
                severity: row.get(2)?,
                agent_name: row.get(3)?,
                task_id: row.get(4)?,
                message_id: row.get(5)?,
                description: row.get(6)?,
                created_at: row.get(7)?,
                resolved_at: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(alerts)
}

/// Stamp resolved_at on a single alert by ID.
pub fn resolve_alert(conn: &Connection, alert_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE alerts SET resolved_at = strftime('%Y-%m-%dT%H:%M:%f', 'now') WHERE id = ?1",
        params![alert_id],
    )?;
    Ok(())
}

/// Resolve all active alerts matching the given conditions.
/// `None` for a filter means that column is not constrained (any value matches).
pub fn resolve_alerts_by_condition(
    conn: &Connection,
    kind: &str,
    agent_name: Option<&str>,
    task_id: Option<&str>,
    message_id: Option<i64>,
) -> Result<()> {
    conn.execute(
        "UPDATE alerts
         SET resolved_at = strftime('%Y-%m-%dT%H:%M:%f', 'now')
         WHERE kind = ?1
           AND resolved_at IS NULL
           AND (?2 IS NULL OR agent_name = ?2)
           AND (?3 IS NULL OR task_id = ?3)
           AND (?4 IS NULL OR message_id = ?4)",
        params![kind, agent_name, task_id, message_id],
    )?;
    Ok(())
}

/// Return true if an active (unresolved) alert matching the given conditions already exists.
/// `None` for a filter means that column is not constrained (any value matches).
pub fn alert_exists(
    conn: &Connection,
    kind: &str,
    agent_name: Option<&str>,
    task_id: Option<&str>,
    message_id: Option<i64>,
) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM alerts
         WHERE kind = ?1
           AND resolved_at IS NULL
           AND (?2 IS NULL OR agent_name = ?2)
           AND (?3 IS NULL OR task_id = ?3)
           AND (?4 IS NULL OR message_id = ?4)",
        params![kind, agent_name, task_id, message_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{alerts::insert_alert, db::tests::make_store};

    #[test]
    fn alert_exists_false_when_empty() {
        let store = make_store();
        assert!(!alert_exists(store.conn(), "unanswered_msg", None, None, Some(1)).unwrap());
    }

    #[test]
    fn alert_exists_true_for_active_alert() {
        let store = make_store();
        insert_alert(
            store.conn(),
            "unanswered_msg",
            "warning",
            Some("dev-1"),
            None,
            Some(42),
            "test",
        )
        .unwrap();
        assert!(alert_exists(store.conn(), "unanswered_msg", None, None, Some(42)).unwrap());
    }

    #[test]
    fn alert_exists_false_after_resolve() {
        let store = make_store();
        let id = insert_alert(
            store.conn(),
            "unanswered_msg",
            "warning",
            Some("dev-1"),
            None,
            Some(42),
            "test",
        )
        .unwrap();
        resolve_alert(store.conn(), id).unwrap();
        assert!(!alert_exists(store.conn(), "unanswered_msg", None, None, Some(42)).unwrap());
    }

    #[test]
    fn get_active_alerts_excludes_resolved() {
        let store = make_store();
        let id1 = insert_alert(
            store.conn(),
            "stalled_task",
            "info",
            Some("agent-a"),
            Some("t1"),
            None,
            "stall",
        )
        .unwrap();
        insert_alert(
            store.conn(),
            "stalled_task",
            "info",
            Some("agent-b"),
            Some("t2"),
            None,
            "stall2",
        )
        .unwrap();
        resolve_alert(store.conn(), id1).unwrap();

        let active = get_active_alerts(store.conn()).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].agent_name.as_deref(), Some("agent-b"));
    }

    #[test]
    fn get_alerts_for_agent_returns_all_including_resolved() {
        let store = make_store();
        let id1 = insert_alert(
            store.conn(),
            "stalled_task",
            "info",
            Some("agent-x"),
            Some("t1"),
            None,
            "d1",
        )
        .unwrap();
        insert_alert(
            store.conn(),
            "unanswered_msg",
            "warning",
            Some("agent-x"),
            None,
            Some(1),
            "d2",
        )
        .unwrap();
        // Different agent — must not appear.
        insert_alert(
            store.conn(),
            "stalled_task",
            "info",
            Some("agent-y"),
            Some("t2"),
            None,
            "d3",
        )
        .unwrap();
        resolve_alert(store.conn(), id1).unwrap();

        let alerts = get_alerts_for_agent(store.conn(), "agent-x").unwrap();
        assert_eq!(alerts.len(), 2);
    }

    #[test]
    fn resolve_alerts_by_condition_resolves_matching_only() {
        let store = make_store();
        insert_alert(
            store.conn(),
            "unanswered_msg",
            "warning",
            Some("r1"),
            None,
            Some(10),
            "d1",
        )
        .unwrap();
        insert_alert(
            store.conn(),
            "unanswered_msg",
            "warning",
            Some("r2"),
            None,
            Some(11),
            "d2",
        )
        .unwrap();

        resolve_alerts_by_condition(store.conn(), "unanswered_msg", None, None, Some(10)).unwrap();

        let active = get_active_alerts(store.conn()).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].message_id, Some(11));
    }
}
