use crate::{
    application::IncidentQuery,
    domain::{
        AnalysisFailure, Diagnosis, DomainError, Incident, IncidentId, IngestReceipt, Observation,
    },
    ports::IncidentRepository,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

pub struct SqliteIncidentRepository {
    connection: Arc<Mutex<Connection>>,
}
impl SqliteIncidentRepository {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        Self::initialize(Connection::open(path)?)
    }
    pub fn in_memory() -> Result<Self> {
        Self::initialize(Connection::open_in_memory()?)
    }
    fn initialize(mut connection: Connection) -> Result<Self> {
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS incidents(id TEXT PRIMARY KEY,namespace TEXT NOT NULL,deployment TEXT NOT NULL,last_seen TEXT NOT NULL,payload TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS occurrences(event_id TEXT PRIMARY KEY,incident_id TEXT NOT NULL REFERENCES incidents(id),observed_at TEXT NOT NULL,payload TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS incident_scope ON incidents(namespace,deployment,last_seen);
            CREATE INDEX IF NOT EXISTS occurrence_scope ON occurrences(incident_id,observed_at);")?;
        let payloads: Vec<String> = {
            let mut statement = connection.prepare("SELECT payload FROM incidents")?;
            statement
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let tx = connection.transaction()?;
        for payload in payloads {
            let mut incident: Incident = serde_json::from_str(&payload)?;
            incident.recover_analysis(Utc::now());
            Self::save(&tx, &incident)?;
        }
        tx.commit()?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }
    async fn run<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = connection
                .lock()
                .map_err(|_| anyhow!("database lock poisoned"))?;
            operation(&mut connection)
        })
        .await
        .context("database worker failed")?
    }
    fn save(connection: &Connection, incident: &Incident) -> Result<()> {
        connection.execute(
            "INSERT INTO incidents VALUES(?1,?2,?3,?4,?5)
            ON CONFLICT(id) DO UPDATE SET last_seen=excluded.last_seen,payload=excluded.payload",
            params![
                incident.id().as_str(),
                incident.observation().workload.namespace.as_str(),
                incident.observation().workload.deployment.as_str(),
                incident.last_seen().to_rfc3339(),
                serde_json::to_string(incident)?
            ],
        )?;
        Ok(())
    }
    fn load(connection: &Connection, id: &IncidentId) -> Result<Incident> {
        let payload: String = connection.query_row(
            "SELECT payload FROM incidents WHERE id=?1",
            [id.as_str()],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&payload)?)
    }
}

#[async_trait]
impl IncidentRepository for SqliteIncidentRepository {
    async fn record(&self, observation: Observation) -> Result<IngestReceipt> {
        self.run(move |connection| {
            let tx = connection.transaction()?;
            let existing: Option<(String, String)> = tx
                .query_row(
                    "SELECT incident_id,payload FROM occurrences WHERE event_id=?1",
                    [observation.event_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((id, payload)) = existing {
                let original: Observation = serde_json::from_str(&payload)?;
                if original != observation {
                    return Err(DomainError::EventConflict.into());
                }
                return Ok(IngestReceipt {
                    incident_id: IncidentId::new(id)?,
                    duplicate: true,
                });
            }
            let id = observation.fingerprint();
            let existing: Option<String> = tx
                .query_row(
                    "SELECT payload FROM incidents WHERE id=?1",
                    [id.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            let incident = if let Some(payload) = existing {
                let mut incident: Incident = serde_json::from_str(&payload)?;
                incident.register(&observation)?;
                incident
            } else {
                Incident::new(observation.clone(), Utc::now())
            };
            Self::save(&tx, &incident)?;
            tx.execute(
                "INSERT INTO occurrences VALUES(?1,?2,?3,?4)",
                params![
                    observation.event_id.as_str(),
                    id.as_str(),
                    observation.observed_at.to_rfc3339(),
                    serde_json::to_string(&observation)?
                ],
            )?;
            tx.commit()?;
            Ok(IngestReceipt {
                incident_id: id,
                duplicate: false,
            })
        })
        .await
    }
    async fn list(&self, query: IncidentQuery) -> Result<Vec<Incident>> {
        self.run(move |connection| {
            let mut statement = connection.prepare("SELECT payload FROM incidents WHERE (?1 IS NULL OR namespace=?1) AND (?2 IS NULL OR deployment=?2) ORDER BY last_seen DESC LIMIT ?3")?;
            let rows = statement.query_map(params![query.namespace.as_ref().map(|v| v.as_str()),query.deployment.as_ref().map(|v| v.as_str()),query.limit.value()], |row| row.get::<_, String>(0))?;
            rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
        }).await
    }
    async fn occurrences(&self, id: IncidentId) -> Result<Vec<Observation>> {
        self.run(move |connection| {
            let mut statement = connection.prepare("SELECT payload FROM occurrences WHERE incident_id=?1 ORDER BY observed_at DESC LIMIT 100")?;
            statement.query_map([id.as_str()], |row| row.get::<_, String>(0))?.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
        }).await
    }
    async fn claim_pending(&self, now: DateTime<Utc>) -> Result<Option<Incident>> {
        self.run(move |connection| {
            let tx = connection.transaction()?;
            let payloads: Vec<String> = {
                let mut statement =
                    tx.prepare("SELECT payload FROM incidents ORDER BY last_seen")?;
                statement
                    .query_map([], |row| row.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            for payload in payloads {
                let mut incident: Incident = serde_json::from_str(&payload)?;
                if !incident.analysis().pending_at(now) {
                    continue;
                }
                incident.claim_analysis(now)?;
                Self::save(&tx, &incident)?;
                tx.commit()?;
                return Ok(Some(incident));
            }
            Ok(None)
        })
        .await
    }
    async fn finish_analysis(
        &self,
        id: IncidentId,
        diagnosis: Result<Diagnosis, AnalysisFailure>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.run(move |connection| {
            let tx = connection.transaction()?;
            let mut incident = Self::load(&tx, &id)?;
            match diagnosis {
                Ok(diagnosis) => incident.complete_analysis(diagnosis)?,
                Err(error) => incident.fail_analysis(error, now)?,
            }
            Self::save(&tx, &incident)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
    async fn health(&self) -> Result<()> {
        self.run(|connection| {
            connection.query_row("SELECT 1", [], |_| Ok(()))?;
            Ok(())
        })
        .await
    }
}
