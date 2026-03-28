//! Events API — cluster-wide event log + ingress for external services.

use axum::{Json, extract::{State, Query}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError};
use crate::services::event::EventService;

#[derive(Deserialize)]
pub struct EventQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
    pub category: Option<String>,
}
fn default_limit() -> u32 { 50 }

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Query(q): Query<EventQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let events = EventService::recent(&db, q.limit, q.category.as_deref())
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(events).unwrap()))
}

// ── Event Ingress — for external services (vmm-san, vmm-server) ─────

#[derive(Deserialize)]
pub struct IngestEvent {
    pub severity: String,       // "info", "warning", "critical"
    pub category: String,       // "san", "server", "disk", "network", "vm"
    pub message: String,
    pub target_type: Option<String>,  // "disk", "volume", "host", "vm"
    pub target_id: Option<String>,
    pub host_id: Option<String>,
    pub hostname: Option<String>,
}

#[derive(Deserialize)]
pub struct IngestBatch {
    pub events: Vec<IngestEvent>,
}

/// POST /api/events/ingest — receive events from external services.
/// Used by vmm-san and vmm-server to proactively report errors, warnings, and state changes.
/// Accepts a single event or a batch.
pub async fn ingest(
    State(state): State<Arc<ClusterState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;

    // Accept both single event and batch
    let events: Vec<IngestEvent> = if body.get("events").is_some() {
        serde_json::from_value::<IngestBatch>(body.clone())
            .map(|b| b.events)
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Invalid batch: {}", e)))?
    } else {
        vec![serde_json::from_value::<IngestEvent>(body)
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Invalid event: {}", e)))?]
    };

    let mut ingested = 0;
    for event in &events {
        // Validate severity
        if !["info", "warning", "critical"].contains(&event.severity.as_str()) {
            continue;
        }

        // Resolve host_id from hostname if not provided
        let host_id = event.host_id.as_deref().or_else(|| {
            event.hostname.as_deref().and_then(|hn| {
                db.query_row(
                    "SELECT id FROM hosts WHERE hostname = ?1",
                    rusqlite::params![hn], |row| row.get::<_, String>(0),
                ).ok().as_deref().map(|_| "") // TODO: proper resolution
            })
        });

        EventService::log(
            &db,
            &event.severity,
            &event.category,
            &event.message,
            event.target_type.as_deref(),
            event.target_id.as_deref(),
            event.host_id.as_deref(),
        );
        ingested += 1;

        // Log critical events to tracing as well
        if event.severity == "critical" {
            tracing::error!("[event-ingest] CRITICAL from {}: {}",
                event.hostname.as_deref().unwrap_or("unknown"), event.message);
        } else if event.severity == "warning" {
            tracing::warn!("[event-ingest] WARNING from {}: {}",
                event.hostname.as_deref().unwrap_or("unknown"), event.message);
        }
    }

    tracing::info!("[event-ingest] Ingested {} event(s)", ingested);
    Ok(Json(serde_json::json!({ "ingested": ingested })))
}
