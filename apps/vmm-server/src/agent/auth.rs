//! Agent authentication — validates X-Agent-Token header from the cluster.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use std::sync::Arc;
use crate::state::AppState;
use crate::auth::middleware::AppError;

/// Authenticated agent request — extracted from X-Agent-Token header.
/// Only valid when the node is in managed mode and the token matches.
#[derive(Debug, Clone)]
pub struct AgentAuth;

impl FromRequestParts<Arc<AppState>> for AgentAuth {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        // Check if we're in managed mode
        let managed_config = state.managed_config.lock()
            .map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "Lock error".into()))?;

        let config = managed_config.as_ref()
            .ok_or_else(|| AppError(StatusCode::FORBIDDEN, "Node is not in managed mode".into()))?;

        // Validate token
        let token = parts.headers.get("X-Agent-Token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError(StatusCode::UNAUTHORIZED, "Missing X-Agent-Token header".into()))?;

        if token != config.agent_token {
            return Err(AppError(StatusCode::UNAUTHORIZED, "Invalid agent token".into()));
        }

        Ok(AgentAuth)
    }
}
