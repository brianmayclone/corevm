//! Events API — cluster-wide event log.

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
