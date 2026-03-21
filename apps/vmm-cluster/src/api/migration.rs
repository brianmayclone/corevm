//! Migration API — trigger VM migration between hosts.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_operator};
use crate::services::audit::AuditService;

#[derive(Deserialize)]
pub struct MigrateRequest {
    pub target_host_id: String,
}

/// POST /api/vms/{id}/migrate — Migrate a VM to another host.
pub async fn migrate(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(vm_id): Path<String>,
    Json(body): Json<MigrateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    // Validate VM exists
    {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        let _vm = crate::services::vm::VmService::get(&db, &vm_id)
            .map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        AuditService::log(&db, user.id, "vm.migrate.requested", "vm", &vm_id,
            Some(&format!("target: {}", body.target_host_id)));
    }

    // Validate target host
    {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        let host = crate::services::host::HostService::get(&db, &body.target_host_id)
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
        if host.status != "online" {
            return Err(AppError(StatusCode::BAD_REQUEST, "Target host is not online".into()));
        }
        if host.maintenance_mode {
            return Err(AppError(StatusCode::BAD_REQUEST, "Target host is in maintenance mode".into()));
        }
    }

    // Start migration in background
    let state_clone = state.clone();
    let target = body.target_host_id.clone();
    let uid = user.id;
    tokio::spawn(async move {
        crate::services::migration::MigrationService::migrate_vm(
            &state_clone, &vm_id, &target, "manual", Some(uid),
        ).await;
    });

    Ok(Json(serde_json::json!({"ok": true, "action": "migration_started"})))
}
