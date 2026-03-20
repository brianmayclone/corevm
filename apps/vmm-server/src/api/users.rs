//! User management API endpoints (admin only).

use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::user::UserService;

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default = "default_role")]
    pub role: String,
}
fn default_role() -> String { "operator".into() }

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub role: Option<String>,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub password: String,
}

/// GET /api/users
pub async fn list(auth: AuthUser, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    let users = UserService::list(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(users).unwrap()))
}

/// POST /api/users
pub async fn create(auth: AuthUser, State(state): State<Arc<AppState>>, Json(req): Json<CreateUserRequest>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    let id = UserService::create(&db, &req.username, &req.password, &req.role)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"id": id, "username": req.username, "role": req.role})))
}

/// PUT /api/users/:id
pub async fn update(auth: AuthUser, State(state): State<Arc<AppState>>, Path(user_id): Path<i64>, Json(req): Json<UpdateUserRequest>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    UserService::update(&db, user_id, req.username.as_deref(), req.role.as_deref())
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /api/users/:id
pub async fn delete(auth: AuthUser, State(state): State<Arc<AppState>>, Path(user_id): Path<i64>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    if auth.id == user_id { return Err(AppError(StatusCode::BAD_REQUEST, "Cannot delete yourself".into())); }
    let db = state.db.lock().unwrap();
    UserService::delete(&db, user_id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// PUT /api/users/:id/password
pub async fn change_password(auth: AuthUser, State(state): State<Arc<AppState>>, Path(user_id): Path<i64>, Json(req): Json<ChangePasswordRequest>) -> Result<Json<serde_json::Value>, AppError> {
    if !auth.is_admin() && auth.id != user_id {
        return Err(AppError(StatusCode::FORBIDDEN, "Access denied".into()));
    }
    let db = state.db.lock().unwrap();
    UserService::change_password(&db, user_id, &req.password)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}
