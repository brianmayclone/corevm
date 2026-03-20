//! Authentication API endpoints.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::middleware::{AuthUser, AppError};
use crate::services::auth::AuthService;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub user: UserInfo,
}

#[derive(Serialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub role: String,
}

/// POST /api/auth/login
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let db = state.db.lock().unwrap();
    let (user, token) = AuthService::login(
        &db, &req.username, &req.password,
        &state.jwt_secret, state.config.auth.session_timeout_hours,
    ).map_err(|e| AppError(StatusCode::UNAUTHORIZED, e))?;

    Ok(Json(LoginResponse {
        access_token: token,
        user: UserInfo { id: user.id, username: user.username, role: user.role },
    }))
}

/// GET /api/auth/me
pub async fn me(auth: AuthUser) -> Json<UserInfo> {
    Json(UserInfo { id: auth.id, username: auth.username, role: auth.role })
}
