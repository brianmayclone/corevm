//! Cluster settings API — SMTP config, LDAP, DRS exclusions, host rename.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::settings::ClusterSettingsService;
use crate::services::ldap::LdapService;
use crate::services::audit::AuditService;

// ── SMTP Settings ───────────────────────────────────────────────────────

pub async fn get_smtp(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let config = ClusterSettingsService::get_smtp_config(&db);
    // Don't expose password in GET
    Ok(Json(serde_json::json!({
        "host": config.host, "port": config.port,
        "username": config.username, "from_address": config.from_address,
        "use_tls": config.use_tls,
        "configured": !config.host.is_empty(),
    })))
}

pub async fn set_smtp(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<crate::services::settings::SmtpConfig>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    ClusterSettingsService::set_smtp_config(&db, &body)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    AuditService::log(&db, user.id, "settings.smtp.update", "settings", "smtp", None);
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Host Rename ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RenameHostRequest {
    pub display_name: String,
}

pub async fn rename_host(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<RenameHostRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    db.execute("UPDATE hosts SET display_name = ?1 WHERE id = ?2",
        rusqlite::params![&body.display_name, &id])
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    AuditService::log(&db, user.id, "host.rename", "host", &id, Some(&body.display_name));
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── DRS Exclusions ──────────────────────────────────────────────────────

pub async fn list_drs_exclusions(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let mut stmt = db.prepare(
        "SELECT id, cluster_id, exclusion_type, target_id, reason, created_at FROM drs_exclusions ORDER BY created_at DESC"
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, i64>(0)?,
            "cluster_id": row.get::<_, String>(1)?,
            "exclusion_type": row.get::<_, String>(2)?,
            "target_id": row.get::<_, String>(3)?,
            "reason": row.get::<_, String>(4)?,
            "created_at": row.get::<_, String>(5)?,
        }))
    }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let exclusions: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Ok(Json(serde_json::Value::Array(exclusions)))
}

#[derive(Deserialize)]
pub struct CreateDrsExclusionRequest {
    pub cluster_id: String,
    pub exclusion_type: String,  // "vm" or "resource_group"
    pub target_id: String,
    #[serde(default)]
    pub reason: String,
}

pub async fn create_drs_exclusion(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateDrsExclusionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    if !["vm", "resource_group"].contains(&body.exclusion_type.as_str()) {
        return Err(AppError(StatusCode::BAD_REQUEST, "exclusion_type must be 'vm' or 'resource_group'".into()));
    }
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    db.execute(
        "INSERT INTO drs_exclusions (cluster_id, exclusion_type, target_id, reason) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![&body.cluster_id, &body.exclusion_type, &body.target_id, &body.reason],
    ).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let id = db.last_insert_rowid();
    AuditService::log(&db, user.id, "drs.exclusion.create", &body.exclusion_type, &body.target_id, Some(&body.reason));
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn delete_drs_exclusion(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    db.execute("DELETE FROM drs_exclusions WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    AuditService::log(&db, user.id, "drs.exclusion.delete", "drs_exclusion", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── LDAP / Active Directory ─────────────────────────────────────────────

pub async fn list_ldap(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let configs = LdapService::list(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(configs).unwrap()))
}

#[derive(Deserialize)]
pub struct CreateLdapRequest {
    pub name: String,
    pub server_url: String,
    pub base_dn: String,
}

pub async fn create_ldap(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateLdapRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let id = LdapService::create(&db, &body.name, &body.server_url, &body.base_dn)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "ldap.create", "ldap", &id.to_string(), Some(&body.name));
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn update_ldap(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    LdapService::update(&db, id, &body).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "ldap.update", "ldap", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn delete_ldap(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    LdapService::delete(&db, id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "ldap.delete", "ldap", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn test_ldap(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let msg = LdapService::test_connection(&db, id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true, "message": msg})))
}
