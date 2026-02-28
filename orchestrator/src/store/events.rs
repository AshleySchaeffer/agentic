use rusqlite::{params, Connection, Result};

/// Insert an event into the append-only event log.
/// Returns the row ID of the newly inserted event.
pub fn insert_event(
    conn: &Connection,
    event_type: &str,
    session_id: &str,
    agent_name: Option<&str>,
    tool_name: Option<&str>,
    payload_json: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO events (event_type, session_id, agent_name, tool_name, payload)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![event_type, session_id, agent_name, tool_name, payload_json],
    )?;
    Ok(conn.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::tests::make_store;

    #[test]
    fn insert_and_query_event() {
        let store = make_store();
        let id = insert_event(
            store.conn(),
            "PreToolUse",
            "sess-123",
            Some("agent-a"),
            Some("SendMessage"),
            Some(r#"{"key":"val"}"#),
        )
        .expect("insert");
        assert!(id > 0);

        let (etype, sess): (String, String) = store
            .conn()
            .query_row(
                "SELECT event_type, session_id FROM events WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query");
        assert_eq!(etype, "PreToolUse");
        assert_eq!(sess, "sess-123");
    }

    #[test]
    fn insert_event_minimal_fields() {
        let store = make_store();
        let id = insert_event(store.conn(), "SessionStart", "sess-456", None, None, None)
            .expect("insert");
        assert!(id > 0);
    }
}
