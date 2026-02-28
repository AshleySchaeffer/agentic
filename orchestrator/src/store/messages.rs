#![allow(dead_code)]

use rusqlite::{params, Connection, Result};

/// Message record as stored in the database.
#[derive(Debug, Clone)]
pub struct Message {
    pub id: i64,
    pub sender: String,
    pub recipient: String,
    pub content_hash: Option<String>,
    pub content_file: Option<String>,
    pub status: String,
    pub response_received: bool,
    pub sent_at: String,
    pub delivered_at: Option<String>,
}

/// Insert a new message record. Returns the row ID.
pub fn insert_message(
    conn: &Connection,
    sender: &str,
    recipient: &str,
    content_hash: Option<&str>,
    content_file: Option<&str>,
    status: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO messages (sender, recipient, content_hash, content_file, status)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![sender, recipient, content_hash, content_file, status],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update the status of a message (e.g. 'delivered', 'blocked').
pub fn update_message_status(conn: &Connection, id: i64, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE messages SET status = ?1 WHERE id = ?2",
        params![status, id],
    )?;
    Ok(())
}

/// Record delivery: set status to 'delivered' and stamp delivered_at.
pub fn mark_delivered(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE messages SET status = 'delivered',
                             delivered_at = strftime('%Y-%m-%dT%H:%M:%f', 'now')
         WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Record that the recipient has responded to a message.
pub fn mark_response_received(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE messages SET response_received = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Return messages sent to `recipient` that have not yet been delivered.
pub fn get_undelivered_for(conn: &Connection, recipient: &str) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, sender, recipient, content_hash, content_file, status,
                response_received, sent_at, delivered_at
         FROM messages
         WHERE recipient = ?1 AND status = 'sent'
         ORDER BY sent_at",
    )?;
    let r = collect_messages(stmt.query(params![recipient])?);
    r
}

/// Return messages that have been delivered but not responded to within `timeout_secs`.
pub fn get_unanswered(conn: &Connection, timeout_secs: u64) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, sender, recipient, content_hash, content_file, status,
                response_received, sent_at, delivered_at
         FROM messages
         WHERE status = 'delivered'
           AND response_received = 0
           AND delivered_at IS NOT NULL
           AND (strftime('%s', 'now') - strftime('%s', delivered_at)) > ?1
         ORDER BY delivered_at",
    )?;
    let r = collect_messages(stmt.query(params![timeout_secs as i64])?);
    r
}

/// Return all messages where `agent_name` is either sender or recipient.
pub fn get_messages_for_agent(conn: &Connection, agent_name: &str) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, sender, recipient, content_hash, content_file, status,
                response_received, sent_at, delivered_at
         FROM messages
         WHERE sender = ?1 OR recipient = ?1
         ORDER BY sent_at DESC",
    )?;
    let r = collect_messages(stmt.query(params![agent_name])?);
    r
}

fn collect_messages(mut rows: rusqlite::Rows<'_>) -> Result<Vec<Message>> {
    let mut msgs = Vec::new();
    while let Some(row) = rows.next()? {
        msgs.push(Message {
            id: row.get(0)?,
            sender: row.get(1)?,
            recipient: row.get(2)?,
            content_hash: row.get(3)?,
            content_file: row.get(4)?,
            status: row.get(5)?,
            response_received: row.get::<_, i32>(6)? != 0,
            sent_at: row.get(7)?,
            delivered_at: row.get(8)?,
        });
    }
    Ok(msgs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::tests::make_store;

    #[test]
    fn insert_and_query_message() {
        let store = make_store();
        let id = insert_message(
            store.conn(),
            "architect",
            "dev-1",
            Some("hash123"),
            None,
            "sent",
        )
        .expect("insert");
        assert!(id > 0);

        let undelivered = get_undelivered_for(store.conn(), "dev-1").expect("query");
        assert_eq!(undelivered.len(), 1);
        assert_eq!(undelivered[0].sender, "architect");
    }

    #[test]
    fn mark_delivered_removes_from_undelivered() {
        let store = make_store();
        let id = insert_message(store.conn(), "a", "b", None, None, "sent").expect("insert");
        mark_delivered(store.conn(), id).expect("deliver");

        let undelivered = get_undelivered_for(store.conn(), "b").expect("query");
        assert_eq!(undelivered.len(), 0);
    }

    #[test]
    fn get_messages_for_agent_covers_both_directions() {
        let store = make_store();
        insert_message(store.conn(), "x", "y", None, None, "sent").expect("insert");
        insert_message(store.conn(), "y", "z", None, None, "sent").expect("insert");

        let msgs = get_messages_for_agent(store.conn(), "y").expect("query");
        assert_eq!(msgs.len(), 2);
    }
}
