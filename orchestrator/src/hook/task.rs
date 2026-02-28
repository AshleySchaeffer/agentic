use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::protocol::{HookInput, HookOutput};
use crate::store::{agents, alerts, events, notifications, tasks};

pub fn handle_pre_task_update(conn: &Connection, event: &HookInput) -> Result<HookOutput> {
    let input = event.tool_input.as_ref();
    let task_id = input
        .and_then(|v| v.get("taskId"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let new_status = input.and_then(|v| v.get("status")).and_then(|v| v.as_str());
    let owner = input.and_then(|v| v.get("owner")).and_then(|v| v.as_str());
    let subject = input
        .and_then(|v| v.get("subject"))
        .and_then(|v| v.as_str());
    let description = input
        .and_then(|v| v.get("description"))
        .and_then(|v| v.as_str());

    let payload = event.tool_input.as_ref().map(|v| v.to_string());
    events::insert_event(
        conn,
        "task_update",
        &event.session_id,
        event.agent_name.as_deref(),
        Some("TaskUpdate"),
        payload.as_deref(),
    )
    .context("insert task_update event")?;

    // Merge status: use new_status if provided, else keep existing or default to "pending".
    let current = tasks::get_task(conn, task_id).context("read current task")?;
    let status = new_status
        .or_else(|| current.as_ref().map(|t| t.status.as_str()))
        .unwrap_or("pending");

    tasks::upsert_task(conn, task_id, subject, description, status, owner)
        .context("upsert task")?;

    // Register blockedBy dependencies.
    if let Some(blocked_by_arr) = input
        .and_then(|v| v.get("addBlockedBy"))
        .and_then(|v| v.as_array())
    {
        for dep in blocked_by_arr {
            if let Some(dep_id) = dep.as_str() {
                tasks::add_task_dep(conn, task_id, dep_id).context("add blockedBy dep")?;
            }
        }
    }

    // Register blocks dependencies (inverse direction).
    if let Some(blocks_arr) = input
        .and_then(|v| v.get("addBlocks"))
        .and_then(|v| v.as_array())
    {
        for dep in blocks_arr {
            if let Some(dep_id) = dep.as_str() {
                tasks::add_task_dep(conn, dep_id, task_id).context("add blocks dep")?;
            }
        }
    }

    if new_status == Some("completed") {
        complete_task_unblock(conn, task_id)?;
    }

    Ok(HookOutput::allow())
}

pub fn handle_task_completed(conn: &Connection, event: &HookInput) -> Result<HookOutput> {
    let task_id = event
        .tool_input
        .as_ref()
        .and_then(|v| v.get("taskId"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let payload = event.tool_input.as_ref().map(|v| v.to_string());
    events::insert_event(
        conn,
        "task_complete",
        &event.session_id,
        event.agent_name.as_deref(),
        None,
        payload.as_deref(),
    )
    .context("insert task_complete event")?;

    tasks::update_task_status(conn, task_id, "completed").context("update task to completed")?;

    complete_task_unblock(conn, task_id)?;

    Ok(HookOutput::allow())
}

pub fn handle_pre_task_query(conn: &Connection, event: &HookInput) -> Result<HookOutput> {
    // Resolve agent name.
    let agent_by_session = agents::get_agent_by_session(conn, &event.session_id)
        .context("look up agent by session")?;
    let agent_name: &str = event
        .agent_name
        .as_deref()
        .or_else(|| agent_by_session.as_ref().map(|a| a.name.as_str()))
        .unwrap_or_default();

    if agent_name.is_empty() {
        return Ok(HookOutput::allow());
    }

    let pending =
        notifications::drain_notifications(conn, agent_name).context("drain notifications")?;

    if pending.is_empty() {
        return Ok(HookOutput::allow());
    }

    let ids: Vec<i64> = pending.iter().map(|(id, _)| *id).collect();
    notifications::mark_notifications_delivered(conn, &ids)
        .context("mark notifications delivered")?;

    let context = pending
        .iter()
        .map(|(_, content)| content.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(HookOutput {
        exit_code: 0,
        updated_input: None,
        additional_context: Some(context),
        stderr_message: None,
    })
}

/// Resolve all deps where `completed_task_id` is the blocker, then queue notifications
/// for tasks that just became fully unblocked as a result.
fn complete_task_unblock(conn: &Connection, completed_task_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE task_deps SET resolved = 1 WHERE blocked_by = ?1",
        rusqlite::params![completed_task_id],
    )
    .context("resolve deps for completed task")?;

    // Tasks that had this task as a blocker AND are now fully unblocked.
    let mut stmt = conn
        .prepare(
            "SELECT t.task_id, t.owner, t.subject
             FROM tasks t
             WHERE t.status IN ('pending', 'in_progress')
               AND EXISTS (
                   SELECT 1 FROM task_deps d2
                   WHERE d2.task_id = t.task_id AND d2.blocked_by = ?1
               )
               AND NOT EXISTS (
                   SELECT 1 FROM task_deps d
                   WHERE d.task_id = t.task_id AND d.resolved = 0
               )",
        )
        .context("prepare newly-unblocked query")?;

    let newly_unblocked: Vec<(String, Option<String>, Option<String>)> = stmt
        .query_map(rusqlite::params![completed_task_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .context("query newly-unblocked tasks")?
        .collect::<rusqlite::Result<_>>()
        .context("collect newly-unblocked tasks")?;

    for (task_id, owner, subject) in newly_unblocked {
        if let Some(owner_name) = &owner {
            let content = format!(
                "Task '{}' is now unblocked.",
                subject.as_deref().unwrap_or(&task_id)
            );
            notifications::insert_notification(conn, owner_name, 0, &content)
                .context("insert unblock notification")?;
        }

        alerts::insert_alert(
            conn,
            "unblocked_pending",
            "info",
            None,
            Some(&task_id),
            None,
            &format!("Task '{}' became unblocked", task_id),
        )
        .context("insert unblocked_pending alert")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::EventType;
    use crate::store::{agents, db::tests::make_store, notifications, tasks};

    fn make_task_update_event(
        session_id: &str,
        agent_name: Option<&str>,
        task_id: &str,
        status: Option<&str>,
        owner: Option<&str>,
        add_blocked_by: Option<Vec<&str>>,
        add_blocks: Option<Vec<&str>>,
    ) -> HookInput {
        let mut obj = serde_json::json!({ "taskId": task_id });
        if let Some(s) = status {
            obj["status"] = serde_json::Value::String(s.to_string());
        }
        if let Some(o) = owner {
            obj["owner"] = serde_json::Value::String(o.to_string());
        }
        if let Some(deps) = add_blocked_by {
            obj["addBlockedBy"] =
                serde_json::Value::Array(deps.iter().map(|d| serde_json::json!(d)).collect());
        }
        if let Some(blocks) = add_blocks {
            obj["addBlocks"] =
                serde_json::Value::Array(blocks.iter().map(|d| serde_json::json!(d)).collect());
        }
        HookInput {
            event_type: EventType::PreToolUse,
            session_id: session_id.to_string(),
            tool_name: Some("TaskUpdate".to_string()),
            tool_input: Some(obj),
            agent_name: agent_name.map(String::from),
            agent_type: None,
            spawner_session_id: None,
        }
    }

    fn make_query_event(session_id: &str, agent_name: Option<&str>, tool: &str) -> HookInput {
        HookInput {
            event_type: EventType::PreToolUse,
            session_id: session_id.to_string(),
            tool_name: Some(tool.to_string()),
            tool_input: None,
            agent_name: agent_name.map(String::from),
            agent_type: None,
            spawner_session_id: None,
        }
    }

    #[test]
    fn task_update_upserts_task() {
        let store = make_store();
        let event = make_task_update_event(
            "sess-1",
            Some("arch"),
            "task-A",
            Some("in_progress"),
            Some("dev-1"),
            None,
            None,
        );
        handle_pre_task_update(store.conn(), &event).expect("task_update");

        let task = tasks::get_task(store.conn(), "task-A")
            .expect("query")
            .expect("exists");
        assert_eq!(task.status, "in_progress");
        assert_eq!(task.owner.as_deref(), Some("dev-1"));
    }

    #[test]
    fn task_update_preserves_existing_status_when_not_set() {
        let store = make_store();
        tasks::upsert_task(
            store.conn(),
            "task-B",
            None,
            None,
            "in_progress",
            Some("dev-2"),
        )
        .expect("upsert");

        // Update only owner — no status in input.
        let event = make_task_update_event(
            "sess-1",
            Some("arch"),
            "task-B",
            None,
            Some("dev-3"),
            None,
            None,
        );
        handle_pre_task_update(store.conn(), &event).expect("task_update no-status");

        let task = tasks::get_task(store.conn(), "task-B")
            .expect("query")
            .expect("exists");
        assert_eq!(task.status, "in_progress"); // not overwritten to "pending"
        assert_eq!(task.owner.as_deref(), Some("dev-3"));
    }

    #[test]
    fn unblock_detection_queues_notification_and_alert() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "owner-a", "sess-a", None, None).expect("upsert");
        tasks::upsert_task(store.conn(), "blocker", None, None, "pending", None).expect("upsert");
        tasks::upsert_task(
            store.conn(),
            "blocked",
            Some("Do work"),
            None,
            "pending",
            Some("owner-a"),
        )
        .expect("upsert");
        tasks::add_task_dep(store.conn(), "blocked", "blocker").expect("add dep");

        // Complete blocker.
        let event = make_task_update_event(
            "sess-a",
            Some("arch"),
            "blocker",
            Some("completed"),
            None,
            None,
            None,
        );
        handle_pre_task_update(store.conn(), &event).expect("complete blocker");

        // Notification queued for owner-a.
        let notifs = notifications::drain_notifications(store.conn(), "owner-a").expect("drain");
        assert_eq!(notifs.len(), 1);
        assert!(notifs[0].1.contains("blocked") || notifs[0].1.contains("Do work"));

        // Alert created.
        let count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM alerts WHERE kind = 'unblocked_pending' AND task_id = 'blocked'",
                [],
                |row| row.get(0),
            )
            .expect("query");
        assert_eq!(count, 1);
    }

    #[test]
    fn unblock_detection_only_triggers_fully_unblocked_tasks() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "owner-b", "sess-b", None, None).expect("upsert");
        tasks::upsert_task(store.conn(), "dep-1", None, None, "pending", None).expect("upsert");
        tasks::upsert_task(store.conn(), "dep-2", None, None, "pending", None).expect("upsert");
        tasks::upsert_task(
            store.conn(),
            "both-blocked",
            None,
            None,
            "pending",
            Some("owner-b"),
        )
        .expect("upsert");
        tasks::add_task_dep(store.conn(), "both-blocked", "dep-1").expect("dep 1");
        tasks::add_task_dep(store.conn(), "both-blocked", "dep-2").expect("dep 2");

        // Complete dep-1 only.
        let event =
            make_task_update_event("sess-b", None, "dep-1", Some("completed"), None, None, None);
        handle_pre_task_update(store.conn(), &event).expect("complete dep-1");

        // Still blocked by dep-2 — no notification.
        let notifs = notifications::drain_notifications(store.conn(), "owner-b").expect("drain");
        assert_eq!(notifs.len(), 0);

        // Complete dep-2.
        let event2 =
            make_task_update_event("sess-b", None, "dep-2", Some("completed"), None, None, None);
        handle_pre_task_update(store.conn(), &event2).expect("complete dep-2");

        // Now both-blocked is fully unblocked.
        let notifs2 = notifications::drain_notifications(store.conn(), "owner-b").expect("drain 2");
        assert_eq!(notifs2.len(), 1);
    }

    #[test]
    fn notification_draining_returns_context_and_marks_delivered() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "worker-x", "sess-x", None, None).expect("upsert");
        notifications::insert_notification(store.conn(), "worker-x", 0, "unblocked: task-1")
            .expect("insert notif 1");
        notifications::insert_notification(store.conn(), "worker-x", 1, "agent died: dev-2")
            .expect("insert notif 2");

        let event = make_query_event("sess-x", Some("worker-x"), "TaskList");
        let output = handle_pre_task_query(store.conn(), &event).expect("task_query");

        assert_eq!(output.exit_code, 0);
        let ctx = output.additional_context.expect("has context");
        // Priority 0 first, then priority 1.
        assert!(ctx.contains("unblocked: task-1"));
        assert!(ctx.contains("agent died: dev-2"));

        // Both should now be marked delivered (drain returns empty).
        let remaining =
            notifications::drain_notifications(store.conn(), "worker-x").expect("drain again");
        assert_eq!(remaining.len(), 0);
    }

    #[test]
    fn task_query_no_notifications_returns_allow_without_context() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "quiet", "sess-q", None, None).expect("upsert");

        let event = make_query_event("sess-q", Some("quiet"), "TaskGet");
        let output = handle_pre_task_query(store.conn(), &event).expect("task_query");

        assert_eq!(output.exit_code, 0);
        assert!(output.additional_context.is_none());
    }

    #[test]
    fn task_completed_event_marks_task_and_unblocks() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "owner-c", "sess-c", None, None).expect("upsert");
        tasks::upsert_task(store.conn(), "src-task", None, None, "in_progress", None)
            .expect("upsert");
        tasks::upsert_task(
            store.conn(),
            "dep-task",
            None,
            None,
            "pending",
            Some("owner-c"),
        )
        .expect("upsert");
        tasks::add_task_dep(store.conn(), "dep-task", "src-task").expect("dep");

        let event = HookInput {
            event_type: EventType::TaskCompleted,
            session_id: "sess-c".to_string(),
            tool_name: None,
            tool_input: Some(serde_json::json!({ "taskId": "src-task" })),
            agent_name: Some("owner-c".to_string()),
            agent_type: None,
            spawner_session_id: None,
        };
        handle_task_completed(store.conn(), &event).expect("task_completed");

        let task = tasks::get_task(store.conn(), "src-task")
            .expect("query")
            .expect("exists");
        assert_eq!(task.status, "completed");

        let notifs = notifications::drain_notifications(store.conn(), "owner-c").expect("drain");
        assert_eq!(notifs.len(), 1);
    }
}
