mod common;

use orchestrator::store::{tasks, Store};

#[tokio::test]
async fn task_create_and_update_through_socket() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client.session_start("sess-tc1", "agent-tc1", None).await;

    client
        .task_update(
            "sess-tc1",
            Some("agent-tc1"),
            serde_json::json!({
                "taskId": "task-tc1",
                "subject": "Test subject",
                "status": "pending"
            }),
        )
        .await;

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let task = tasks::get_task(store.conn(), "task-tc1")
        .expect("query")
        .expect("task exists");
    assert_eq!(task.status, "pending");
    assert_eq!(task.subject.as_deref(), Some("Test subject"));

    client
        .task_update(
            "sess-tc1",
            Some("agent-tc1"),
            serde_json::json!({
                "taskId": "task-tc1",
                "status": "in_progress"
            }),
        )
        .await;

    let task = tasks::get_task(store.conn(), "task-tc1")
        .expect("query")
        .expect("task exists");
    assert_eq!(task.status, "in_progress");
}

#[tokio::test]
async fn blocked_by_dependency_and_unblock_cascade() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client
        .session_start("sess-arch-tc2", "arch-tc2", None)
        .await;
    client.session_start("sess-dev-tc2", "dev-tc2", None).await;

    client
        .task_update(
            "sess-arch-tc2",
            Some("arch-tc2"),
            serde_json::json!({ "taskId": "blocker-tc2", "status": "pending" }),
        )
        .await;

    client
        .task_update(
            "sess-arch-tc2",
            Some("arch-tc2"),
            serde_json::json!({
                "taskId": "blocked-tc2",
                "subject": "Blocked task",
                "status": "pending",
                "owner": "dev-tc2",
                "addBlockedBy": ["blocker-tc2"]
            }),
        )
        .await;

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let unblocked = tasks::get_unblocked_tasks(store.conn()).expect("unblocked");
    let ids: Vec<&str> = unblocked.iter().map(|t| t.task_id.as_str()).collect();
    assert!(
        !ids.contains(&"blocked-tc2"),
        "blocked-tc2 should not be unblocked yet"
    );

    client
        .task_update(
            "sess-arch-tc2",
            Some("arch-tc2"),
            serde_json::json!({ "taskId": "blocker-tc2", "status": "completed" }),
        )
        .await;

    let output = client
        .task_query("sess-dev-tc2", "dev-tc2", "TaskList")
        .await;
    assert_eq!(output.exit_code, 0);
    let ctx = output
        .additional_context
        .expect("unblock notification present");
    assert!(
        ctx.contains("unblocked") || ctx.contains("Blocked task"),
        "unexpected context: {ctx}"
    );
}

#[tokio::test]
async fn multi_dependency_only_unblocks_when_all_resolved() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client
        .session_start("sess-arch-tc3", "arch-tc3", None)
        .await;
    client.session_start("sess-dev-tc3", "dev-tc3", None).await;

    client
        .task_update(
            "sess-arch-tc3",
            Some("arch-tc3"),
            serde_json::json!({ "taskId": "dep-a-tc3", "status": "pending" }),
        )
        .await;
    client
        .task_update(
            "sess-arch-tc3",
            Some("arch-tc3"),
            serde_json::json!({ "taskId": "dep-b-tc3", "status": "pending" }),
        )
        .await;
    client
        .task_update(
            "sess-arch-tc3",
            Some("arch-tc3"),
            serde_json::json!({
                "taskId": "multi-tc3",
                "subject": "Multi-blocked task",
                "status": "pending",
                "owner": "dev-tc3",
                "addBlockedBy": ["dep-a-tc3", "dep-b-tc3"]
            }),
        )
        .await;

    client
        .task_update(
            "sess-arch-tc3",
            Some("arch-tc3"),
            serde_json::json!({ "taskId": "dep-a-tc3", "status": "completed" }),
        )
        .await;

    let output = client
        .task_query("sess-dev-tc3", "dev-tc3", "TaskList")
        .await;
    assert_eq!(output.exit_code, 0);
    assert!(
        output.additional_context.is_none(),
        "should not get notification while dep-b still blocks"
    );

    client
        .task_update(
            "sess-arch-tc3",
            Some("arch-tc3"),
            serde_json::json!({ "taskId": "dep-b-tc3", "status": "completed" }),
        )
        .await;

    let output = client
        .task_query("sess-dev-tc3", "dev-tc3", "TaskList")
        .await;
    assert_eq!(output.exit_code, 0);
    assert!(
        output.additional_context.is_some(),
        "should get notification after all blockers complete"
    );
}

#[tokio::test]
async fn task_completed_event_triggers_unblock() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client
        .session_start("sess-arch-tc4", "arch-tc4", None)
        .await;
    client.session_start("sess-dev-tc4", "dev-tc4", None).await;

    client
        .task_update(
            "sess-arch-tc4",
            Some("arch-tc4"),
            serde_json::json!({ "taskId": "blocker-tc4", "status": "pending" }),
        )
        .await;
    client
        .task_update(
            "sess-arch-tc4",
            Some("arch-tc4"),
            serde_json::json!({
                "taskId": "blocked-tc4",
                "status": "pending",
                "owner": "dev-tc4",
                "addBlockedBy": ["blocker-tc4"]
            }),
        )
        .await;

    client
        .task_completed("sess-arch-tc4", "arch-tc4", "blocker-tc4")
        .await;

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let task = tasks::get_task(store.conn(), "blocker-tc4")
        .expect("query")
        .expect("exists");
    assert_eq!(task.status, "completed");

    let output = client
        .task_query("sess-dev-tc4", "dev-tc4", "TaskList")
        .await;
    assert_eq!(output.exit_code, 0);
    assert!(
        output.additional_context.is_some(),
        "TaskCompleted event should trigger unblock notification"
    );
}

#[tokio::test]
async fn task_query_drains_notifications_once() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client
        .session_start("sess-arch-tc5", "arch-tc5", None)
        .await;
    client.session_start("sess-dev-tc5", "dev-tc5", None).await;

    client
        .task_update(
            "sess-arch-tc5",
            Some("arch-tc5"),
            serde_json::json!({ "taskId": "blocker-tc5", "status": "pending" }),
        )
        .await;
    client
        .task_update(
            "sess-arch-tc5",
            Some("arch-tc5"),
            serde_json::json!({
                "taskId": "blocked-tc5",
                "status": "pending",
                "owner": "dev-tc5",
                "addBlockedBy": ["blocker-tc5"]
            }),
        )
        .await;

    client
        .task_update(
            "sess-arch-tc5",
            Some("arch-tc5"),
            serde_json::json!({ "taskId": "blocker-tc5", "status": "completed" }),
        )
        .await;

    let first = client
        .task_query("sess-dev-tc5", "dev-tc5", "TaskList")
        .await;
    assert_eq!(first.exit_code, 0);
    assert!(
        first.additional_context.is_some(),
        "first query should return notification"
    );

    let second = client
        .task_query("sess-dev-tc5", "dev-tc5", "TaskList")
        .await;
    assert_eq!(second.exit_code, 0);
    assert!(
        second.additional_context.is_none(),
        "second query should return no notification (already drained)"
    );
}

#[tokio::test]
async fn blocks_dependency_inverse_direction() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client
        .session_start("sess-arch-tc6", "arch-tc6", None)
        .await;
    client.session_start("sess-dev-tc6", "dev-tc6", None).await;

    client
        .task_update(
            "sess-arch-tc6",
            Some("arch-tc6"),
            serde_json::json!({ "taskId": "src-tc6", "status": "pending" }),
        )
        .await;
    client
        .task_update(
            "sess-arch-tc6",
            Some("arch-tc6"),
            serde_json::json!({
                "taskId": "dest-tc6",
                "status": "pending",
                "owner": "dev-tc6"
            }),
        )
        .await;

    // Register dependency via addBlocks on src — dest becomes blocked_by src.
    client
        .task_update(
            "sess-arch-tc6",
            Some("arch-tc6"),
            serde_json::json!({ "taskId": "src-tc6", "addBlocks": ["dest-tc6"] }),
        )
        .await;

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let unblocked = tasks::get_unblocked_tasks(store.conn()).expect("unblocked");
    let ids: Vec<&str> = unblocked.iter().map(|t| t.task_id.as_str()).collect();
    assert!(
        !ids.contains(&"dest-tc6"),
        "dest-tc6 should not be unblocked yet"
    );

    client
        .task_update(
            "sess-arch-tc6",
            Some("arch-tc6"),
            serde_json::json!({ "taskId": "src-tc6", "status": "completed" }),
        )
        .await;

    let output = client
        .task_query("sess-dev-tc6", "dev-tc6", "TaskList")
        .await;
    assert_eq!(output.exit_code, 0);
    assert!(
        output.additional_context.is_some(),
        "addBlocks inverse direction should trigger unblock notification"
    );
}
