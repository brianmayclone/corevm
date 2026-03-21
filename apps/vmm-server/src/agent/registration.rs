//! Agent registration/deregistration handlers.
//!
//! Called by vmm-cluster to register this node as a managed agent.

use axum::{Json, extract::State};
use axum::http::StatusCode;
use std::sync::Arc;
use crate::state::AppState;
use crate::auth::middleware::AppError;
use vmm_core::cluster::{AgentRegisterRequest, AgentRegisterResponse, ManagedNodeConfig, AgentResponse};

/// POST /agent/register — Register this node with a cluster.
/// Does NOT require agent auth (the cluster doesn't have a token yet).
/// The cluster has already verified admin credentials via /api/auth/login.
pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentRegisterRequest>,
) -> Result<Json<AgentRegisterResponse>, AppError> {
    // Check if already managed
    {
        let managed = state.managed_config.lock()
            .map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "Lock error".into()))?;
        if managed.is_some() {
            return Err(AppError(StatusCode::CONFLICT, "Already managed by a cluster".into()));
        }
    }

    let config = ManagedNodeConfig {
        managed: true,
        cluster_id: req.cluster_id,
        cluster_url: req.cluster_url,
        agent_token: req.agent_token,
        node_id: req.node_id.clone(),
    };

    // Persist to disk
    let config_path = state.config.vms.config_dir.join("cluster.json");
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    std::fs::write(&config_path, &json)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot save cluster config: {}", e)))?;

    tracing::info!("Registered with cluster: node_id={}", req.node_id);

    // Set in-memory state
    {
        let mut managed = state.managed_config.lock()
            .map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "Lock error".into()))?;
        *managed = Some(config);
    }

    let hostname = gethostname::gethostname().to_string_lossy().to_string();
    Ok(Json(AgentRegisterResponse {
        node_id: req.node_id,
        hostname,
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}

/// POST /agent/deregister — Remove this node from cluster management.
pub async fn deregister(
    State(state): State<Arc<AppState>>,
    _agent: crate::agent::auth::AgentAuth,
) -> Result<Json<AgentResponse>, AppError> {
    // Remove persistent config
    let config_path = state.config.vms.config_dir.join("cluster.json");
    let _ = std::fs::remove_file(&config_path);

    // Clear in-memory state
    {
        let mut managed = state.managed_config.lock()
            .map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "Lock error".into()))?;
        *managed = None;
    }

    tracing::info!("Deregistered from cluster — returning to standalone mode");
    Ok(Json(AgentResponse::ok()))
}
