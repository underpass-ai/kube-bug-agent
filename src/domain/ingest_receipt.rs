use super::IncidentId;
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct IngestReceipt {
    pub incident_id: IncidentId,
    pub duplicate: bool,
}
