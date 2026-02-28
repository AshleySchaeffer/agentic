use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::protocol::{HookInput, HookOutput};
use crate::store::{agents, alerts, events, messages, notifications};

pub fn handle_session_start(conn: &Connection, event: &HookInput) -> Result<HookOutput> {
    events::insert_event(
        conn,
        "session_start",
        &event.session_id,
        event.agent_name.as_deref(),
        None,
        None,
    )
    .context("insert session_start event")?;

    if let Some(name) = &event.agent_name {
        agents::upsert_agent(
            conn,
            name,
            &event.session_id,
            event.agent_type.as_deref(),
            event.spawner_session_id.as_deref(),
        )
        .context("upsert agent on session_start")?;
    }

    Ok(HookOutput::allow())
}

pub fn handle_session_end(conn: &Connection, event: &HookInput) -> Result<HookOutput> {
    events::insert_event(
        conn,
        "session_end",
        &event.session_id,
        event.agent_name.as_deref(),
        None,
        None,
    )
    .context("insert session_end event")?;

    let agent = agents::get_agent_by_session(conn, &event.session_id)
        .context("look up agent by session")?;

    if let Some(agent) = agent {
        agents::update_agent_status(conn, &agent.name, "dead").context("mark agent dead")?;

        // Undelivered messages TO this agent — alert senders and queue notifications.
        let undelivered =
            messages::get_undelivered_for(conn, &agent.name).context("get undelivered messages")?;
        for msg in &undelivered {
            alerts::insert_alert(
                conn,
                "dead_agent",
                "warning",
                Some(&agent.name),
                None,
                Some(msg.id),
                &format!(
                    "Agent '{}' died with undelivered message from '{}'",
                    agent.name, msg.sender
                ),
            )
            .context("insert dead_agent alert")?;
            notifications::insert_notification(
                conn,
                &msg.sender,
                1,
                &format!(
                    "Agent '{}' (your message recipient) has died. Message may not have been received.",
                    agent.name
                ),
            )
            .context("insert dead_agent notification")?;
        }

        // In-progress tasks owned by this agent — alert only.
        let mut stmt = conn
            .prepare("SELECT task_id FROM tasks WHERE owner = ?1 AND status = 'in_progress'")
            .context("prepare stalled tasks query")?;
        let stalled: Vec<String> = stmt
            .query_map(rusqlite::params![agent.name], |row| row.get(0))
            .context("query stalled tasks")?
            .collect::<rusqlite::Result<_>>()
            .context("collect stalled tasks")?;
        for task_id in &stalled {
            alerts::insert_alert(
                conn,
                "dead_agent",
                "critical",
                Some(&agent.name),
                Some(task_id),
                None,
                &format!(
                    "Agent '{}' died with in-progress task '{}'",
                    agent.name, task_id
                ),
            )
            .context("insert stalled task alert")?;
        }
    }

    Ok(HookOutput::allow())
}

pub fn handle_subagent_start(conn: &Connection, event: &HookInput) -> Result<HookOutput> {
    events::insert_event(
        conn,
        "session_start",
        &event.session_id,
        event.agent_name.as_deref(),
        None,
        None,
    )
    .context("insert session_start event for subagent")?;

    if let Some(name) = &event.agent_name {
        agents::upsert_agent(
            conn,
            name,
            &event.session_id,
            event.agent_type.as_deref(),
            event.spawner_session_id.as_deref(),
        )
        .context("upsert subagent")?;
    }

    Ok(HookOutput::allow())
}

pub fn handle_teammate_idle(conn: &Connection, event: &HookInput) -> Result<HookOutput> {
    events::insert_event(
        conn,
        "agent_idle",
        &event.session_id,
        event.agent_name.as_deref(),
        None,
        None,
    )
    .context("insert agent_idle event")?;

    if let Some(name) = &event.agent_name {
        agents::update_agent_status(conn, name, "idle").context("mark agent idle")?;
    }

    Ok(HookOutput::allow())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::EventType;
    use crate::store::db::tests::make_store;

    fn make_event(
        event_type: EventType,
        session_id: &str,
        agent_name: Option<&str>,
        agent_type: Option<&str>,
        spawner_session_id: Option<&str>,
    ) -> HookInput {
        HookInput {
            event_type,
            session_id: session_id.to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: agent_name.map(String::from),
            agent_type: agent_type.map(String::from),
            spawner_session_id: spawner_session_id.map(String::from),
        }
    }

    #[test]
    fn session_start_creates_active_agent() {
        let store = make_store();
        let event = make_event(
            EventType::SessionStart,
            "sess-1",
            Some("architect"),
            Some("architect"),
            None,
        );
        handle_session_start(store.conn(), &event).expect("session_start");

        let agent = agents::get_agent(store.conn(), "architect")
            .expect("query")
            .expect("agent exists");
        assert_eq!(agent.status, "active");
        assert_eq!(agent.session_id, "sess-1");
        assert_eq!(agent.agent_type.as_deref(), Some("architect"));
    }

    #[test]
    fn session_start_no_agent_name_is_noop_for_agents_table() {
        let store = make_store();
        let event = make_event(EventType::SessionStart, "sess-anon", None, None, None);
        handle_session_start(store.conn(), &event).expect("session_start no-name");
        // No agent row created, but event was inserted.
        assert_eq!(agents::list_agents(store.conn()).expect("list").len(), 0);
    }

    #[test]
    fn session_end_marks_agent_dead() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "dev-1", "sess-dev", None, None).expect("upsert");

        let event = make_event(EventType::SessionEnd, "sess-dev", Some("dev-1"), None, None);
        handle_session_end(store.conn(), &event).expect("session_end");

        let agent = agents::get_agent(store.conn(), "dev-1")
            .expect("query")
            .expect("exists");
        assert_eq!(agent.status, "dead");
    }

    #[test]
    fn session_end_notifies_sender_of_undelivered_message() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "dev-1", "sess-dev", None, None).expect("upsert");
        agents::upsert_agent(store.conn(), "sender-a", "sess-a", None, None).expect("upsert");
        // Undelivered message to dev-1 from sender-a.
        crate::store::messages::insert_message(
            store.conn(),
            "sender-a",
            "dev-1",
            None,
            None,
            "sent",
        )
        .expect("insert message");

        let event = make_event(EventType::SessionEnd, "sess-dev", Some("dev-1"), None, None);
        handle_session_end(store.conn(), &event).expect("session_end");

        // Notification should be queued for sender-a.
        let notifs = notifications::drain_notifications(store.conn(), "sender-a").expect("drain");
        assert_eq!(notifs.len(), 1);
        assert!(notifs[0].1.contains("dev-1"));
    }

    #[test]
    fn session_end_alerts_on_in_progress_tasks() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "dev-2", "sess-dev2", None, None).expect("upsert");
        crate::store::tasks::upsert_task(
            store.conn(),
            "task-99",
            None,
            None,
            "in_progress",
            Some("dev-2"),
        )
        .expect("upsert task");

        let event = make_event(
            EventType::SessionEnd,
            "sess-dev2",
            Some("dev-2"),
            None,
            None,
        );
        handle_session_end(store.conn(), &event).expect("session_end");

        let count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM alerts WHERE kind = 'dead_agent' AND task_id = 'task-99'",
                [],
                |row| row.get(0),
            )
            .expect("query alerts");
        assert_eq!(count, 1);
    }

    #[test]
    fn subagent_start_records_spawner() {
        let store = make_store();
        let event = make_event(
            EventType::SubagentStart,
            "sess-sub",
            Some("sub-agent"),
            Some("dev"),
            Some("sess-parent"),
        );
        handle_subagent_start(store.conn(), &event).expect("subagent_start");

        let agent = agents::get_agent(store.conn(), "sub-agent")
            .expect("query")
            .expect("exists");
        assert_eq!(agent.spawner_session_id.as_deref(), Some("sess-parent"));
        assert_eq!(agent.agent_type.as_deref(), Some("dev"));
    }

    #[test]
    fn teammate_idle_marks_agent_idle() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "worker", "sess-w", None, None).expect("upsert");

        let event = make_event(
            EventType::TeammateIdle,
            "sess-w",
            Some("worker"),
            None,
            None,
        );
        handle_teammate_idle(store.conn(), &event).expect("teammate_idle");

        let agent = agents::get_agent(store.conn(), "worker")
            .expect("query")
            .expect("exists");
        assert_eq!(agent.status, "idle");
    }
}
