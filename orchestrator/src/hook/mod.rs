// Hook subcommand: reads Claude Code's hook JSON from stdin, translates to internal
// protocol, forwards to daemon, translates response back to Claude Code's expected
// JSON format on stdout.
//
// This module is the translation boundary between Claude Code's hook protocol and
// the orchestrator daemon's internal protocol.

pub mod activity;
pub mod sendmessage;
pub mod session;
pub mod task;

use crate::config::Config;
use crate::protocol::{DaemonRequest, DaemonResponse, EventType, HookInput, HookOutput};
use serde_json::Value;
use std::io::{self, Read, Write};
use tokio::net::UnixStream;
use tracing::warn;

/// Entry point for the `hook` subcommand.
/// Reads Claude Code's hook JSON from stdin, translates, forwards to daemon,
/// translates response, writes to stdout/stderr, exits with correct code.
pub async fn run(config: &Config) -> anyhow::Result<()> {
    let mut raw_input = String::new();
    io::stdin().lock().read_to_string(&mut raw_input)?;

    let raw: Value = match serde_json::from_str(&raw_input) {
        Ok(v) => v,
        Err(e) => {
            warn!("hook: failed to parse stdin: {e}");
            std::process::exit(0);
        }
    };

    let hook_event_name = raw
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Translate Claude Code's JSON to our internal HookInput.
    let hook_input = translate_input(&raw, hook_event_name);

    // Auto-bootstrap: if daemon isn't running, start it.
    let output = match send_to_daemon(config, hook_input).await {
        Ok(resp) => resp.hook_output,
        Err(_first_err) => {
            // Try bootstrap, then retry once.
            if let Ok(()) = auto_bootstrap(config).await {
                let hook_input_retry = translate_input(&raw, hook_event_name);
                match send_to_daemon(config, hook_input_retry).await {
                    Ok(resp) => resp.hook_output,
                    Err(e) => {
                        warn!("hook: daemon still unreachable after bootstrap: {e}");
                        HookOutput::allow()
                    }
                }
            } else {
                HookOutput::allow()
            }
        }
    };

    // Translate our internal HookOutput to Claude Code's expected format.
    write_output(hook_event_name, &output);
    std::process::exit(output.exit_code as i32);
}

/// Translate Claude Code's raw hook JSON into our internal HookInput.
fn translate_input(raw: &Value, hook_event_name: &str) -> HookInput {
    let session_id = raw
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let event_type = match hook_event_name {
        "SessionStart" => EventType::SessionStart,
        "SessionEnd" => EventType::SessionEnd,
        "PreToolUse" => EventType::PreToolUse,
        "PostToolUse" => EventType::PostToolUse,
        "SubagentStart" => EventType::SubagentStart,
        "TeammateIdle" => EventType::TeammateIdle,
        "TaskCompleted" => EventType::TaskCompleted,
        _ => EventType::PostToolUse, // fallback: treat as activity
    };

    let tool_name = raw
        .get("tool_name")
        .and_then(|v| v.as_str())
        .map(String::from);
    let tool_input = raw.get("tool_input").cloned();

    // Derive agent_name from event-specific fields.
    let agent_name = raw
        .get("teammate_name")
        .and_then(|v| v.as_str())
        .or_else(|| raw.get("agent_id").and_then(|v| v.as_str()))
        .map(String::from);

    let agent_type = raw
        .get("agent_type")
        .and_then(|v| v.as_str())
        .map(String::from);

    // For TaskCompleted, pack task fields into tool_input so handlers can access them.
    let tool_input = if event_type == EventType::TaskCompleted {
        let mut obj = tool_input.unwrap_or(Value::Object(Default::default()));
        if let Value::Object(ref mut map) = obj {
            if let Some(task_id) = raw.get("task_id").and_then(|v| v.as_str()) {
                map.entry("taskId")
                    .or_insert(Value::String(task_id.to_string()));
            }
            if let Some(subject) = raw.get("task_subject").and_then(|v| v.as_str()) {
                map.entry("subject")
                    .or_insert(Value::String(subject.to_string()));
            }
            if let Some(desc) = raw.get("task_description").and_then(|v| v.as_str()) {
                map.entry("description")
                    .or_insert(Value::String(desc.to_string()));
            }
        }
        Some(obj)
    } else {
        tool_input
    };

    let spawner_session_id = raw
        .get("spawner_session_id")
        .and_then(|v| v.as_str())
        .map(String::from);

    HookInput {
        event_type,
        session_id,
        tool_name,
        tool_input,
        agent_name,
        agent_type,
        spawner_session_id,
    }
}

/// Write the hook output in Claude Code's expected format.
/// - Exit code 2: write stderr_message to stderr, no stdout.
/// - PreToolUse: wrap in hookSpecificOutput with hookEventName.
/// - Other events: write additionalContext if present.
fn write_output(hook_event_name: &str, output: &HookOutput) {
    if output.exit_code == 2 {
        // Blocking: write to stderr. Claude Code reads stderr on exit 2.
        if let Some(msg) = &output.stderr_message {
            let _ = io::stderr().write_all(msg.as_bytes());
        }
        return;
    }

    // Build JSON response for exit 0.
    let needs_hook_specific = hook_event_name == "PreToolUse"
        && (output.updated_input.is_some() || output.additional_context.is_some());

    if needs_hook_specific {
        let mut hso = serde_json::json!({
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
        });
        if let Some(updated) = &output.updated_input {
            hso["updatedInput"] = updated.clone();
        }
        if let Some(ctx) = &output.additional_context {
            hso["additionalContext"] = Value::String(ctx.clone());
        }
        let response = serde_json::json!({ "hookSpecificOutput": hso });
        let _ = io::stdout().write_all(response.to_string().as_bytes());
    } else if let Some(ctx) = &output.additional_context {
        // Non-PreToolUse events with context (e.g., SessionStart).
        let response = serde_json::json!({ "additionalContext": ctx });
        let _ = io::stdout().write_all(response.to_string().as_bytes());
    }
    // Otherwise: exit 0, no stdout = allow with no modifications.
}

/// Try to start the daemon. Returns Ok if socket appears within timeout.
async fn auto_bootstrap(config: &Config) -> anyhow::Result<()> {
    // Create socket directory.
    if let Some(parent) = config.socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let exe = std::env::current_exe()?;
    std::process::Command::new(&exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if config.socket_path.exists() {
            // Give the listener a moment to bind.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("daemon did not start");
        }
    }
}

/// Send a HookInput to the daemon and return the DaemonResponse.
pub async fn send_to_daemon(config: &Config, event: HookInput) -> anyhow::Result<DaemonResponse> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let req = DaemonRequest { request_id, event };
    let payload = serde_json::to_vec(&req)?;

    let mut stream = UnixStream::connect(&config.socket_path).await?;
    crate::daemon::write_framed(&mut stream, &payload).await?;
    let resp_bytes = crate::daemon::read_framed(&mut stream).await?;
    let resp: DaemonResponse = serde_json::from_slice(&resp_bytes)?;
    Ok(resp)
}
