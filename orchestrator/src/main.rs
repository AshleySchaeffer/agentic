mod cli;
mod config;
mod daemon;
mod hook;
mod mcp;
mod protocol;
mod store;
mod tui;

use clap::Parser;
use cli::{Cli, Command};
use config::Config;
use protocol::{EventType, HookInput};
use serde_json::Value;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let config = Config::load();

    match cli.command {
        Command::Bootstrap => bootstrap(&config).await,
        Command::Hook => hook::run(&config).await,
        Command::Daemon => daemon::run(&config).await,
        Command::Tui => tui::run(&config),
        Command::Mcp => mcp::run(&config),
        Command::Stop => stop(&config).await,
        Command::Status => status(&config).await,
        Command::Install { project, uninstall } => install(project, uninstall),
    }
}

/// Ensure the daemon is running. If not, spawn it and wait for the socket.
async fn bootstrap(config: &Config) -> anyhow::Result<()> {
    if is_daemon_alive(config).await {
        println!("orchestrator: daemon already running");
        return Ok(());
    }

    // Spawn the daemon as a detached background process.
    let exe = std::env::current_exe()?;
    std::process::Command::new(&exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    // Poll until the socket appears (up to 5 seconds).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if is_daemon_alive(config).await {
            println!("orchestrator: daemon started");
            launch_tui_pane(&exe);
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("orchestrator: daemon did not start within 5 seconds");
        }
    }
}

/// Send a Shutdown request to the daemon.
async fn stop(config: &Config) -> anyhow::Result<()> {
    let event = HookInput {
        event_type: EventType::Shutdown,
        session_id: "cli".to_string(),
        tool_name: None,
        tool_input: None,
        agent_name: None,
        agent_type: None,
        spawner_session_id: None,
    };
    match hook::send_to_daemon(config, event).await {
        Ok(_) => println!("orchestrator: daemon stopped"),
        Err(e) => println!("orchestrator: stop failed (daemon may not be running): {e}"),
    }
    Ok(())
}

/// Ping the daemon and report whether it is alive.
async fn status(config: &Config) -> anyhow::Result<()> {
    if is_daemon_alive(config).await {
        println!("orchestrator: daemon running");
    } else {
        println!("orchestrator: daemon not running");
    }
    Ok(())
}

/// Build the hooks JSON object in Claude Code's settings format.
fn orchestrator_hooks() -> Value {
    let cmd = "orchestrator hook";

    // Sync hooks: Claude Code waits for the response
    let sync_hook = serde_json::json!([{ "type": "command", "command": cmd, "timeout": 5 }]);
    // Async hooks: fire-and-forget
    let async_hook =
        serde_json::json!([{ "type": "command", "command": cmd, "timeout": 10, "async": true }]);

    serde_json::json!({
        "SessionStart": [
            { "hooks": sync_hook }
        ],
        "PreToolUse": [
            { "matcher": "SendMessage", "hooks": sync_hook },
            { "matcher": "TaskUpdate", "hooks": sync_hook },
            { "matcher": "TaskGet|TaskList", "hooks": sync_hook }
        ],
        "PostToolUse": [
            { "matcher": ".*", "hooks": async_hook }
        ],
        "SessionEnd": [
            { "hooks": async_hook }
        ],
        "TaskCompleted": [
            { "hooks": async_hook }
        ],
        "SubagentStart": [
            { "hooks": async_hook }
        ],
        "TeammateIdle": [
            { "hooks": async_hook }
        ]
    })
}

/// Install or uninstall orchestrator hooks in Claude Code settings.
fn install(project: bool, uninstall: bool) -> anyhow::Result<()> {
    let settings_path = if project {
        std::path::PathBuf::from(".claude/settings.json")
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(home).join(".claude/settings.json")
    };

    let scope = if project { "project" } else { "user" };

    // Read existing settings or start fresh
    let mut settings: Value = if settings_path.exists() {
        let contents = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&contents)?
    } else {
        serde_json::json!({})
    };

    let settings_obj = settings
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json is not a JSON object"))?;

    if uninstall {
        // Remove orchestrator hooks by filtering out entries with "orchestrator hook" command
        if let Some(Value::Object(hooks)) = settings_obj.get_mut("hooks") {
            let event_names: Vec<String> = hooks.keys().cloned().collect();
            for event_name in event_names {
                if let Some(Value::Array(matcher_groups)) = hooks.get_mut(&event_name) {
                    matcher_groups.retain(|group| !contains_orchestrator_hook(group));
                    // Remove the event key if no matcher groups remain
                    if matcher_groups.is_empty() {
                        hooks.remove(&event_name);
                    }
                }
            }
            // Remove hooks key entirely if empty
            if hooks.is_empty() {
                settings_obj.remove("hooks");
            }
        }
        let json = serde_json::to_string_pretty(&settings)?;
        std::fs::write(&settings_path, json.as_bytes())?;
        println!(
            "Removed orchestrator hooks from {scope} settings ({}).",
            settings_path.display()
        );
    } else {
        // Merge orchestrator hooks into existing hooks
        let new_hooks = orchestrator_hooks();

        let existing_hooks = settings_obj
            .entry("hooks")
            .or_insert_with(|| serde_json::json!({}));

        let existing_obj = existing_hooks
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("hooks field is not a JSON object"))?;

        if let Value::Object(new_obj) = new_hooks {
            for (event_name, new_groups) in new_obj {
                if let Some(Value::Array(existing_groups)) = existing_obj.get_mut(&event_name) {
                    // Remove any existing orchestrator hooks for this event
                    existing_groups.retain(|group| !contains_orchestrator_hook(group));
                    // Append new ones
                    if let Value::Array(groups) = new_groups {
                        existing_groups.extend(groups);
                    }
                } else {
                    existing_obj.insert(event_name, new_groups);
                }
            }
        }

        let json = serde_json::to_string_pretty(&settings)?;
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&settings_path, json.as_bytes())?;
        println!(
            "Installed orchestrator hooks to {scope} settings ({}).",
            settings_path.display()
        );
    }

    Ok(())
}

/// Check if a matcher group contains an "orchestrator hook" command.
fn contains_orchestrator_hook(group: &Value) -> bool {
    if let Some(Value::Array(hooks)) = group.get("hooks") {
        hooks.iter().any(|h| {
            h.get("command")
                .and_then(|c| c.as_str())
                .is_some_and(|c| c.starts_with("orchestrator hook"))
        })
    } else {
        false
    }
}

/// Auto-launch the TUI in a tmux split pane, or suggest the command if not in tmux.
fn launch_tui_pane(exe: &std::path::Path) {
    if std::env::var("TMUX").is_ok() {
        let cmd = format!("{} tui", exe.display());
        let _ = std::process::Command::new("tmux")
            .args(["split-window", "-h", "-l", "40%", &cmd])
            .spawn();
    } else {
        println!(
            "orchestrator: run '{} tui' to open the dashboard",
            exe.display()
        );
    }
}

/// Try a Ping request. Returns true if the daemon responds.
async fn is_daemon_alive(config: &Config) -> bool {
    let event = HookInput {
        event_type: EventType::Ping,
        session_id: "cli".to_string(),
        tool_name: None,
        tool_input: None,
        agent_name: None,
        agent_type: None,
        spawner_session_id: None,
    };
    hook::send_to_daemon(config, event).await.is_ok()
}
