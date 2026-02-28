use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::protocol::{HookInput, HookOutput};
use crate::store::{agents, events};

pub fn handle_post_tool_use(conn: &Connection, event: &HookInput) -> Result<HookOutput> {
    let tool_name = event.tool_name.as_deref().unwrap_or("unknown");

    // Resolve agent name: prefer explicit field, fall back to session lookup.
    let agent_by_session = agents::get_agent_by_session(conn, &event.session_id)
        .context("look up agent by session")?;
    let agent_name: &str = event
        .agent_name
        .as_deref()
        .or_else(|| agent_by_session.as_ref().map(|a| a.name.as_str()))
        .unwrap_or("unknown");

    let payload = event.tool_input.as_ref().map(|v| v.to_string());
    events::insert_event(
        conn,
        "tool_activity",
        &event.session_id,
        Some(agent_name),
        Some(tool_name),
        payload.as_deref(),
    )
    .context("insert tool_activity event")?;

    agents::update_agent_activity(conn, agent_name).context("update agent last_activity")?;

    match tool_name {
        "SendMessage" => {
            let recipient = event
                .tool_input
                .as_ref()
                .and_then(|v| v.get("recipient"))
                .and_then(|v| v.as_str());
            if let Some(recip) = recipient {
                conn.execute(
                    "UPDATE messages
                     SET status = 'delivered',
                         delivered_at = strftime('%Y-%m-%dT%H:%M:%f', 'now')
                     WHERE id = (
                         SELECT id FROM messages
                         WHERE sender = ?1 AND recipient = ?2 AND status = 'sent'
                         ORDER BY sent_at DESC LIMIT 1
                     )",
                    rusqlite::params![agent_name, recip],
                )
                .context("mark message delivered")?;

                events::insert_event(
                    conn,
                    "msg_deliver",
                    &event.session_id,
                    Some(agent_name),
                    Some("SendMessage"),
                    None,
                )
                .context("insert msg_deliver event")?;
            }
        }
        "Write" | "Edit" => {
            let file_path = event
                .tool_input
                .as_ref()
                .and_then(|v| v.get("file_path"))
                .and_then(|v| v.as_str());
            if let Some(fp) = file_path {
                let event_id = events::insert_event(
                    conn,
                    "file_change",
                    &event.session_id,
                    Some(agent_name),
                    Some(tool_name),
                    Some(fp),
                )
                .context("insert file_change event")?;

                conn.execute(
                    "INSERT INTO file_changes (agent_name, file_path, event_id) VALUES (?1, ?2, ?3)",
                    rusqlite::params![agent_name, fp, event_id],
                )
                .context("insert file_changes row")?;
            }
        }
        _ => {}
    }

    Ok(HookOutput::allow())
}
