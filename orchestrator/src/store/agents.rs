#![allow(dead_code)]

use rusqlite::{params, Connection, Result};

/// Agent record as stored in the database.
#[derive(Debug, Clone)]
pub struct Agent {
    pub name: String,
    pub session_id: String,
    pub agent_type: Option<String>,
    pub status: String,
    pub spawner_session_id: Option<String>,
    pub first_seen: String,
    pub last_activity: String,
}

/// Insert or update an agent record. Sets status to 'active' and updates last_activity.
pub fn upsert_agent(
    conn: &Connection,
    name: &str,
    session_id: &str,
    agent_type: Option<&str>,
    spawner_session_id: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO agents (name, session_id, agent_type, status, spawner_session_id,
                             first_seen, last_activity)
         VALUES (?1, ?2, ?3, 'active', ?4,
                 strftime('%Y-%m-%dT%H:%M:%f', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%f', 'now'))
         ON CONFLICT(name) DO UPDATE SET
             session_id = excluded.session_id,
             agent_type = COALESCE(excluded.agent_type, agents.agent_type),
             status = 'active',
             spawner_session_id = COALESCE(excluded.spawner_session_id, agents.spawner_session_id),
             last_activity = strftime('%Y-%m-%dT%H:%M:%f', 'now')",
        params![name, session_id, agent_type, spawner_session_id],
    )?;
    Ok(())
}

/// Update an agent's status (e.g. 'active', 'idle', 'dead').
pub fn update_agent_status(conn: &Connection, name: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE agents SET status = ?1 WHERE name = ?2",
        params![status, name],
    )?;
    Ok(())
}

/// Record that an agent performed activity, updating its last_activity timestamp.
pub fn update_agent_activity(conn: &Connection, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE agents SET last_activity = strftime('%Y-%m-%dT%H:%M:%f', 'now') WHERE name = ?1",
        params![name],
    )?;
    Ok(())
}

/// Retrieve an agent by name.
pub fn get_agent(conn: &Connection, name: &str) -> Result<Option<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT name, session_id, agent_type, status, spawner_session_id, first_seen, last_activity
         FROM agents WHERE name = ?1",
    )?;
    let mut rows = stmt.query(params![name])?;
    if let Some(row) = rows.next()? {
        Ok(Some(Agent {
            name: row.get(0)?,
            session_id: row.get(1)?,
            agent_type: row.get(2)?,
            status: row.get(3)?,
            spawner_session_id: row.get(4)?,
            first_seen: row.get(5)?,
            last_activity: row.get(6)?,
        }))
    } else {
        Ok(None)
    }
}

/// Retrieve an agent by session ID.
pub fn get_agent_by_session(conn: &Connection, session_id: &str) -> Result<Option<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT name, session_id, agent_type, status, spawner_session_id, first_seen, last_activity
         FROM agents WHERE session_id = ?1",
    )?;
    let mut rows = stmt.query(params![session_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(Agent {
            name: row.get(0)?,
            session_id: row.get(1)?,
            agent_type: row.get(2)?,
            status: row.get(3)?,
            spawner_session_id: row.get(4)?,
            first_seen: row.get(5)?,
            last_activity: row.get(6)?,
        }))
    } else {
        Ok(None)
    }
}

/// List all agents.
pub fn list_agents(conn: &Connection) -> Result<Vec<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT name, session_id, agent_type, status, spawner_session_id, first_seen, last_activity
         FROM agents ORDER BY first_seen",
    )?;
    let agents = stmt
        .query_map([], |row| {
            Ok(Agent {
                name: row.get(0)?,
                session_id: row.get(1)?,
                agent_type: row.get(2)?,
                status: row.get(3)?,
                spawner_session_id: row.get(4)?,
                first_seen: row.get(5)?,
                last_activity: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(agents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::tests::make_store;

    #[test]
    fn upsert_and_get_agent() {
        let store = make_store();
        upsert_agent(store.conn(), "architect", "sess-1", Some("architect"), None).expect("upsert");

        let agent = get_agent(store.conn(), "architect")
            .expect("query")
            .expect("exists");
        assert_eq!(agent.name, "architect");
        assert_eq!(agent.session_id, "sess-1");
        assert_eq!(agent.status, "active");
        assert_eq!(agent.agent_type.as_deref(), Some("architect"));
    }

    #[test]
    fn update_status() {
        let store = make_store();
        upsert_agent(store.conn(), "worker", "sess-2", None, None).expect("upsert");
        update_agent_status(store.conn(), "worker", "dead").expect("update status");
        let agent = get_agent(store.conn(), "worker")
            .expect("query")
            .expect("exists");
        assert_eq!(agent.status, "dead");
    }

    #[test]
    fn get_by_session() {
        let store = make_store();
        upsert_agent(
            store.conn(),
            "dev-1",
            "sess-xyz",
            Some("dev"),
            Some("sess-parent"),
        )
        .expect("upsert");
        let agent = get_agent_by_session(store.conn(), "sess-xyz")
            .expect("query")
            .expect("exists");
        assert_eq!(agent.name, "dev-1");
    }

    #[test]
    fn list_agents_empty_and_populated() {
        let store = make_store();
        assert_eq!(list_agents(store.conn()).expect("list").len(), 0);
        upsert_agent(store.conn(), "a", "s1", None, None).expect("upsert");
        upsert_agent(store.conn(), "b", "s2", None, None).expect("upsert");
        assert_eq!(list_agents(store.conn()).expect("list").len(), 2);
    }
}
