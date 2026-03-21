//! Notifications API — manage channels, rules, and view notification log.
//!
//! Thin handlers delegating to NotificationService.

use axum::{Json, extract::{State, Path, Query}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::notification::NotificationService;
use crate::services::audit::AuditService;

// ── Channels ────────────────────────────────────────────────────────────

pub async fn list_channels(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let channels = NotificationService::list_channels(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(channels).unwrap()))
}

#[derive(Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub channel_type: String,
    #[serde(default = "empty_json")]
    pub config: serde_json::Value,
}
fn empty_json() -> serde_json::Value { serde_json::json!({}) }

pub async fn create_channel(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateChannelRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let config_str = serde_json::to_string(&body.config).unwrap_or_else(|_| "{}".into());
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let id = NotificationService::create_channel(&db, &body.name, &body.channel_type, &config_str)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "notification.channel.create", "channel", &id.to_string(), Some(&body.name));
    Ok(Json(serde_json::json!({"id": id})))
}

#[derive(Deserialize)]
pub struct UpdateChannelRequest {
    pub enabled: Option<bool>,
    pub config: Option<serde_json::Value>,
}

pub async fn update_channel(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateChannelRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let config_str = body.config.as_ref().map(|c| serde_json::to_string(c).unwrap_or_default());
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    NotificationService::update_channel(&db, id, body.enabled, config_str.as_deref())
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn delete_channel(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    NotificationService::delete_channel(&db, id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "notification.channel.delete", "channel", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn test_channel(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let msg = NotificationService::test_channel(&db, id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true, "message": msg})))
}

// ── Rules ───────────────────────────────────────────────────────────────

pub async fn list_rules(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let rules = NotificationService::list_rules(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(rules).unwrap()))
}

#[derive(Deserialize)]
pub struct CreateRuleRequest {
    pub name: String,
    #[serde(default = "default_category")]
    pub event_category: String,
    #[serde(default = "default_severity")]
    pub min_severity: String,
    pub channel_id: i64,
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: i64,
    pub cluster_id: Option<String>,
}
fn default_category() -> String { "*".into() }
fn default_severity() -> String { "warning".into() }
fn default_cooldown() -> i64 { 300 }

pub async fn create_rule(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateRuleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let id = NotificationService::create_rule(&db, &body.name, &body.event_category,
        &body.min_severity, body.channel_id, body.cooldown_secs, body.cluster_id.as_deref())
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "notification.rule.create", "rule", &id.to_string(), Some(&body.name));
    Ok(Json(serde_json::json!({"id": id})))
}

#[derive(Deserialize)]
pub struct UpdateRuleRequest {
    pub enabled: Option<bool>,
}

pub async fn update_rule(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateRuleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    NotificationService::update_rule(&db, id, body.enabled)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn delete_rule(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    NotificationService::delete_rule(&db, id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "notification.rule.delete", "rule", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Log ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LogQuery {
    #[serde(default = "default_log_limit")]
    pub limit: u32,
}
fn default_log_limit() -> u32 { 50 }

pub async fn notification_log(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Query(q): Query<LogQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let log = NotificationService::recent_log(&db, q.limit)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(log).unwrap()))
}
