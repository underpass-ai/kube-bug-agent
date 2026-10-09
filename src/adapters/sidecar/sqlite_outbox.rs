use super::{SpoolCapacity, tail_cursor::TailCursor};
use crate::{
    domain::{EventId, Observation},
    ports::ObservationOutbox,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};
use std::{path::Path, sync::Mutex};

pub struct SqliteOutbox {
    connection: Mutex<Connection>,
    capacity: SpoolCapacity,
}
impl SqliteOutbox {
    pub fn open(path: &Path, capacity: SpoolCapacity) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS outbox(event_id TEXT PRIMARY KEY,payload TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS cursors(path TEXT PRIMARY KEY,value TEXT NOT NULL);",
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
            capacity,
        })
    }
    pub(super) fn cursor(&self, path: &str) -> Result<TailCursor> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("outbox lock poisoned"))?;
        let value: Option<String> = connection
            .query_row("SELECT value FROM cursors WHERE path=?1", [path], |row| {
                row.get(0)
            })
            .optional()?;
        value
            .map(|value| serde_json::from_str(&value).map_err(Into::into))
            .unwrap_or_else(|| Ok(TailCursor::default()))
    }
    pub(super) fn commit_line(
        &self,
        path: &str,
        cursor: &TailCursor,
        observation: Option<Observation>,
    ) -> Result<bool> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("outbox lock poisoned"))?;
        let tx = connection.transaction()?;
        if let Some(observation) = observation {
            let count: u32 = tx.query_row("SELECT count(*) FROM outbox", [], |row| row.get(0))?;
            if count >= self.capacity.value() {
                return Ok(false);
            }
            tx.execute(
                "INSERT OR IGNORE INTO outbox VALUES(?1,?2)",
                params![
                    observation.event_id.as_str(),
                    serde_json::to_string(&observation)?
                ],
            )?;
        }
        tx.execute("INSERT INTO cursors VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET value=excluded.value",params![path,serde_json::to_string(cursor)?])?;
        tx.commit()?;
        Ok(true)
    }
    pub fn enqueue(&self, observation: Observation) -> Result<bool> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("outbox lock poisoned"))?;
        let tx = connection.transaction()?;
        let count: u32 = tx.query_row("SELECT count(*) FROM outbox", [], |row| row.get(0))?;
        if count >= self.capacity.value() {
            return Ok(false);
        }
        tx.execute(
            "INSERT OR IGNORE INTO outbox VALUES(?1,?2)",
            params![
                observation.event_id.as_str(),
                serde_json::to_string(&observation)?
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }
}
#[async_trait]
impl ObservationOutbox for SqliteOutbox {
    async fn next(&self) -> Result<Option<Observation>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("outbox lock poisoned"))?;
        let payload: Option<String> = connection
            .query_row(
                "SELECT payload FROM outbox ORDER BY rowid LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
            .transpose()
    }
    async fn acknowledge(&self, id: EventId) -> Result<()> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("outbox lock poisoned"))?;
        connection.execute("DELETE FROM outbox WHERE event_id=?1", [id.as_str()])?;
        Ok(())
    }
}
