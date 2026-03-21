//! DRS API — view and manage DRS recommendations.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use serde::Serialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::audit::AuditService;
use crate::services::event::EventService;

#[derive(Debug, Serialize)]
pub struct DrsRecommendation {
    pub id: i64,
    pub cluster_id: String,
    pub vm_id: String,
    pub vm_name: String,
    pub source_host_id: String,
    pub source_host_name: String,
    pub target_host_id: String,
    pub target_host_name: String,
    pub reason: String,
    pub priority: String,
    pub status: String,
    pub created_at: String,
}

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let mut stmt = db.prepare(
        "SELECT r.id, r.cluster_id, r.vm_id, v.name, r.source_host_id, sh.hostname, \
                r.target_host_id, th.hostname, r.reason, r.priority, r.status, r.created_at \
         FROM drs_recommendations r \
         JOIN vms v ON r.vm_id = v.id \
         JOIN hosts sh ON r.source_host_id = sh.id \
         JOIN hosts th ON r.target_host_id = th.id \
         WHERE r.status = 'pending' \
         ORDER BY r.created_at DESC"
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let recs: Vec<DrsRecommendation> = stmt.query_map([], |row| {
        Ok(DrsRecommendation {
            id: row.get(0)?, cluster_id: row.get(1)?, vm_id: row.get(2)?,
            vm_name: row.get(3)?, source_host_id: row.get(4)?,
            source_host_name: row.get(5)?, target_host_id: row.get(6)?,
            target_host_name: row.get(7)?, reason: row.get(8)?,
            priority: row.get(9)?, status: row.get(10)?, created_at: row.get(11)?,
        })
    }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .filter_map(|r| r.ok()).collect();

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
        db.query_row(
            "SELECT vm_id, target_host_id FROM drs_recommendations WHERE id = ?1 AND status = 'pending'",
            rusqlite::params![id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).map_err(|_| AppError(StatusCode::NOT_FOUND, "Recommendation not found or already applied".into()))?
    };

    // Mark as applied
    {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        db.execute("UPDATE drs_recommendations SET status = 'applied' WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        AuditService::log(&db, user.id, "drs.apply", "drs_recommendation", &id.to_string(), None);
    }

    // Trigger migration (in background)
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
    db.execute("UPDATE drs_recommendations SET status = 'dismissed' WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    AuditService::log(&db, user.id, "drs.dismiss", "drs_recommendation", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}
