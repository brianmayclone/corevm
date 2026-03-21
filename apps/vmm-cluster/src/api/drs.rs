//! DRS API — view and manage DRS recommendations.
//!
//! Thin handlers delegating to DrsService.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::drs_service::DrsService;
use crate::services::audit::AuditService;

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let recs = DrsService::list_pending(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(recs).unwrap()))
}

/// POST /api/drs/{id}/apply — Apply a DRS recommendation (trigger migration).
pub async fn apply(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;

    let (vm_id, target_host_id) = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        DrsService::get_apply_target(&db, id)
            .map_err(|e| AppError(StatusCode::NOT_FOUND, e))?
    };

    {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        DrsService::mark_applied(&db, id)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        AuditService::log(&db, user.id, "drs.apply", "drs_recommendation", &id.to_string(), None);
    }

    let state_clone = state.clone();
    tokio::spawn(async move {
        crate::services::migration::MigrationService::migrate_vm(
            &state_clone, &vm_id, &target_host_id, "drs", Some(user.id),
        ).await;
    });

    Ok(Json(serde_json::json!({"ok": true, "action": "migration_started"})))
}

/// POST /api/drs/{id}/dismiss — Dismiss a DRS recommendation.
pub async fn dismiss(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    DrsService::dismiss(&db, id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    AuditService::log(&db, user.id, "drs.dismiss", "drs_recommendation", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}
