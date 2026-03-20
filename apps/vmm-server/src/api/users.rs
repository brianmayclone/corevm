//! User management API endpoints (admin only).

use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};

#[derive(Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub created_at: String,
}

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

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// GET /api/users
pub async fn list(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<User>>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare("SELECT id, username, role, created_at FROM users ORDER BY id")
        .map_err(db_err)?;
    let users: Vec<User> = stmt.query_map([], |row| {
        Ok(User {
            id: row.get(0)?,
            username: row.get(1)?,
            role: row.get(2)?,
            created_at: row.get(3)?,
        })
    }).map_err(db_err)?
    .filter_map(|r| r.ok())
    .collect();
    Ok(Json(users))
}

/// POST /api/users
pub async fn create(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    if !["admin", "operator", "viewer"].contains(&req.role.as_str()) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid role".into()));
    }

    use argon2::PasswordHasher;
    let salt = argon2::password_hash::SaltString::generate(&mut rand::rngs::OsRng);
    let hash = argon2::Argon2::default()
        .hash_password(req.password.as_bytes(), &salt)
        .map_err(|e| db_err(e))?.to_string();

    let db = state.db.lock().unwrap();
    db.execute(
        "INSERT INTO users (username, password_hash, role) VALUES (?1, ?2, ?3)",
        rusqlite::params![&req.username, &hash, &req.role],
    ).map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError(StatusCode::CONFLICT, "Username already exists".into())
        } else { db_err(e) }
    })?;
    let id = db.last_insert_rowid();
    Ok(Json(serde_json::json!({"id": id, "username": req.username, "role": req.role})))
}

/// PUT /api/users/:id
pub async fn update(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    if let Some(ref username) = req.username {
        db.execute("UPDATE users SET username = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![username, user_id]).map_err(db_err)?;
    }
    if let Some(ref role) = req.role {
        if !["admin", "operator", "viewer"].contains(&role.as_str()) {
            return Err(AppError(StatusCode::BAD_REQUEST, "Invalid role".into()));
        }
        db.execute("UPDATE users SET role = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![role, user_id]).map_err(db_err)?;
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /api/users/:id
pub async fn delete(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    if auth.id == user_id {
        return Err(AppError(StatusCode::BAD_REQUEST, "Cannot delete yourself".into()));
    }
    let db = state.db.lock().unwrap();
    let affected = db.execute("DELETE FROM users WHERE id = ?1", rusqlite::params![user_id])
        .map_err(db_err)?;
    if affected == 0 {
        Err(AppError(StatusCode::NOT_FOUND, "User not found".into()))
    } else {
        Ok(Json(serde_json::json!({"ok": true})))
    }
}

/// PUT /api/users/:id/password
pub async fn change_password(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !auth.is_admin() && auth.id != user_id {
        return Err(AppError(StatusCode::FORBIDDEN, "Access denied".into()));
    }
    use argon2::PasswordHasher;
    let salt = argon2::password_hash::SaltString::generate(&mut rand::rngs::OsRng);
    let hash = argon2::Argon2::default()
        .hash_password(req.password.as_bytes(), &salt)
        .map_err(|e| db_err(e))?.to_string();

    let db = state.db.lock().unwrap();
    let affected = db.execute(
        "UPDATE users SET password_hash = ?1, updated_at = datetime('now') WHERE id = ?2",
        rusqlite::params![&hash, user_id],
    ).map_err(db_err)?;
    if affected == 0 {
        Err(AppError(StatusCode::NOT_FOUND, "User not found".into()))
    } else {
        Ok(Json(serde_json::json!({"ok": true})))
    }
}
