#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Event types that Claude Code sends via hooks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum EventType {
    SessionStart,
    SessionEnd,
    PreToolUse,
    PostToolUse,
    SubagentStart,
    TeammateIdle,
    TaskCompleted,
    /// Internal: request daemon shutdown.
    Shutdown,
    /// Internal: ping to check daemon liveness.
    Ping,
}

/// The JSON Claude Code sends to hook handlers on stdin.
/// Claude Code uses camelCase field names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookInput {
    pub event_type: EventType,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawner_session_id: Option<String>,
}

/// The JSON the hook handler writes to stdout.
/// exit_code: 0 = allow, 1 = block with message, 2 = block with stderr.
/// Claude Code reads camelCase field names from hook stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutput {
    pub exit_code: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_message: Option<String>,
}

impl HookOutput {
    /// Passthrough: allow the tool call with no modifications.
    pub fn allow() -> Self {
        HookOutput {
            exit_code: 0,
            updated_input: None,
            additional_context: None,
            stderr_message: None,
        }
    }

    /// Block the tool call, reporting a message to the agent.
    pub fn block(message: impl Into<String>) -> Self {
        HookOutput {
            exit_code: 2,
            updated_input: None,
            additional_context: None,
            stderr_message: Some(message.into()),
        }
    }
}

/// Wrapper sent from the hook handler to the daemon over the Unix socket.
/// Protocol: 4-byte big-endian length prefix followed by UTF-8 JSON bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub request_id: String,
    pub event: HookInput,
}

/// Response from the daemon back to the hook handler.
/// Mirrors the request_id for correlation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub request_id: String,
    pub hook_output: HookOutput,
}
