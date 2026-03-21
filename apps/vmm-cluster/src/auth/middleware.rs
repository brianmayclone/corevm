//! axum middleware for JWT authentication.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode, header},
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::jwt;

/// Authenticated user extracted from JWT — use as axum extractor.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub role: String,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }

    pub fn is_operator_or_above(&self) -> bool {
        self.role == "admin" || self.role == "operator"
    }
}

/// App-wide error type for JSON error responses.
pub struct AppError(pub StatusCode, pub String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error": self.1}))).into_response()
    }
}

impl FromRequestParts<Arc<ClusterState>> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<ClusterState>) -> Result<Self, Self::Rejection> {
        let token = extract_token(parts)
            .ok_or_else(|| AppError(StatusCode::UNAUTHORIZED, "Missing authorization token".into()))?;

        let claims = jwt::validate_token(&token, &state.jwt_secret)
            .map_err(|e| AppError(StatusCode::UNAUTHORIZED, e))?;

        Ok(AuthUser {
            id: claims.sub,
            username: claims.username,
            role: claims.role,
        })
    }
}

/// Extract JWT from `Authorization: Bearer <token>` header or `?token=` query param.
fn extract_token(parts: &Parts) -> Option<String> {
    if let Some(auth_header) = parts.headers.get(header::AUTHORIZATION) {
        if let Ok(val) = auth_header.to_str() {
            if let Some(token) = val.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    if let Some(query) = parts.uri.query() {
        for pair in query.split('&') {
            if let Some(token) = pair.strip_prefix("token=") {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Require admin role — returns AppError if not admin.
pub fn require_admin(user: &AuthUser) -> Result<(), AppError> {
    if user.is_admin() { Ok(()) }
    else { Err(AppError(StatusCode::FORBIDDEN, "Admin access required".into())) }
}

/// Require operator or above — returns AppError if viewer.
pub fn require_operator(user: &AuthUser) -> Result<(), AppError> {
    if user.is_operator_or_above() { Ok(()) }
    else { Err(AppError(StatusCode::FORBIDDEN, "Operator access required".into())) }
}
