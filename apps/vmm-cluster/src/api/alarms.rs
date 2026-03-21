//! Alarms API — view and acknowledge alarms.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use serde::Serialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_operator};

#[derive(Debug, Serialize)]
pub struct Alarm {
    pub id: i64,
    pub name: String,
    pub target_type: String,
    pub target_id: String,
    pub condition_type: String,
    pub threshold: Option<f64>,
    pub severity: String,
    pub triggered: bool,
    pub acknowledged: bool,
    pub created_at: String,
    pub triggered_at: Option<String>,
}

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let mut stmt = db.prepare(
        "SELECT id, name, target_type, target_id, condition_type, threshold, severity, \
                triggered, acknowledged, created_at, triggered_at \
         FROM alarms ORDER BY triggered DESC, created_at DESC"
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let alarms: Vec<Alarm> = stmt.query_map([], |row| {
        Ok(Alarm {
            id: row.get(0)?, name: row.get(1)?, target_type: row.get(2)?,
            target_id: row.get(3)?, condition_type: row.get(4)?,
            threshold: row.get(5)?, severity: row.get(6)?,
            triggered: row.get::<_, i32>(7)? != 0,
            acknowledged: row.get::<_, i32>(8)? != 0,
            created_at: row.get(9)?, triggered_at: row.get(10)?,
        })
    }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .filter_map(|r| r.ok()).collect();

    Ok(Json(serde_json::to_value(alarms).unwrap()))
}

/// POST /api/alarms/{id}/acknowledge
pub async fn acknowledge(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&_user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    db.execute("UPDATE alarms SET acknowledged = 1 WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"ok": true})))
}
