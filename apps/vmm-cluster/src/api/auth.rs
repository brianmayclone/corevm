//! Auth API handlers — login, current user info.

use axum::{Json, extract::State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::state::ClusterState;
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

pub async fn login(
    State(state): State<Arc<ClusterState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let (user, token) = AuthService::login(
        &db, &body.username, &body.password,
        &state.jwt_secret, state.config.auth.session_timeout_hours,
    ).map_err(|e| AppError(StatusCode::UNAUTHORIZED, e))?;

    Ok(Json(LoginResponse {
        access_token: token,
        user: UserInfo { id: user.id, username: user.username, role: user.role },
    }))
}

pub async fn me(user: AuthUser) -> Json<UserInfo> {
    Json(UserInfo { id: user.id, username: user.username, role: user.role })
}
