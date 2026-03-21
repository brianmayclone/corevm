//! User management API handlers — thin layer delegating to UserService.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::user::UserService;
use crate::services::audit::AuditService;

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: String,
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub role: Option<String>,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub password: String,
}

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let users = UserService::list(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(users).unwrap()))
}

pub async fn create(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let id = UserService::create(&db, &body.username, &body.password, &body.role)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "user.create", "user", &id.to_string(), Some(&body.username));
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn update(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    UserService::update(&db, id, body.username.as_deref(), body.role.as_deref())
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "user.update", "user", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn delete(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    UserService::delete(&db, id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "user.delete", "user", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn change_password(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    UserService::change_password(&db, id, &body.password)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "user.change_password", "user", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}
