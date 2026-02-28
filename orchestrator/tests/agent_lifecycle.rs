mod common;

use orchestrator::store::agents;
use orchestrator::store::Store;

#[tokio::test]
async fn agent_full_lifecycle() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    let out = client
        .session_start("sess-lc-1", "agent-lc-1", Some("dev"))
        .await;
    assert_eq!(out.exit_code, 0);

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let agent = agents::get_agent(store.conn(), "agent-lc-1")
        .expect("query")
        .expect("agent exists after session_start");
    assert_eq!(agent.status, "active");
    assert_eq!(agent.session_id, "sess-lc-1");

    let out = client.teammate_idle("sess-lc-1", "agent-lc-1").await;
    assert_eq!(out.exit_code, 0);

    let agent = agents::get_agent(store.conn(), "agent-lc-1")
        .expect("query")
        .expect("agent exists after teammate_idle");
    assert_eq!(agent.status, "idle");

    let out = client.session_end("sess-lc-1", Some("agent-lc-1")).await;
    assert_eq!(out.exit_code, 0);

    let agent = agents::get_agent(store.conn(), "agent-lc-1")
        .expect("query")
        .expect("agent exists after session_end");
    assert_eq!(agent.status, "dead");
}

#[tokio::test]
async fn subagent_records_spawner_and_type() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    let out = client
        .subagent_start("sess-sub-1", "subagent-1", "dev", "sess-parent-1")
        .await;
    assert_eq!(out.exit_code, 0);

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let agent = agents::get_agent(store.conn(), "subagent-1")
        .expect("query")
        .expect("subagent exists");
    assert_eq!(agent.status, "active");
    assert_eq!(agent.agent_type.as_deref(), Some("dev"));
    assert_eq!(agent.spawner_session_id.as_deref(), Some("sess-parent-1"));
}

#[tokio::test]
async fn session_end_with_undelivered_message_creates_alert() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client.session_start("sess-al-a", "agent-al-a", None).await;
    client.session_start("sess-al-b", "agent-al-b", None).await;

    // A sends a message to B that is never delivered.
    let out = client
        .send_message("sess-al-a", "agent-al-a", "agent-al-b", "hello")
        .await;
    assert_eq!(out.exit_code, 0);

    // B dies while the message is still undelivered.
    let out = client.session_end("sess-al-b", Some("agent-al-b")).await;
    assert_eq!(out.exit_code, 0);

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM alerts WHERE kind = 'dead_agent' AND agent_name = 'agent-al-b'",
            [],
            |row| row.get(0),
        )
        .expect("query alerts");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn session_end_with_in_progress_task_creates_alert() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client
        .session_start("sess-ip-1", "agent-ip-1", Some("dev"))
        .await;

    client
        .task_update(
            "sess-ip-1",
            Some("agent-ip-1"),
            serde_json::json!({
                "taskId": "task-ip-1",
                "status": "in_progress",
                "owner": "agent-ip-1",
            }),
        )
        .await;

    let out = client.session_end("sess-ip-1", Some("agent-ip-1")).await;
    assert_eq!(out.exit_code, 0);

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM alerts \
             WHERE kind = 'dead_agent' AND task_id = 'task-ip-1' AND severity = 'critical'",
            [],
            |row| row.get(0),
        )
        .expect("query alerts");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn session_start_reactivates_agent() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client.session_start("sess-ra-1", "agent-ra", None).await;
    client.session_end("sess-ra-1", Some("agent-ra")).await;

    let store = Store::open(&daemon.config().db_path).expect("open store");
    let agent = agents::get_agent(store.conn(), "agent-ra")
        .expect("query")
        .expect("agent exists after first session_end");
    assert_eq!(agent.status, "dead");

    client.session_start("sess-ra-2", "agent-ra", None).await;

    let agent = agents::get_agent(store.conn(), "agent-ra")
        .expect("query")
        .expect("agent exists after reactivation");
    assert_eq!(agent.status, "active");
    assert_eq!(agent.session_id, "sess-ra-2");
}

#[tokio::test]
async fn multiple_agents_independent_lifecycles() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());

    client.session_start("sess-mi-1", "agent-mi-1", None).await;
    client.session_start("sess-mi-2", "agent-mi-2", None).await;
    client.session_start("sess-mi-3", "agent-mi-3", None).await;

    client.teammate_idle("sess-mi-2", "agent-mi-2").await;
    client.session_end("sess-mi-3", Some("agent-mi-3")).await;

    let store = Store::open(&daemon.config().db_path).expect("open store");

    let a1 = agents::get_agent(store.conn(), "agent-mi-1")
        .expect("query")
        .expect("agent-mi-1 exists");
    assert_eq!(a1.status, "active");

    let a2 = agents::get_agent(store.conn(), "agent-mi-2")
        .expect("query")
        .expect("agent-mi-2 exists");
    assert_eq!(a2.status, "idle");

    let a3 = agents::get_agent(store.conn(), "agent-mi-3")
        .expect("query")
        .expect("agent-mi-3 exists");
    assert_eq!(a3.status, "dead");
}
