use super::super::{
    domain::{ControlDecision, ExecutionStatus, RecordedDecision},
    ports::DecisionRepository,
};
use crate::{application::QueryLimit, domain::EventId};
use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};
use std::{path::Path, sync::Mutex};

pub struct SqliteDecisionRepository {
    connection: Mutex<Connection>,
}

impl SqliteDecisionRepository {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS control_decisions(
             id TEXT PRIMARY KEY, decision TEXT NOT NULL, status TEXT NOT NULL);",
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}

#[async_trait]
impl DecisionRepository for SqliteDecisionRepository {
    async fn prepare(&self, decision: &ControlDecision) -> Result<()> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("audit lock poisoned"))?;
        let payload = serde_json::to_string(decision)?;
        let existing: Option<String> = connection
            .query_row(
                "SELECT decision FROM control_decisions WHERE id=?1",
                [decision.id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            ensure!(existing == payload, "decision ID conflict");
        } else {
            connection.execute(
                "INSERT INTO control_decisions VALUES(?1,?2,?3)",
                params![
                    decision.id.as_str(),
                    payload,
                    serde_json::to_string(&ExecutionStatus::Pending)?
                ],
            )?;
        }
        Ok(())
    }
    async fn finish(&self, id: &EventId, status: ExecutionStatus) -> Result<()> {
        ensure!(
            status != ExecutionStatus::Pending,
            "cannot finish as pending"
        );
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("audit lock poisoned"))?;
        let changed = connection.execute(
            "UPDATE control_decisions SET status=?1 WHERE id=?2 AND (status=?3 OR status=?1)",
            params![
                serde_json::to_string(&status)?,
                id.as_str(),
                serde_json::to_string(&ExecutionStatus::Pending)?
            ],
        )?;
        ensure!(
            changed == 1,
            "missing decision or invalid execution transition"
        );
        Ok(())
    }
    async fn recent(&self, limit: QueryLimit) -> Result<Vec<RecordedDecision>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("audit lock poisoned"))?;
        let mut query = connection.prepare(
            "SELECT decision,status FROM control_decisions ORDER BY rowid DESC LIMIT ?1",
        )?;
        let rows = query.query_map([limit.value()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (decision, status) = row?;
            Ok(RecordedDecision {
                decision: serde_json::from_str(&decision)?,
                status: serde_json::from_str(&status)?,
            })
        })
        .collect()
    }
}
