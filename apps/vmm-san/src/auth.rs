//! Shared-secret authentication between CoreSAN peers.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

/// Header name for peer authentication.
pub const PEER_SECRET_HEADER: &str = "X-CoreSAN-Secret";

/// Axum middleware that validates the shared secret on incoming peer requests.
pub async fn require_peer_secret(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract the expected secret from the app state extension.
    let expected = request.extensions()
        .get::<PeerSecret>()
        .map(|s| s.0.clone())
        .unwrap_or_default();

    // If no secret is configured, allow all requests (single-node mode).
    if expected.is_empty() {
        return Ok(next.run(request).await);
    }

    let provided = request.headers()
        .get(PEER_SECRET_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if provided == expected {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Wrapper type for the peer shared secret, stored as an Axum extension.
#[derive(Clone)]
pub struct PeerSecret(pub String);
