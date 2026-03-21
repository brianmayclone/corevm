//! Alarms API — view and acknowledge alarms.
//!
//! Thin handlers delegating to AlarmService.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_operator};
use crate::services::alarm::AlarmService;

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let alarms = AlarmService::list(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(alarms).unwrap()))
}

/// POST /api/alarms/{id}/acknowledge
pub async fn acknowledge(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    AlarmService::acknowledge(&db, id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}
