pub mod recover;

use crate::config::Config;
use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

pub fn run(config: &Config) -> Result<()> {
    let conn = Connection::open_with_flags(
        &config.db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                writeln!(
                    out,
                    "{}",
                    json!({"jsonrpc": "2.0", "id": null,
                           "error": {"code": -32700, "message": "Parse error"}})
                )?;
                continue;
            }
        };

        if let Some(response) = handle_request(conn.as_ref(), &request) {
            writeln!(out, "{response}")?;
        }
    }

    Ok(())
}

fn handle_request(conn: Option<&Connection>, req: &Value) -> Option<Value> {
    // Notifications have no id field — no response needed.
    let id = req.get("id")?.clone();

    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    Some(match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": {"name": "orchestrator", "version": "0.1.0"},
                "capabilities": {"tools": {}}
            }
        }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {"tools": [recover::tool_definition()]}
        }),
        "tools/call" => {
            let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let tool_args = params.get("arguments").cloned().unwrap_or(Value::Null);
            match tool_name {
                "orchestrator:recover" => {
                    let content = match conn {
                        Some(c) => recover::call(c, tool_args),
                        None => Err(anyhow::anyhow!("database not available")),
                    };
                    match content {
                        Ok(text) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {"content": [{"type": "text", "text": text}]}
                        }),
                        Err(e) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [{"type": "text", "text": format!("Error: {e}")}],
                                "isError": true
                            }
                        }),
                    }
                }
                _ => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("Unknown tool: {tool_name}")}
                }),
            }
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {method}")}
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_returns_none() {
        let req = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle_request(None, &req).is_none());
    }

    #[test]
    fn initialize_returns_server_info() {
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let resp = handle_request(None, &req).unwrap();
        assert_eq!(resp["result"]["serverInfo"]["name"], "orchestrator");
        assert_eq!(resp["result"]["serverInfo"]["version"], "0.1.0");
        assert_eq!(resp["id"], 1);
    }

    #[test]
    fn tools_list_returns_correct_definition() {
        let req = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}});
        let resp = handle_request(None, &req).unwrap();
        let tools = &resp["result"]["tools"];
        assert!(tools.is_array());
        assert_eq!(tools[0]["name"], "orchestrator:recover");
        assert!(tools[0]["inputSchema"]["properties"]["session_id"].is_object());
        assert_eq!(resp["id"], 2);
    }

    #[test]
    fn unknown_method_returns_error() {
        let req = json!({"jsonrpc": "2.0", "id": 3, "method": "unknown/method", "params": {}});
        let resp = handle_request(None, &req).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
        assert_eq!(resp["id"], 3);
    }

    #[test]
    fn tools_call_no_db_returns_error_result() {
        let req = json!({
            "jsonrpc": "2.0", "id": 4,
            "method": "tools/call",
            "params": {"name": "orchestrator:recover", "arguments": {"session_id": "x"}}
        });
        let resp = handle_request(None, &req).unwrap();
        assert_eq!(resp["result"]["isError"], true);
        assert_eq!(resp["id"], 4);
    }

    #[test]
    fn tools_call_unknown_tool_returns_error() {
        let req = json!({
            "jsonrpc": "2.0", "id": 5,
            "method": "tools/call",
            "params": {"name": "unknown:tool", "arguments": {}}
        });
        let resp = handle_request(None, &req).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
        assert_eq!(resp["id"], 5);
    }

    #[test]
    fn string_id_is_echoed() {
        let req = json!({"jsonrpc": "2.0", "id": "req-abc", "method": "tools/list", "params": {}});
        let resp = handle_request(None, &req).unwrap();
        assert_eq!(resp["id"], "req-abc");
    }
}
