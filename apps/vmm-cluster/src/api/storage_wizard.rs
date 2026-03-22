//! Storage Wizard API — guided cluster filesystem setup.

use axum::{Json, extract::State};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::storage_wizard::{StorageWizardService, WizardConfig};

#[derive(Deserialize)]
pub struct CheckHostsRequest {
    pub cluster_id: String,
    pub fs_type: String,
}

/// POST /api/storage/wizard/check — Check package status on all hosts.
pub async fn check_hosts(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Json(body): Json<CheckHostsRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Get all hosts in the cluster
    let host_ids: Vec<String> = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        crate::services::host::HostService::list(&db).unwrap_or_default()
            .iter()
            .filter(|h| h.cluster_id == body.cluster_id && h.status == "online")
            .map(|h| h.id.clone())
            .collect()
    };

    let results = StorageWizardService::check_hosts(&state, &host_ids, &body.fs_type).await;
    Ok(Json(serde_json::to_value(results).unwrap()))
}

#[derive(Deserialize)]
pub struct InstallRequest {
    pub host_ids: Vec<String>,
    pub fs_type: String,
    #[serde(default)]
    pub sudo_passwords: std::collections::HashMap<String, String>,
}

/// POST /api/storage/wizard/install — Install missing packages on hosts.
pub async fn install_packages(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<InstallRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    StorageWizardService::install_on_hosts(&state, &body.host_ids, &body.fs_type, &body.sudo_passwords).await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /api/storage/wizard/setup — Setup filesystem and create datastore.
pub async fn setup(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(config): Json<WizardConfig>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;

    let steps = StorageWizardService::setup(&state, &config).await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "steps": steps,
    })))
}
