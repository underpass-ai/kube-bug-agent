use super::{CollectorState, IncidentFilters};
use crate::{
    adapters::{
        runtime::ShutdownSignal,
        wire::{IncidentDto, ObservationDto},
    },
    application::IngestIncident,
    domain::{DomainError, IncidentId, IngestReceipt, Observation},
};
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::json;
use std::net::SocketAddr;
use subtle::ConstantTimeEq;

pub struct CollectorServer;
impl CollectorServer {
    pub fn router(state: CollectorState) -> Router {
        let protected = Router::new()
            .route("/v1/observations", post(Self::ingest))
            .route("/v1/incidents", get(Self::list))
            .route("/v1/incidents/{id}/occurrences", get(Self::occurrences))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                Self::authenticate,
            ));
        Router::new()
            .merge(protected)
            .route("/healthz", get(|| async { Json(json!({"status":"ok"})) }))
            .route("/readyz", get(Self::ready))
            .layer(DefaultBodyLimit::max(65_536))
            .with_state(state)
    }
    async fn authenticate(
        State(state): State<CollectorState>,
        request: Request,
        next: Next,
    ) -> Response {
        if let Some(token) = state.token {
            let provided = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .unwrap_or("");
            if provided
                .as_bytes()
                .ct_eq(token.expose().as_bytes())
                .unwrap_u8()
                != 1
            {
                return Self::error(StatusCode::UNAUTHORIZED, "unauthorized");
            }
        }
        next.run(request).await
    }
    async fn ingest(
        State(state): State<CollectorState>,
        Json(observation): Json<ObservationDto>,
    ) -> Result<Json<IngestReceipt>, Response> {
        let mut observation: Observation = observation
            .try_into()
            .map_err(|_| Self::error(StatusCode::BAD_REQUEST, "invalid observation"))?;
        if let Some(topology) = state.topology {
            let identities = topology.read().await;
            let identity = identities
                .get(&observation.workload.pod_uid)
                .filter(|identity| identity.namespace == observation.workload.namespace)
                .ok_or_else(|| {
                    Self::error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "pod identity is not yet available",
                    )
                })?;
            let container = observation.workload.container.clone();
            observation.workload = identity.clone();
            observation.workload.container = container;
        }
        IngestIncident::new(state.repository)
            .execute(observation)
            .await
            .map(Json)
            .map_err(|error| {
                if error
                    .downcast_ref::<DomainError>()
                    .is_some_and(|error| matches!(error, DomainError::EventConflict))
                {
                    Self::error(StatusCode::CONFLICT, "event ID payload conflict")
                } else {
                    Self::error(StatusCode::INTERNAL_SERVER_ERROR, "persistence failed")
                }
            })
    }
    async fn list(
        State(state): State<CollectorState>,
        Query(filters): Query<IncidentFilters>,
    ) -> Result<Json<Vec<IncidentDto>>, Response> {
        let query = filters
            .into_query()
            .map_err(|_| Self::error(StatusCode::BAD_REQUEST, "invalid filters"))?;
        state
            .repository
            .list(query)
            .await
            .map(|items| Json(items.iter().map(IncidentDto::from).collect()))
            .map_err(|_| Self::error(StatusCode::INTERNAL_SERVER_ERROR, "query failed"))
    }
    async fn occurrences(
        State(state): State<CollectorState>,
        Path(id): Path<String>,
    ) -> Result<Json<Vec<ObservationDto>>, Response> {
        let id = IncidentId::new(id)
            .map_err(|_| Self::error(StatusCode::BAD_REQUEST, "invalid incident ID"))?;
        state
            .repository
            .occurrences(id)
            .await
            .map(|items| Json(items.iter().map(ObservationDto::from).collect()))
            .map_err(|_| Self::error(StatusCode::INTERNAL_SERVER_ERROR, "query failed"))
    }
    async fn ready(State(state): State<CollectorState>) -> Response {
        match state.repository.health().await {
            Ok(()) => Json(json!({"status":"ready"})).into_response(),
            Err(_) => Self::error(StatusCode::SERVICE_UNAVAILABLE, "database unavailable"),
        }
    }
    fn error(status: StatusCode, message: &str) -> Response {
        (status, Json(json!({"error":message}))).into_response()
    }
    pub async fn serve(address: SocketAddr, state: CollectorState) -> Result<()> {
        ensure!(
            address.ip().is_loopback() || state.token.is_some(),
            "AGENT_TOKEN is required outside loopback"
        );
        let listener = tokio::net::TcpListener::bind(address).await?;
        let mut shutdown = ShutdownSignal::new()?;
        tracing::info!(address=%listener.local_addr()?,"collector listening");
        axum::serve(listener, Self::router(state))
            .with_graceful_shutdown(async move {
                shutdown.wait().await;
            })
            .await?;
        Ok(())
    }
}
