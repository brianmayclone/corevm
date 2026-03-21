//! Managed-mode guard — blocks regular API calls when host is managed by a cluster.

use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;
use crate::state::AppState;

/// Paths that are ALWAYS allowed, even in managed mode.
const ALLOWED_PATHS: &[&str] = &[
    "/api/system/info",
    "/api/auth/login",
    "/api/auth/me",
];

/// Middleware that blocks all regular API requests when in managed mode.
/// Agent routes (/agent/*) and WebSocket routes (/ws/*) are not affected
/// because they're mounted separately or use their own auth.
pub async fn managed_mode_guard(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();

    // Always allow: agent routes, websocket, and whitelisted paths
    if path.starts_with("/agent/")
        || path.starts_with("/ws/")
        || ALLOWED_PATHS.iter().any(|p| path == *p)
    {
        return next.run(request).await;
    }

    // Check if we're in managed mode
    let managed = state.managed_config.lock()
        .ok()
        .and_then(|m| m.as_ref().map(|c| c.cluster_url.clone()));

    if let Some(cluster_url) = managed {
        // Only block /api/* routes
        if path.starts_with("/api/") {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "managed_by_cluster",
                    "cluster_url": cluster_url,
                    "message": "This host is managed by VMM-Cluster. Use the cluster management interface."
                })),
            ).into_response();
        }
    }

    next.run(request).await
}
