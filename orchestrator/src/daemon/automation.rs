use crate::config::Config;
use crate::daemon::alerts::{alert_exists, resolve_alert};
use crate::store::alerts::insert_alert;
use crate::store::messages::get_unanswered;
use crate::store::notifications::insert_notification;
use anyhow::Result;
use rusqlite::{params, Connection};

/// Run one automation tick: detect new problems, queue notifications, resolve cleared alerts.
pub fn run_automation_tick(conn: &Connection, config: &Config) -> Result<()> {
    detect_unanswered_messages(conn, config)?;
    detect_stalled_tasks(conn, config)?;
    detect_team_completion(conn)?;
    resolve_answered_messages(conn)?;
    resolve_resumed_agents(conn, config)?;
    Ok(())
}

/// Create a warning alert and a priority-2 notification for each delivered message that has
/// exceeded the unanswered timeout and does not already have an open alert.
fn detect_unanswered_messages(conn: &Connection, config: &Config) -> Result<()> {
    let messages = get_unanswered(conn, config.unanswered_timeout_secs)?;
    for msg in messages {
        if !alert_exists(conn, "unanswered_msg", None, None, Some(msg.id))? {
            let description = format!(
                "Message {} from '{}' to '{}' has not been answered",
                msg.id, msg.sender, msg.recipient
            );
            insert_alert(
                conn,
                "unanswered_msg",
                "warning",
                Some(&msg.recipient),
                None,
                Some(msg.id),
                &description,
            )?;
            insert_notification(conn, &msg.recipient, 2, &description)?;
        }
    }
    Ok(())
}

/// Create an info alert (TUI-visible only, no notification) for each active agent whose
/// in-progress task has not shown activity within the stall timeout.
fn detect_stalled_tasks(conn: &Connection, config: &Config) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT a.name, t.task_id
         FROM agents a
         JOIN tasks t ON t.owner = a.name AND t.status = 'in_progress'
         WHERE a.status = 'active'
           AND (julianday('now') - julianday(a.last_activity)) * 86400 > ?1",
    )?;
    let stalls: Vec<(String, String)> = stmt
        .query_map(params![config.stall_timeout_secs as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for (agent_name, task_id) in stalls {
        if !alert_exists(
            conn,
            "stalled_task",
            Some(&agent_name),
            Some(&task_id),
            None,
        )? {
            let description = format!("Agent '{}' has stalled on task '{}'", agent_name, task_id);
            insert_alert(
                conn,
                "stalled_task",
                "info",
                Some(&agent_name),
                Some(&task_id),
                None,
                &description,
            )?;
        }
    }
    Ok(())
}

/// Resolve open unanswered_msg alerts whose message has since received a response.
fn resolve_answered_messages(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT al.id FROM alerts al
         JOIN messages m ON m.id = al.message_id
         WHERE al.kind = 'unanswered_msg'
           AND al.resolved_at IS NULL
           AND m.response_received = 1",
    )?;
    let ids: Vec<i64> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        resolve_alert(conn, id)?;
    }
    Ok(())
}

/// Create a one-time info alert when all tasks are completed and all agents are dead.
/// The TUI uses this alert to show an auto-close prompt.
fn detect_team_completion(conn: &Connection) -> Result<()> {
    let task_count: i64 = conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
    let agent_count: i64 = conn.query_row("SELECT COUNT(*) FROM agents", [], |r| r.get(0))?;

    if task_count == 0 || agent_count == 0 {
        return Ok(());
    }

    let pending_tasks: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status != 'completed'",
        [],
        |r| r.get(0),
    )?;
    let alive_agents: i64 = conn.query_row(
        "SELECT COUNT(*) FROM agents WHERE status != 'dead'",
        [],
        |r| r.get(0),
    )?;

    if pending_tasks == 0
        && alive_agents == 0
        && !alert_exists(conn, "team_completed", None, None, None)?
    {
        insert_alert(
            conn,
            "team_completed",
            "info",
            None,
            None,
            None,
            "All tasks completed and all agents have exited",
        )?;
    }
    Ok(())
}

/// Resolve open stalled_task alerts for agents that have since shown recent activity.
fn resolve_resumed_agents(conn: &Connection, config: &Config) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT al.id FROM alerts al
         JOIN agents a ON a.name = al.agent_name
         WHERE al.kind = 'stalled_task'
           AND al.resolved_at IS NULL
           AND (julianday('now') - julianday(a.last_activity)) * 86400 <= ?1",
    )?;
    let ids: Vec<i64> = stmt
        .query_map(params![config.stall_timeout_secs as i64], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        resolve_alert(conn, id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::alerts::{alert_exists, get_active_alerts};
    use crate::store::{
        agents::upsert_agent,
        db::tests::make_store,
        messages::{insert_message, mark_delivered, mark_response_received},
        notifications::drain_notifications,
        tasks::upsert_task,
    };

    fn test_config() -> Config {
        Config {
            unanswered_timeout_secs: 300,
            stall_timeout_secs: 600,
            automation_interval_secs: 30,
            ..Config::default()
        }
    }

    #[test]
    fn unanswered_message_creates_alert_and_notification() {
        let store = make_store();
        upsert_agent(store.conn(), "sender", "sess-s", None, None).unwrap();
        upsert_agent(store.conn(), "recip", "sess-r", None, None).unwrap();
        let msg_id = insert_message(store.conn(), "sender", "recip", None, None, "sent").unwrap();
        mark_delivered(store.conn(), msg_id).unwrap();
        // Age the delivered_at timestamp well beyond the timeout.
        store
            .conn()
            .execute(
                "UPDATE messages SET delivered_at = '2020-01-01T00:00:00.000' WHERE id = ?1",
                params![msg_id],
            )
            .unwrap();

        run_automation_tick(store.conn(), &test_config()).unwrap();

        assert!(alert_exists(store.conn(), "unanswered_msg", None, None, Some(msg_id)).unwrap());
        let notifs = drain_notifications(store.conn(), "recip").unwrap();
        assert_eq!(notifs.len(), 1);
    }

    #[test]
    fn stall_detection_creates_alert_no_notification() {
        let store = make_store();
        upsert_agent(store.conn(), "dev-1", "sess-d", None, None).unwrap();
        upsert_task(
            store.conn(),
            "task-1",
            None,
            None,
            "in_progress",
            Some("dev-1"),
        )
        .unwrap();
        // Age the agent's last_activity beyond the stall timeout.
        store
            .conn()
            .execute(
                "UPDATE agents SET last_activity = '2020-01-01T00:00:00.000' WHERE name = 'dev-1'",
                [],
            )
            .unwrap();

        run_automation_tick(store.conn(), &test_config()).unwrap();

        assert!(alert_exists(
            store.conn(),
            "stalled_task",
            Some("dev-1"),
            Some("task-1"),
            None
        )
        .unwrap());
        // Stall alerts must not produce a notification.
        let notifs = drain_notifications(store.conn(), "dev-1").unwrap();
        assert!(notifs.is_empty());
    }

    #[test]
    fn second_tick_does_not_duplicate_alerts() {
        let store = make_store();
        upsert_agent(store.conn(), "dev-2", "sess-d2", None, None).unwrap();
        upsert_task(
            store.conn(),
            "task-2",
            None,
            None,
            "in_progress",
            Some("dev-2"),
        )
        .unwrap();
        store
            .conn()
            .execute(
                "UPDATE agents SET last_activity = '2020-01-01T00:00:00.000' WHERE name = 'dev-2'",
                [],
            )
            .unwrap();

        run_automation_tick(store.conn(), &test_config()).unwrap();
        run_automation_tick(store.conn(), &test_config()).unwrap();

        let active = get_active_alerts(store.conn()).unwrap();
        let stall_count = active.iter().filter(|a| a.kind == "stalled_task").count();
        assert_eq!(stall_count, 1);
    }

    #[test]
    fn auto_resolve_unanswered_on_response() {
        let store = make_store();
        upsert_agent(store.conn(), "sender2", "sess-s2", None, None).unwrap();
        upsert_agent(store.conn(), "recip2", "sess-r2", None, None).unwrap();
        let msg_id = insert_message(store.conn(), "sender2", "recip2", None, None, "sent").unwrap();
        mark_delivered(store.conn(), msg_id).unwrap();
        store
            .conn()
            .execute(
                "UPDATE messages SET delivered_at = '2020-01-01T00:00:00.000' WHERE id = ?1",
                params![msg_id],
            )
            .unwrap();

        // First tick creates the alert.
        run_automation_tick(store.conn(), &test_config()).unwrap();
        assert!(alert_exists(store.conn(), "unanswered_msg", None, None, Some(msg_id)).unwrap());

        // Recipient responds; second tick resolves the alert.
        mark_response_received(store.conn(), msg_id).unwrap();
        run_automation_tick(store.conn(), &test_config()).unwrap();
        assert!(!alert_exists(store.conn(), "unanswered_msg", None, None, Some(msg_id)).unwrap());
    }

    #[test]
    fn team_completion_creates_alert_when_all_done() {
        let store = make_store();
        upsert_agent(store.conn(), "agent-fin", "sess-f", None, None).unwrap();
        upsert_task(
            store.conn(),
            "task-fin",
            None,
            None,
            "completed",
            Some("agent-fin"),
        )
        .unwrap();
        // Mark agent dead.
        store
            .conn()
            .execute(
                "UPDATE agents SET status = 'dead' WHERE name = 'agent-fin'",
                [],
            )
            .unwrap();

        run_automation_tick(store.conn(), &test_config()).unwrap();

        assert!(alert_exists(store.conn(), "team_completed", None, None, None).unwrap());
    }

    #[test]
    fn team_completion_no_alert_when_tasks_pending() {
        let store = make_store();
        upsert_agent(store.conn(), "agent-pend", "sess-p", None, None).unwrap();
        upsert_task(store.conn(), "task-pend", None, None, "pending", None).unwrap();
        store
            .conn()
            .execute(
                "UPDATE agents SET status = 'dead' WHERE name = 'agent-pend'",
                [],
            )
            .unwrap();

        run_automation_tick(store.conn(), &test_config()).unwrap();

        assert!(!alert_exists(store.conn(), "team_completed", None, None, None).unwrap());
    }

    #[test]
    fn team_completion_no_duplicate_alert_on_second_tick() {
        let store = make_store();
        upsert_agent(store.conn(), "agent-dup", "sess-d", None, None).unwrap();
        upsert_task(
            store.conn(),
            "task-dup",
            None,
            None,
            "completed",
            Some("agent-dup"),
        )
        .unwrap();
        store
            .conn()
            .execute(
                "UPDATE agents SET status = 'dead' WHERE name = 'agent-dup'",
                [],
            )
            .unwrap();

        run_automation_tick(store.conn(), &test_config()).unwrap();
        run_automation_tick(store.conn(), &test_config()).unwrap();

        let count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM alerts WHERE kind = 'team_completed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "only one team_completed alert should be created");
    }

    #[test]
    fn auto_resolve_stall_on_resumed_activity() {
        let store = make_store();
        upsert_agent(store.conn(), "dev-3", "sess-d3", None, None).unwrap();
        upsert_task(
            store.conn(),
            "task-3",
            None,
            None,
            "in_progress",
            Some("dev-3"),
        )
        .unwrap();
        store
            .conn()
            .execute(
                "UPDATE agents SET last_activity = '2020-01-01T00:00:00.000' WHERE name = 'dev-3'",
                [],
            )
            .unwrap();

        // First tick creates stall alert.
        run_automation_tick(store.conn(), &test_config()).unwrap();
        assert!(alert_exists(
            store.conn(),
            "stalled_task",
            Some("dev-3"),
            Some("task-3"),
            None
        )
        .unwrap());

        // Agent resumes activity; second tick resolves the stall alert.
        store
            .conn()
            .execute(
                "UPDATE agents
                 SET last_activity = strftime('%Y-%m-%dT%H:%M:%f', 'now')
                 WHERE name = 'dev-3'",
                [],
            )
            .unwrap();
        run_automation_tick(store.conn(), &test_config()).unwrap();
        assert!(!alert_exists(
            store.conn(),
            "stalled_task",
            Some("dev-3"),
            Some("task-3"),
            None
        )
        .unwrap());
    }
}
