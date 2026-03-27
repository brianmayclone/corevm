//! API access control middleware — controls whether CLI/API access is allowed.
//!
//! When `api.cli_access_enabled` is false in the config, all API requests
//! are blocked with 403 (except login, system info, and web-ui traffic).

use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;
use crate::state::AppState;

/// Paths that are always allowed, even when CLI access is disabled.
/// This ensures the web UI and basic auth still work.
const ALWAYS_ALLOWED: &[&str] = &[
    "/api/auth/login",
    "/api/auth/me",
    "/api/system/info",
    "/api/settings/api-access",
];

/// Middleware that blocks API requests when CLI access is disabled.
pub async fn api_access_guard(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();

    // Non-API routes (UI, WebSocket, agent) are always allowed
    if !path.starts_with("/api/") {
        return next.run(request).await;
    }

    // Whitelisted paths are always allowed
    if ALWAYS_ALLOWED.iter().any(|p| path == *p) {
        return next.run(request).await;
    }

    // Check if CLI access is enabled
    if !state.config.api.cli_access_enabled {
        // Check if request comes from the web UI (Referer header)
        let is_web_ui = request.headers()
            .get("referer")
            .and_then(|v| v.to_str().ok())
            .map(|r| {
                let server_port = state.config.server.port;
                r.contains(&format!(":{}", server_port)) || r.contains("localhost") || r.contains("127.0.0.1")
            })
            .unwrap_or(false);

        if !is_web_ui {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "cli_access_disabled",
                    "message": "CLI/API access is disabled. Enable it via web UI or server config."
                })),
            ).into_response();
        }
    }

    // Check IP whitelist (if configured)
    if !state.config.api.allowed_ips.is_empty() {
        let client_ip = request.headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
            .or_else(|| {
                request.extensions()
                    .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                    .map(|ci| ci.0.ip().to_string())
            });

        if let Some(ref ip) = client_ip {
            let allowed = state.config.api.allowed_ips.iter()
                .any(|allowed| allowed == ip || allowed == "0.0.0.0");
            if !allowed {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "ip_not_allowed",
                        "message": format!("IP {} is not in the allowed list", ip)
                    })),
                ).into_response();
            }
        }
    }

    next.run(request).await
}
