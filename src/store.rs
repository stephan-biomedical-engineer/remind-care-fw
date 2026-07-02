use rusqlite::{params, Connection, Result};
use std::path::Path;
use tokio::sync::Mutex;
use std::sync::Arc;

use crate::models::{EventPayload, ScheduleResponse};

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                payload TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS key_value (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn push_event(&self, event: &EventPayload) -> Result<()> {
        let payload = serde_json::to_string(event).unwrap();
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO events (payload) VALUES (?1)",
            params![payload],
        )?;
        Ok(())
    }

    pub async fn get_unsynced_events(&self) -> Result<Vec<(i32, EventPayload)>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT id, payload FROM events ORDER BY id ASC")?;
        
        let event_iter = stmt.query_map([], |row| {
            let id: i32 = row.get(0)?;
            let payload_str: String = row.get(1)?;
            let payload: EventPayload = serde_json::from_str(&payload_str).unwrap();
            Ok((id, payload))
        })?;

        let mut events = Vec::new();
        for event in event_iter {
            events.push(event?);
        }
        Ok(events)
    }

    pub async fn delete_event(&self, id: i32) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM events WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub async fn get_unsynced_count(&self) -> Result<u64> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM events")?;
        let count: u64 = stmt.query_row([], |row| row.get(0))?;
        Ok(count)
    }

    pub async fn save_schedule(&self, schedule: &ScheduleResponse) -> Result<()> {
        let payload = serde_json::to_string(schedule).unwrap();
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO key_value (key, value) VALUES ('schedule', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![payload],
        )?;
        Ok(())
    }

    pub async fn load_schedule(&self) -> Result<Option<ScheduleResponse>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT value FROM key_value WHERE key = 'schedule'")?;
        
        let result: rusqlite::Result<String> = stmt.query_row([], |row| row.get(0));
        
        match result {
            Ok(payload_str) => {
                let schedule: ScheduleResponse = serde_json::from_str(&payload_str).unwrap();
                Ok(Some(schedule))
            },
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
