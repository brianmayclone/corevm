//! Authentication API endpoints.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::jwt;
use crate::auth::middleware::{AuthUser, AppError};

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
    let (user_id, password_hash, role) = db.query_row(
        "SELECT id, password_hash, role FROM users WHERE username = ?1",
        rusqlite::params![&req.username],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
    ).map_err(|_| AppError(StatusCode::UNAUTHORIZED, "Invalid credentials".into()))?;

    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    let parsed_hash = PasswordHash::new(&password_hash)
        .map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "Internal error".into()))?;
    Argon2::default().verify_password(req.password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError(StatusCode::UNAUTHORIZED, "Invalid credentials".into()))?;

    let token = jwt::create_access_token(
        user_id, &req.username, &role,
        &state.jwt_secret, state.config.auth.session_timeout_hours,
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(LoginResponse {
        access_token: token,
        user: UserInfo { id: user_id, username: req.username, role },
    }))
}

/// GET /api/auth/me
pub async fn me(auth: AuthUser) -> Json<UserInfo> {
    Json(UserInfo { id: auth.id, username: auth.username, role: auth.role })
}
