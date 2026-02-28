#![allow(dead_code)]

use anyhow::Result;
use rusqlite::{params, Connection};

/// Return the number of undelivered notifications queued for `agent_name`.
pub fn get_pending_count(conn: &Connection, agent_name: &str) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notifications WHERE target_agent = ?1 AND delivered_at IS NULL",
        params![agent_name],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

/// Delete delivered notifications older than `max_age_secs` seconds. Returns the number deleted.
pub fn cleanup_old_notifications(conn: &Connection, max_age_secs: u64) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM notifications
         WHERE delivered_at IS NOT NULL
           AND (strftime('%s', 'now') - strftime('%s', delivered_at)) > ?1",
        params![max_age_secs as i64],
    )?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{db::tests::make_store, notifications::insert_notification};

    #[test]
    fn get_pending_count_counts_undelivered_only() {
        let store = make_store();
        insert_notification(store.conn(), "agent-a", 1, "msg1").unwrap();
        insert_notification(store.conn(), "agent-a", 1, "msg2").unwrap();
        // Deliver one of the notifications.
        store
            .conn()
            .execute(
                "UPDATE notifications
                 SET delivered_at = strftime('%Y-%m-%dT%H:%M:%f', 'now')
                 WHERE id = 1",
                [],
            )
            .unwrap();
        assert_eq!(get_pending_count(store.conn(), "agent-a").unwrap(), 1);
    }

    #[test]
    fn cleanup_removes_old_delivered_notifications() {
        let store = make_store();
        insert_notification(store.conn(), "agent-b", 1, "old").unwrap();
        // Stamp a very old delivered_at.
        store
            .conn()
            .execute(
                "UPDATE notifications SET delivered_at = '2020-01-01T00:00:00.000' WHERE id = 1",
                [],
            )
            .unwrap();
        // A second notification delivered recently — must survive cleanup.
        insert_notification(store.conn(), "agent-b", 1, "recent").unwrap();
        store
            .conn()
            .execute(
                "UPDATE notifications
                 SET delivered_at = strftime('%Y-%m-%dT%H:%M:%f', 'now')
                 WHERE id = 2",
                [],
            )
            .unwrap();

        let removed = cleanup_old_notifications(store.conn(), 3600).unwrap();
        assert_eq!(removed, 1);
    }

    #[test]
    fn cleanup_does_not_remove_undelivered() {
        let store = make_store();
        insert_notification(store.conn(), "agent-c", 1, "pending").unwrap();
        // max_age_secs = 0 means "everything older than now", but undelivered must be immune.
        let removed = cleanup_old_notifications(store.conn(), 0).unwrap();
        assert_eq!(removed, 0);
    }
}
