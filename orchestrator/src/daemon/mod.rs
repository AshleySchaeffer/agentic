// Daemon: long-lived Unix socket listener.
//
// Protocol (shared with hook handler):
//   Each message is a 4-byte big-endian length prefix followed by that many UTF-8 JSON bytes.
//   Request:  DaemonRequest  (hook handler → daemon)
//   Response: DaemonResponse (daemon → hook handler)
//
// On startup the daemon:
//   1. Creates the socket directory.
//   2. Opens the SQLite store and runs migrations.
//   3. Starts accepting connections until SIGTERM or a Shutdown request arrives.
//
// On shutdown the daemon removes the socket file.

pub mod alerts;
pub mod automation;
pub mod notifications;

use crate::config::Config;
use crate::hook::{activity, sendmessage, session, task};
use crate::protocol::{DaemonRequest, DaemonResponse, EventType, HookInput, HookOutput};
use crate::store::events::insert_event;
use crate::store::Store;
use rusqlite::Connection;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// Run the daemon: open store, bind socket, accept connections.
pub async fn run(config: &Config) -> anyhow::Result<()> {
    // Create socket directory.
    if let Some(parent) = config.socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Remove stale socket file if present.
    let _ = tokio::fs::remove_file(&config.socket_path).await;

    // Open database.
    let store = Store::open(&config.db_path)?;
    store.run_migrations()?;
    let store = Arc::new(Mutex::new(store));
    let config = Arc::new(config.clone());

    let listener = UnixListener::bind(&config.socket_path)?;
    info!("daemon listening on {:?}", config.socket_path);

    // Shutdown signal.
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));

    let mut automation_timer = tokio::time::interval(std::time::Duration::from_secs(
        config.automation_interval_secs,
    ));

    loop {
        tokio::select! {
            _ = automation_timer.tick() => {
                let store = store.lock().await;
                if let Err(e) = automation::run_automation_tick(store.conn(), &config) {
                    warn!("automation tick error: {e}");
                }
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _addr)) => {
                        let store = Arc::clone(&store);
                        let shutdown_tx = Arc::clone(&shutdown_tx);
                        let config = Arc::clone(&config);
                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_connection(stream, store, shutdown_tx, config).await
                            {
                                warn!("connection error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        error!("accept error: {e}");
                    }
                }
            }
            _ = &mut shutdown_rx => {
                info!("daemon shutting down");
                break;
            }
            _ = sigterm() => {
                info!("daemon received SIGTERM, shutting down");
                break;
            }
        }
    }

    let _ = tokio::fs::remove_file(&config.socket_path).await;
    info!("daemon stopped");
    Ok(())
}

/// Handle a single connection: read one request, produce one response.
async fn handle_connection(
    mut stream: UnixStream,
    store: Arc<Mutex<Store>>,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    config: Arc<Config>,
) -> anyhow::Result<()> {
    let request = read_framed(&mut stream).await?;
    let req: DaemonRequest = serde_json::from_slice(&request)?;

    let hook_output = if req.event.event_type == EventType::Shutdown {
        // Trigger graceful shutdown.
        if let Some(tx) = shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }
        HookOutput::allow()
    } else if req.event.event_type == EventType::Ping {
        HookOutput::allow()
    } else {
        let store = store.lock().await;
        match dispatch(store.conn(), config.as_ref(), &req.event) {
            Ok(output) => output,
            Err(e) => {
                warn!("dispatch error: {e}");
                HookOutput::allow()
            }
        }
    };

    let response = DaemonResponse {
        request_id: req.request_id,
        hook_output,
    };
    let response_bytes = serde_json::to_vec(&response)?;
    write_framed(&mut stream, &response_bytes).await?;
    Ok(())
}

/// Route a hook event to the correct handler based on event_type and tool_name.
/// Falls back to a plain event insert for unhandled combinations.
pub fn dispatch(
    conn: &Connection,
    config: &Config,
    event: &HookInput,
) -> anyhow::Result<HookOutput> {
    let tool = event.tool_name.as_deref();
    match (&event.event_type, tool) {
        (EventType::SessionStart, _) => {
            // Fresh team detection: when a top-level agent (no spawner) starts and the previous
            // team has explicitly completed (team_completed alert present + all agents dead),
            // wipe prior data so the TUI starts fresh.
            if event.spawner_session_id.is_none() {
                let active: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM agents WHERE status != 'dead'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(anyhow::Error::from)?;
                let prior_team_done =
                    alerts::alert_exists(conn, "team_completed", None, None, None)?;
                if active == 0 && prior_team_done {
                    crate::store::db::wipe_data(conn)?;
                }
            }
            session::handle_session_start(conn, event)
        }
        (EventType::SessionEnd, _) => session::handle_session_end(conn, event),
        (EventType::SubagentStart, _) => session::handle_subagent_start(conn, event),
        (EventType::TeammateIdle, _) => session::handle_teammate_idle(conn, event),
        (EventType::PreToolUse, Some("SendMessage")) => {
            sendmessage::handle_pre_send(conn, config, event)
        }
        (EventType::PreToolUse, Some("TaskUpdate")) => task::handle_pre_task_update(conn, event),
        (EventType::PreToolUse, Some("TaskGet" | "TaskList")) => {
            task::handle_pre_task_query(conn, event)
        }
        (EventType::PostToolUse, _) => activity::handle_post_tool_use(conn, event),
        (EventType::TaskCompleted, _) => task::handle_task_completed(conn, event),
        _ => {
            let payload = event.tool_input.as_ref().map(|v| v.to_string());
            insert_event(
                conn,
                &format!("{:?}", event.event_type),
                &event.session_id,
                event.agent_name.as_deref(),
                event.tool_name.as_deref(),
                payload.as_deref(),
            )?;
            Ok(HookOutput::allow())
        }
    }
}

/// Read a length-prefixed message: 4-byte BE length then that many bytes.
pub async fn read_framed(stream: &mut UnixStream) -> anyhow::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Write a length-prefixed message: 4-byte BE length then the bytes.
pub async fn write_framed(stream: &mut UnixStream, data: &[u8]) -> anyhow::Result<()> {
    let len = data.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(data).await?;
    stream.flush().await?;
    Ok(())
}

/// Future that completes on SIGTERM (Unix only).
#[cfg(unix)]
async fn sigterm() {
    use tokio::signal::unix::{signal, SignalKind};
    if let Ok(mut sig) = signal(SignalKind::terminate()) {
        sig.recv().await;
    } else {
        std::future::pending::<()>().await;
    }
}

#[cfg(not(unix))]
async fn sigterm() {
    std::future::pending::<()>().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::protocol::EventType;
    use crate::store::{agents, db::tests::make_store, notifications};

    fn make_event(
        event_type: EventType,
        session_id: &str,
        tool_name: Option<&str>,
        agent_name: Option<&str>,
        tool_input: Option<serde_json::Value>,
    ) -> HookInput {
        HookInput {
            event_type,
            session_id: session_id.to_string(),
            tool_name: tool_name.map(String::from),
            tool_input,
            agent_name: agent_name.map(String::from),
            agent_type: None,
            spawner_session_id: None,
        }
    }

    #[test]
    fn dispatch_session_start_creates_agent() {
        let store = make_store();
        let event = make_event(
            EventType::SessionStart,
            "sess-1",
            None,
            Some("architect"),
            None,
        );
        let output = dispatch(store.conn(), &Config::default(), &event).expect("dispatch");
        assert_eq!(output.exit_code, 0);

        let agent = agents::get_agent(store.conn(), "architect")
            .expect("query")
            .expect("agent exists");
        assert_eq!(agent.status, "active");
        assert_eq!(agent.session_id, "sess-1");
    }

    #[test]
    fn dispatch_session_start_wipes_data_for_new_team() {
        use crate::store::{alerts as store_alerts, tasks};

        let store = make_store();
        // Simulate a previous team: an agent (now dead), a completed task, and the
        // team_completed alert that detect_team_completion() would have created.
        agents::upsert_agent(store.conn(), "old-agent", "sess-old", None, None).expect("upsert");
        agents::update_agent_status(store.conn(), "old-agent", "dead").expect("mark dead");
        tasks::upsert_task(store.conn(), "old-task", None, None, "completed", None)
            .expect("upsert task");
        store_alerts::insert_alert(
            store.conn(),
            "team_completed",
            "info",
            None,
            None,
            None,
            "All tasks completed and all agents have exited",
        )
        .expect("insert team_completed alert");

        // New top-level agent arrives (no spawner_session_id).
        let event = HookInput {
            event_type: EventType::SessionStart,
            session_id: "sess-new".to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: Some("new-architect".to_string()),
            agent_type: Some("architect".to_string()),
            spawner_session_id: None,
        };
        dispatch(store.conn(), &Config::default(), &event).expect("dispatch");

        // Old data must be gone.
        let task_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .expect("count tasks");
        assert_eq!(task_count, 0, "old task should be wiped");

        let old_agent = agents::get_agent(store.conn(), "old-agent").expect("query");
        assert!(old_agent.is_none(), "old agent should be wiped");

        // New agent must have been created.
        let new_agent = agents::get_agent(store.conn(), "new-architect")
            .expect("query")
            .expect("new agent exists");
        assert_eq!(new_agent.status, "active");
        assert_eq!(new_agent.session_id, "sess-new");
    }

    #[test]
    fn dispatch_session_start_no_wipe_when_active_agents_exist() {
        use crate::store::tasks;

        let store = make_store();
        // An active agent is still running.
        agents::upsert_agent(store.conn(), "active-agent", "sess-a", None, None).expect("upsert");
        tasks::upsert_task(store.conn(), "live-task", None, None, "in_progress", None)
            .expect("upsert task");

        // New top-level agent arrives, but the active agent is still alive.
        let event = HookInput {
            event_type: EventType::SessionStart,
            session_id: "sess-b".to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: Some("second-agent".to_string()),
            agent_type: None,
            spawner_session_id: None,
        };
        dispatch(store.conn(), &Config::default(), &event).expect("dispatch");

        // Existing data must NOT be wiped.
        let task_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .expect("count tasks");
        assert_eq!(task_count, 1, "live task must not be wiped");
    }

    #[test]
    fn dispatch_subagent_start_no_wipe() {
        use crate::store::tasks;

        let store = make_store();
        agents::upsert_agent(store.conn(), "parent", "sess-p", None, None).expect("upsert");
        tasks::upsert_task(store.conn(), "parent-task", None, None, "in_progress", None)
            .expect("upsert task");

        // Subagent has a spawner — must never trigger a wipe.
        let event = HookInput {
            event_type: EventType::SubagentStart,
            session_id: "sess-sub".to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: Some("sub-worker".to_string()),
            agent_type: Some("dev".to_string()),
            spawner_session_id: Some("sess-p".to_string()),
        };
        dispatch(store.conn(), &Config::default(), &event).expect("dispatch");

        let task_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .expect("count tasks");
        assert_eq!(task_count, 1, "subagent start must not wipe");
    }

    #[test]
    fn dispatch_session_end_marks_agent_dead() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "dev-x", "sess-x", None, None).expect("upsert");

        let event = make_event(EventType::SessionEnd, "sess-x", None, Some("dev-x"), None);
        dispatch(store.conn(), &Config::default(), &event).expect("dispatch");

        let agent = agents::get_agent(store.conn(), "dev-x")
            .expect("query")
            .expect("exists");
        assert_eq!(agent.status, "dead");
    }

    #[test]
    fn dispatch_sendmessage_blocks_dead_recipient() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "dead-one", "sess-d", None, None).expect("upsert");
        agents::update_agent_status(store.conn(), "dead-one", "dead").expect("mark dead");
        agents::upsert_agent(store.conn(), "sender", "sess-s", None, None).expect("upsert");

        let input = serde_json::json!({ "recipient": "dead-one", "content": "hello" });
        let event = make_event(
            EventType::PreToolUse,
            "sess-s",
            Some("SendMessage"),
            Some("sender"),
            Some(input),
        );
        let output = dispatch(store.conn(), &Config::default(), &event).expect("dispatch");

        assert_eq!(output.exit_code, 2);
        assert!(output
            .stderr_message
            .as_deref()
            .unwrap_or("")
            .contains("dead-one"));
    }

    #[test]
    fn dispatch_task_list_drains_notifications() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "worker", "sess-w", None, None).expect("upsert");
        notifications::insert_notification(store.conn(), "worker", 0, "task X is unblocked")
            .expect("insert");

        let event = make_event(
            EventType::PreToolUse,
            "sess-w",
            Some("TaskList"),
            Some("worker"),
            None,
        );
        let output = dispatch(store.conn(), &Config::default(), &event).expect("dispatch");

        assert_eq!(output.exit_code, 0);
        assert_eq!(
            output.additional_context.as_deref(),
            Some("task X is unblocked")
        );

        // Notification now marked delivered — second query returns nothing.
        let event2 = make_event(
            EventType::PreToolUse,
            "sess-w",
            Some("TaskList"),
            Some("worker"),
            None,
        );
        let output2 = dispatch(store.conn(), &Config::default(), &event2).expect("dispatch2");
        assert!(output2.additional_context.is_none());
    }

    #[test]
    fn dispatch_subagent_start_creates_agent_with_spawner() {
        let store = make_store();
        let event = HookInput {
            event_type: EventType::SubagentStart,
            session_id: "sess-sub".to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: Some("sub-worker".to_string()),
            agent_type: Some("dev".to_string()),
            spawner_session_id: Some("sess-parent".to_string()),
        };
        dispatch(store.conn(), &Config::default(), &event).expect("dispatch");

        let agent = agents::get_agent(store.conn(), "sub-worker")
            .expect("query")
            .expect("exists");
        assert_eq!(agent.spawner_session_id.as_deref(), Some("sess-parent"));
    }

    #[test]
    fn dispatch_teammate_idle_marks_idle() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "idler", "sess-i", None, None).expect("upsert");

        let event = make_event(EventType::TeammateIdle, "sess-i", None, Some("idler"), None);
        dispatch(store.conn(), &Config::default(), &event).expect("dispatch");

        let agent = agents::get_agent(store.conn(), "idler")
            .expect("query")
            .expect("exists");
        assert_eq!(agent.status, "idle");
    }

    #[test]
    fn dispatch_unhandled_event_inserts_event_and_allows() {
        let store = make_store();
        // PreToolUse with an unhandled tool name — falls through to generic insert.
        let event = make_event(
            EventType::PreToolUse,
            "sess-misc",
            Some("Bash"),
            Some("agent-misc"),
            None,
        );
        let output = dispatch(store.conn(), &Config::default(), &event).expect("dispatch");
        assert_eq!(output.exit_code, 0);

        let count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE tool_name = 'Bash'",
                [],
                |row| row.get(0),
            )
            .expect("query");
        assert_eq!(count, 1);
    }
}
