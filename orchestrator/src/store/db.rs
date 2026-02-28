use rusqlite::{Connection, Result};
use std::path::Path;

/// Migration SQL embedded at compile time.
/// Uses include_str! so the path is resolved relative to the source file,
/// not the binary's working directory.
const MIGRATION_001: &str = include_str!("../../migrations/001_initial.sql");

/// Wrapper around a rusqlite Connection providing migration management.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) the SQLite database at `path`.
    /// Creates parent directories if they do not exist.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                    Some(format!("create_dir_all failed: {e}")),
                )
            })?;
        }
        let conn = Connection::open(path)?;
        // Enable WAL mode for better concurrency.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(Store { conn })
    }

    /// Run all embedded migrations in order.
    /// Migrations are idempotent (CREATE TABLE IF NOT EXISTS).
    pub fn run_migrations(&self) -> Result<()> {
        self.conn.execute_batch(MIGRATION_001)
    }

    /// Access the underlying connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Delete all rows from every data table while preserving the schema.
    #[allow(dead_code)]
    pub fn wipe_data(&self) -> Result<()> {
        wipe_data(&self.conn)
    }
}

/// Delete all rows from every data table while preserving the schema.
/// Ordering respects foreign key constraints: file_changes (FK → events) is deleted first,
/// events last.
pub fn wipe_data(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM file_changes;
         DELETE FROM notifications;
         DELETE FROM alerts;
         DELETE FROM task_deps;
         DELETE FROM tasks;
         DELETE FROM messages;
         DELETE FROM agents;
         DELETE FROM events;",
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn in_memory() -> Store {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .expect("pragma");
        let store = Store { conn };
        store.run_migrations().expect("migrations");
        store
    }

    #[test]
    fn migrations_are_idempotent() {
        let store = in_memory();
        // Running migrations twice must not fail.
        store.run_migrations().expect("second migration run");
    }

    #[test]
    fn wipe_data_clears_all_tables() {
        use crate::store::{agents, alerts, tasks};

        let store = in_memory();
        // Populate several tables.
        agents::upsert_agent(store.conn(), "agent-a", "sess-a", None, None).expect("upsert agent");
        tasks::upsert_task(store.conn(), "task-1", None, None, "pending", None)
            .expect("upsert task");
        alerts::insert_alert(
            store.conn(),
            "stalled_task",
            "info",
            Some("agent-a"),
            Some("task-1"),
            None,
            "desc",
        )
        .expect("insert alert");

        // Wipe all data.
        store.wipe_data().expect("wipe_data");

        let agent_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM agents", [], |r| r.get(0))
            .expect("count agents");
        let task_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .expect("count tasks");
        let alert_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM alerts", [], |r| r.get(0))
            .expect("count alerts");

        assert_eq!(agent_count, 0);
        assert_eq!(task_count, 0);
        assert_eq!(alert_count, 0);
    }

    #[test]
    fn open_creates_parent_dirs() {
        let dir = std::env::temp_dir().join("orchestrator_test_open");
        let db_path = dir.join("sub").join("test.db");
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&db_path).expect("open");
        store.run_migrations().expect("migrations");
        assert!(db_path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    // Expose in_memory for sibling test modules.
    pub fn make_store() -> Store {
        in_memory()
    }
}
