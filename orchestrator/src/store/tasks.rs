#![allow(dead_code)]

use rusqlite::{params, Connection, Result};

/// Task record as stored in the database.
#[derive(Debug, Clone)]
pub struct Task {
    pub task_id: String,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub status: String,
    pub owner: Option<String>,
    pub updated_at: String,
}

/// Insert or update a task.
pub fn upsert_task(
    conn: &Connection,
    task_id: &str,
    subject: Option<&str>,
    description: Option<&str>,
    status: &str,
    owner: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO tasks (task_id, subject, description, status, owner)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(task_id) DO UPDATE SET
             subject = COALESCE(excluded.subject, tasks.subject),
             description = COALESCE(excluded.description, tasks.description),
             status = excluded.status,
             owner = COALESCE(excluded.owner, tasks.owner),
             updated_at = strftime('%Y-%m-%dT%H:%M:%f', 'now')",
        params![task_id, subject, description, status, owner],
    )?;
    Ok(())
}

/// Update a task's status and refresh updated_at.
pub fn update_task_status(conn: &Connection, task_id: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET status = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%f', 'now')
         WHERE task_id = ?2",
        params![status, task_id],
    )?;
    Ok(())
}

/// Record that `task_id` is blocked by `blocked_by`.
pub fn add_task_dep(conn: &Connection, task_id: &str, blocked_by: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO task_deps (task_id, blocked_by, resolved) VALUES (?1, ?2, 0)",
        params![task_id, blocked_by],
    )?;
    Ok(())
}

/// Mark the dependency between `task_id` and `blocked_by` as resolved.
pub fn resolve_dep(conn: &Connection, task_id: &str, blocked_by: &str) -> Result<()> {
    conn.execute(
        "UPDATE task_deps SET resolved = 1 WHERE task_id = ?1 AND blocked_by = ?2",
        params![task_id, blocked_by],
    )?;
    Ok(())
}

/// Return tasks whose all dependencies are resolved and whose status is pending or in_progress.
pub fn get_unblocked_tasks(conn: &Connection) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(
        "SELECT t.task_id, t.subject, t.description, t.status, t.owner, t.updated_at
         FROM tasks t
         WHERE t.status IN ('pending', 'in_progress')
           AND NOT EXISTS (
               SELECT 1 FROM task_deps d
               WHERE d.task_id = t.task_id AND d.resolved = 0
           )
         ORDER BY t.updated_at",
    )?;
    let tasks = stmt
        .query_map([], |row| {
            Ok(Task {
                task_id: row.get(0)?,
                subject: row.get(1)?,
                description: row.get(2)?,
                status: row.get(3)?,
                owner: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(tasks)
}

/// Retrieve a single task by ID.
pub fn get_task(conn: &Connection, task_id: &str) -> Result<Option<Task>> {
    let mut stmt = conn.prepare(
        "SELECT task_id, subject, description, status, owner, updated_at
         FROM tasks WHERE task_id = ?1",
    )?;
    let mut rows = stmt.query(params![task_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(Task {
            task_id: row.get(0)?,
            subject: row.get(1)?,
            description: row.get(2)?,
            status: row.get(3)?,
            owner: row.get(4)?,
            updated_at: row.get(5)?,
        }))
    } else {
        Ok(None)
    }
}

/// List all tasks ordered by updated_at descending.
pub fn list_tasks(conn: &Connection) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(
        "SELECT task_id, subject, description, status, owner, updated_at
         FROM tasks ORDER BY updated_at DESC",
    )?;
    let tasks = stmt
        .query_map([], |row| {
            Ok(Task {
                task_id: row.get(0)?,
                subject: row.get(1)?,
                description: row.get(2)?,
                status: row.get(3)?,
                owner: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::tests::make_store;

    #[test]
    fn upsert_and_get_task() {
        let store = make_store();
        upsert_task(
            store.conn(),
            "task-1",
            Some("Do thing"),
            Some("Detailed desc"),
            "pending",
            Some("dev-1"),
        )
        .expect("upsert");
        let task = get_task(store.conn(), "task-1")
            .expect("query")
            .expect("exists");
        assert_eq!(task.subject.as_deref(), Some("Do thing"));
        assert_eq!(task.status, "pending");
        assert_eq!(task.owner.as_deref(), Some("dev-1"));
    }

    #[test]
    fn update_task_status_works() {
        let store = make_store();
        upsert_task(store.conn(), "task-2", None, None, "pending", None).expect("upsert");
        update_task_status(store.conn(), "task-2", "completed").expect("update");
        let task = get_task(store.conn(), "task-2")
            .expect("query")
            .expect("exists");
        assert_eq!(task.status, "completed");
    }

    #[test]
    fn dependency_tracking() {
        let store = make_store();
        upsert_task(store.conn(), "blocker", None, None, "pending", None).expect("upsert");
        upsert_task(store.conn(), "blocked", None, None, "pending", None).expect("upsert");
        add_task_dep(store.conn(), "blocked", "blocker").expect("add dep");

        // While dep is unresolved, blocked task should NOT appear in unblocked list.
        let unblocked = get_unblocked_tasks(store.conn()).expect("unblocked");
        let ids: Vec<_> = unblocked.iter().map(|t| t.task_id.as_str()).collect();
        assert!(ids.contains(&"blocker"));
        assert!(!ids.contains(&"blocked"));

        // Resolve the dep.
        resolve_dep(store.conn(), "blocked", "blocker").expect("resolve");
        let unblocked2 = get_unblocked_tasks(store.conn()).expect("unblocked2");
        let ids2: Vec<_> = unblocked2.iter().map(|t| t.task_id.as_str()).collect();
        assert!(ids2.contains(&"blocked"));
    }
}
