mod common;

use orchestrator::store::{messages, Store};
use serde_json::json;

#[tokio::test]
async fn send_message_to_active_agent_allowed() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client
        .session_start("sess-md1a", "md1-agent-a", Some("dev"))
        .await;
    client.session_start("sess-md1b", "md1-agent-b", None).await;

    let output = client
        .send_message("sess-md1a", "md1-agent-a", "md1-agent-b", "hello")
        .await;
    assert_eq!(output.exit_code, 0);

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let undelivered =
        messages::get_undelivered_for(store.conn(), "md1-agent-b").expect("query undelivered");
    assert_eq!(undelivered.len(), 1);
    assert_eq!(undelivered[0].sender, "md1-agent-a");
    assert_eq!(undelivered[0].status, "sent");
}

#[tokio::test]
async fn send_message_to_dead_agent_blocked() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    // Start B then end its session — session_end marks the agent dead.
    client.session_start("sess-md2b", "md2-agent-b", None).await;
    client.session_end("sess-md2b", Some("md2-agent-b")).await;

    client
        .session_start("sess-md2a", "md2-agent-a", Some("dev"))
        .await;

    let output = client
        .send_message("sess-md2a", "md2-agent-a", "md2-agent-b", "hello")
        .await;
    assert_eq!(output.exit_code, 2);
    assert!(output
        .stderr_message
        .as_deref()
        .unwrap_or("")
        .contains("md2-agent-b"));
}

#[tokio::test]
async fn send_message_to_unknown_agent_allowed() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client.session_start("sess-md3a", "md3-agent-a", None).await;

    // Recipient not in DB at all — allowed because it may not have started yet.
    let output = client
        .send_message("sess-md3a", "md3-agent-a", "nonexistent-md3", "hi")
        .await;
    assert_eq!(output.exit_code, 0);
}

#[tokio::test]
async fn message_delivery_tracking_via_post_tool_use() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client.session_start("sess-md4a", "md4-agent-a", None).await;
    client.session_start("sess-md4b", "md4-agent-b", None).await;

    // PreToolUse: send message → status "sent".
    let send_out = client
        .send_message("sess-md4a", "md4-agent-a", "md4-agent-b", "deliver me")
        .await;
    assert_eq!(send_out.exit_code, 0);

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let undelivered =
        messages::get_undelivered_for(store.conn(), "md4-agent-b").expect("query undelivered");
    assert_eq!(undelivered.len(), 1, "message should be in sent state");

    // PostToolUse: SendMessage completion → status transitions to "delivered".
    let post_out = client
        .post_tool_use(
            "sess-md4a",
            Some("md4-agent-a"),
            "SendMessage",
            Some(json!({"recipient": "md4-agent-b"})),
        )
        .await;
    assert_eq!(post_out.exit_code, 0);

    // Re-open to read past any connection caching.
    let store2 = Store::open(&daemon.config().db_path).expect("open store 2");
    let all_msgs =
        messages::get_messages_for_agent(store2.conn(), "md4-agent-b").expect("query all");
    let delivered = all_msgs.iter().find(|m| m.status == "delivered");
    assert!(
        delivered.is_some(),
        "message should be delivered after PostToolUse"
    );

    // Undelivered queue must now be empty.
    let still_undelivered =
        messages::get_undelivered_for(store2.conn(), "md4-agent-b").expect("query undelivered 2");
    assert_eq!(still_undelivered.len(), 0);
}

#[tokio::test]
async fn post_tool_use_records_file_change() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client.session_start("sess-md5a", "md5-agent-a", None).await;

    let out = client
        .post_tool_use(
            "sess-md5a",
            Some("md5-agent-a"),
            "Write",
            Some(json!({"file_path": "/test/md5/file.rs"})),
        )
        .await;
    assert_eq!(out.exit_code, 0);

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM file_changes WHERE agent_name = ?1 AND file_path = ?2",
            rusqlite::params!["md5-agent-a", "/test/md5/file.rs"],
            |row| row.get(0),
        )
        .expect("query file_changes");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn send_to_idle_agent_allowed() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    // Start B then mark it idle — idle is not dead.
    client.session_start("sess-md6b", "md6-agent-b", None).await;
    client.teammate_idle("sess-md6b", "md6-agent-b").await;

    client.session_start("sess-md6a", "md6-agent-a", None).await;

    let output = client
        .send_message("sess-md6a", "md6-agent-a", "md6-agent-b", "wake up")
        .await;
    assert_eq!(output.exit_code, 0);
}
