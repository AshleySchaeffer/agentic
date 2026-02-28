use anyhow::{Context, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::protocol::{HookInput, HookOutput};
use crate::store::{agents, events, messages};

pub fn handle_pre_send(
    conn: &Connection,
    config: &Config,
    event: &HookInput,
) -> Result<HookOutput> {
    let input = event.tool_input.as_ref();
    let recipient = input
        .and_then(|v| v.get("recipient"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let content = input
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    // Resolve sender name from session, fall back to session_id string.
    let sender_agent = agents::get_agent_by_session(conn, &event.session_id)
        .context("look up sender by session")?;
    let sender_name: &str = event
        .agent_name
        .as_deref()
        .or_else(|| sender_agent.as_ref().map(|a| a.name.as_str()))
        .unwrap_or(event.session_id.as_str());

    // Liveness check: block if recipient is dead.
    if let Some(recipient_agent) =
        agents::get_agent(conn, recipient).context("look up recipient")?
    {
        if recipient_agent.status == "dead" {
            return Ok(HookOutput::block(format!(
                "Recipient '{}' is unavailable (session ended)",
                recipient
            )));
        }
    }

    // Hash message content.
    let hash: String = Sha256::digest(content.as_bytes())
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    // Large message offloading: if content exceeds threshold, write to file and rewrite input.
    let (content_file, updated_input) = if content.len() > config.message_size_threshold {
        let file_path = config.docs_dir.join(format!("{}.md", hash));
        std::fs::create_dir_all(&config.docs_dir).context("create docs dir")?;
        if !file_path.exists() {
            std::fs::write(&file_path, content).context("write message file")?;
        }
        let file_path_str = file_path.to_string_lossy().into_owned();
        let reference = format!(
            "Full message written to {} — read this file for details.",
            file_path_str
        );
        let mut modified = event
            .tool_input
            .clone()
            .unwrap_or(serde_json::Value::Object(Default::default()));
        if let serde_json::Value::Object(ref mut map) = modified {
            map.insert("content".to_string(), serde_json::Value::String(reference));
        }
        (Some(file_path_str), Some(modified))
    } else {
        (None, None)
    };

    messages::insert_message(
        conn,
        sender_name,
        recipient,
        Some(&hash),
        content_file.as_deref(),
        "sent",
    )
    .context("insert message record")?;

    let payload = event.tool_input.as_ref().map(|v| v.to_string());
    events::insert_event(
        conn,
        "msg_send",
        &event.session_id,
        Some(sender_name),
        Some("SendMessage"),
        payload.as_deref(),
    )
    .context("insert msg_send event")?;

    Ok(HookOutput {
        exit_code: 0,
        updated_input,
        additional_context: None,
        stderr_message: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::protocol::EventType;
    use crate::store::{agents, db::tests::make_store, messages};

    fn make_send_event(
        session_id: &str,
        agent_name: Option<&str>,
        recipient: &str,
        content: &str,
    ) -> HookInput {
        HookInput {
            event_type: EventType::PreToolUse,
            session_id: session_id.to_string(),
            tool_name: Some("SendMessage".to_string()),
            tool_input: Some(serde_json::json!({
                "recipient": recipient,
                "content": content,
            })),
            agent_name: agent_name.map(String::from),
            agent_type: None,
            spawner_session_id: None,
        }
    }

    #[test]
    fn blocks_message_to_dead_recipient() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "dead-agent", "sess-dead", None, None).expect("upsert");
        agents::update_agent_status(store.conn(), "dead-agent", "dead").expect("mark dead");
        agents::upsert_agent(store.conn(), "sender", "sess-sender", None, None).expect("upsert");

        let event = make_send_event("sess-sender", Some("sender"), "dead-agent", "hello");
        let output = handle_pre_send(store.conn(), &Config::default(), &event).expect("handler");

        assert_eq!(output.exit_code, 2);
        assert!(output
            .stderr_message
            .as_deref()
            .unwrap_or("")
            .contains("dead-agent"));
    }

    #[test]
    fn allows_message_to_active_recipient() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "alive", "sess-alive", None, None).expect("upsert");
        agents::upsert_agent(store.conn(), "sender", "sess-sender", None, None).expect("upsert");

        let event = make_send_event("sess-sender", Some("sender"), "alive", "ping");
        let output = handle_pre_send(store.conn(), &Config::default(), &event).expect("handler");

        assert_eq!(output.exit_code, 0);
        assert!(output.updated_input.is_none());
        // Message should be recorded.
        let msgs = messages::get_undelivered_for(store.conn(), "alive").expect("query");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].sender, "sender");
        assert!(msgs[0].content_hash.is_some());
        assert!(msgs[0].content_file.is_none());
    }

    #[test]
    fn allows_message_to_unknown_recipient() {
        // Recipient not in DB at all — allow (they may not have started yet).
        let store = make_store();
        agents::upsert_agent(store.conn(), "sender", "sess-s", None, None).expect("upsert");

        let event = make_send_event("sess-s", Some("sender"), "new-agent", "hi");
        let output = handle_pre_send(store.conn(), &Config::default(), &event).expect("handler");
        assert_eq!(output.exit_code, 0);
    }

    #[test]
    fn sender_resolved_from_session_when_agent_name_absent() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "implicit-sender", "sess-imp", None, None)
            .expect("upsert");
        agents::upsert_agent(store.conn(), "target", "sess-t", None, None).expect("upsert");

        let event = make_send_event("sess-imp", None, "target", "hello");
        let output = handle_pre_send(store.conn(), &Config::default(), &event).expect("handler");
        assert_eq!(output.exit_code, 0);

        let msgs = messages::get_undelivered_for(store.conn(), "target").expect("query");
        assert_eq!(msgs[0].sender, "implicit-sender");
    }

    #[test]
    fn idle_recipient_is_allowed() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "idle-agent", "sess-idle", None, None).expect("upsert");
        agents::update_agent_status(store.conn(), "idle-agent", "idle").expect("mark idle");
        agents::upsert_agent(store.conn(), "sender2", "sess-s2", None, None).expect("upsert");

        let event = make_send_event("sess-s2", Some("sender2"), "idle-agent", "wake up");
        let output = handle_pre_send(store.conn(), &Config::default(), &event).expect("handler");
        assert_eq!(output.exit_code, 0);
    }

    fn tmp_docs_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("orch-sendmsg-{}", tag));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn small_message_passes_through_without_updated_input() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "rx", "sess-rx", None, None).expect("upsert");
        agents::upsert_agent(store.conn(), "tx", "sess-tx", None, None).expect("upsert");

        let config = Config {
            message_size_threshold: 100,
            ..Config::default()
        };
        let event = make_send_event("sess-tx", Some("tx"), "rx", "short msg");
        let output = handle_pre_send(store.conn(), &config, &event).expect("handler");

        assert_eq!(output.exit_code, 0);
        assert!(output.updated_input.is_none());
        let msgs = messages::get_undelivered_for(store.conn(), "rx").expect("query");
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].content_file.is_none());
    }

    #[test]
    fn large_message_offloaded_with_correct_updated_input() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "reader", "sess-r", None, None).expect("upsert");
        agents::upsert_agent(store.conn(), "writer", "sess-w", None, None).expect("upsert");

        let docs_dir = tmp_docs_dir("offload");
        let config = Config {
            message_size_threshold: 10,
            docs_dir: docs_dir.clone(),
            ..Config::default()
        };

        let big_content = "x".repeat(100);
        let event = make_send_event("sess-w", Some("writer"), "reader", &big_content);
        let output = handle_pre_send(store.conn(), &config, &event).expect("handler");

        assert_eq!(output.exit_code, 0);
        let updated = output.updated_input.expect("should have updated_input");
        let new_content = updated
            .get("content")
            .and_then(|v| v.as_str())
            .expect("content field");
        assert!(
            new_content.contains("Full message written to"),
            "reference should mention file: {new_content}"
        );
        assert!(new_content.ends_with("— read this file for details."));
        // recipient field must be preserved.
        assert_eq!(
            updated.get("recipient").and_then(|v| v.as_str()),
            Some("reader")
        );

        // DB record should have content_file set.
        let msgs = messages::get_undelivered_for(store.conn(), "reader").expect("query");
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].content_file.is_some());

        std::fs::remove_dir_all(&docs_dir).ok();
    }

    #[test]
    fn large_message_hash_deterministic_same_file() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "dest", "sess-d", None, None).expect("upsert");
        agents::upsert_agent(store.conn(), "src", "sess-s2", None, None).expect("upsert");

        let docs_dir = tmp_docs_dir("deterministic");
        let config = Config {
            message_size_threshold: 5,
            docs_dir: docs_dir.clone(),
            ..Config::default()
        };

        let content = "deterministic content payload";
        // Send the same message twice.
        let event1 = make_send_event("sess-s2", Some("src"), "dest", content);
        let out1 = handle_pre_send(store.conn(), &config, &event1).expect("first send");
        let event2 = make_send_event("sess-s2", Some("src"), "dest", content);
        let out2 = handle_pre_send(store.conn(), &config, &event2).expect("second send");

        let path1 = out1
            .updated_input
            .as_ref()
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .expect("path1");
        let path2 = out2
            .updated_input
            .as_ref()
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .expect("path2");
        assert_eq!(
            path1, path2,
            "same content must produce same file reference"
        );

        // Exactly one file should exist (second write was a no-op).
        let docs_entries: Vec<_> = std::fs::read_dir(&docs_dir).expect("read_dir").collect();
        assert_eq!(docs_entries.len(), 1);

        std::fs::remove_dir_all(&docs_dir).ok();
    }

    #[test]
    fn content_file_recorded_in_messages_table() {
        let store = make_store();
        agents::upsert_agent(store.conn(), "recip", "sess-rc", None, None).expect("upsert");
        agents::upsert_agent(store.conn(), "sendx", "sess-sx", None, None).expect("upsert");

        let docs_dir = tmp_docs_dir("cfrecord");
        let config = Config {
            message_size_threshold: 5,
            docs_dir: docs_dir.clone(),
            ..Config::default()
        };

        let event = make_send_event("sess-sx", Some("sendx"), "recip", "this is a long message");
        handle_pre_send(store.conn(), &config, &event).expect("handler");

        let msgs = messages::get_undelivered_for(store.conn(), "recip").expect("query");
        assert_eq!(msgs.len(), 1);
        let cf = msgs[0]
            .content_file
            .as_deref()
            .expect("content_file must be set");
        assert!(
            cf.ends_with(".md"),
            "content_file should be a .md path: {cf}"
        );
        // The file on disk must exist and contain the original content.
        let on_disk = std::fs::read_to_string(cf).expect("read file");
        assert_eq!(on_disk, "this is a long message");

        std::fs::remove_dir_all(&docs_dir).ok();
    }
}
