//! Tasks API — view long-running operations (migrations, HA restarts, etc.).

use axum::{Json, extract::{State, Query}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError};
use crate::services::task::TaskService;

#[derive(Deserialize)]
pub struct TaskQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
}
fn default_limit() -> u32 { 50 }

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Query(q): Query<TaskQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let tasks = TaskService::list(&db, q.limit)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(tasks).unwrap()))
}
