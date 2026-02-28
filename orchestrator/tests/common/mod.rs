use orchestrator::config::Config;
use orchestrator::daemon;
use orchestrator::hook;
use orchestrator::protocol::{EventType, HookInput, HookOutput};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio::task::JoinHandle;

/// A daemon instance running in a temporary directory, isolated per test.
pub struct TestDaemon {
    _temp_dir: TempDir,
    config: Config,
    task: JoinHandle<anyhow::Result<()>>,
}

#[allow(dead_code)]
impl TestDaemon {
    /// Start a new daemon with an isolated temp socket and DB.
    pub async fn start() -> Self {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config = Config {
            socket_path: temp_dir.path().join("daemon.sock"),
            db_path: temp_dir.path().join("orchestrator.db"),
            docs_dir: temp_dir.path().join("docs"),
            message_size_threshold: 2048,
            unanswered_timeout_secs: 300,
            stall_timeout_secs: 600,
            automation_interval_secs: 30,
        };

        let config_for_task = config.clone();
        let task = tokio::spawn(async move { daemon::run(&config_for_task).await });

        // Poll until socket appears (up to 5 seconds).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if config.socket_path.exists() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                panic!("test daemon did not start within 5 seconds");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        TestDaemon {
            _temp_dir: temp_dir,
            config,
            task,
        }
    }

    /// Return the socket path for TestClient connections.
    pub fn socket_path(&self) -> &Path {
        &self.config.socket_path
    }

    /// Return the config used by this daemon (for assertions on DB state).
    pub fn config(&self) -> &Config {
        &self.config
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.task.abort();
        // _temp_dir drops automatically, cleaning up socket + DB files.
    }
}

/// A client that sends requests to the daemon over its Unix socket.
///
/// Each method opens a fresh connection (matching the daemon's one-request-per-connection design).
pub struct TestClient {
    socket_path: PathBuf,
}

#[allow(dead_code)]
impl TestClient {
    pub fn new(socket_path: PathBuf) -> Self {
        TestClient { socket_path }
    }

    /// Send a HookInput event and return the HookOutput response.
    pub async fn send_event(&self, event: HookInput) -> HookOutput {
        let config = Config {
            socket_path: self.socket_path.clone(),
            ..Config::default()
        };
        hook::send_to_daemon(&config, event)
            .await
            .expect("send_event failed")
            .hook_output
    }

    pub async fn ping(&self) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::Ping,
            session_id: "test".to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: None,
            agent_type: None,
            spawner_session_id: None,
        })
        .await
    }

    pub async fn session_start(
        &self,
        session_id: &str,
        agent_name: &str,
        agent_type: Option<&str>,
    ) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::SessionStart,
            session_id: session_id.to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: Some(agent_name.to_string()),
            agent_type: agent_type.map(String::from),
            spawner_session_id: None,
        })
        .await
    }

    pub async fn session_end(&self, session_id: &str, agent_name: Option<&str>) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::SessionEnd,
            session_id: session_id.to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: agent_name.map(String::from),
            agent_type: None,
            spawner_session_id: None,
        })
        .await
    }

    pub async fn subagent_start(
        &self,
        session_id: &str,
        agent_name: &str,
        agent_type: &str,
        spawner_session_id: &str,
    ) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::SubagentStart,
            session_id: session_id.to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: Some(agent_name.to_string()),
            agent_type: Some(agent_type.to_string()),
            spawner_session_id: Some(spawner_session_id.to_string()),
        })
        .await
    }

    pub async fn teammate_idle(&self, session_id: &str, agent_name: &str) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::TeammateIdle,
            session_id: session_id.to_string(),
            tool_name: None,
            tool_input: None,
            agent_name: Some(agent_name.to_string()),
            agent_type: None,
            spawner_session_id: None,
        })
        .await
    }

    pub async fn send_message(
        &self,
        session_id: &str,
        agent_name: &str,
        recipient: &str,
        content: &str,
    ) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::PreToolUse,
            session_id: session_id.to_string(),
            tool_name: Some("SendMessage".to_string()),
            tool_input: Some(serde_json::json!({ "recipient": recipient, "content": content })),
            agent_name: Some(agent_name.to_string()),
            agent_type: None,
            spawner_session_id: None,
        })
        .await
    }

    pub async fn task_update(
        &self,
        session_id: &str,
        agent_name: Option<&str>,
        input: serde_json::Value,
    ) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::PreToolUse,
            session_id: session_id.to_string(),
            tool_name: Some("TaskUpdate".to_string()),
            tool_input: Some(input),
            agent_name: agent_name.map(String::from),
            agent_type: None,
            spawner_session_id: None,
        })
        .await
    }

    pub async fn task_query(&self, session_id: &str, agent_name: &str, tool: &str) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::PreToolUse,
            session_id: session_id.to_string(),
            tool_name: Some(tool.to_string()),
            tool_input: None,
            agent_name: Some(agent_name.to_string()),
            agent_type: None,
            spawner_session_id: None,
        })
        .await
    }

    pub async fn post_tool_use(
        &self,
        session_id: &str,
        agent_name: Option<&str>,
        tool_name: &str,
        tool_input: Option<serde_json::Value>,
    ) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::PostToolUse,
            session_id: session_id.to_string(),
            tool_name: Some(tool_name.to_string()),
            tool_input,
            agent_name: agent_name.map(String::from),
            agent_type: None,
            spawner_session_id: None,
        })
        .await
    }

    pub async fn task_completed(
        &self,
        session_id: &str,
        agent_name: &str,
        task_id: &str,
    ) -> HookOutput {
        self.send_event(HookInput {
            event_type: EventType::TaskCompleted,
            session_id: session_id.to_string(),
            tool_name: None,
            tool_input: Some(serde_json::json!({ "taskId": task_id })),
            agent_name: Some(agent_name.to_string()),
            agent_type: None,
            spawner_session_id: None,
        })
        .await
    }
}
