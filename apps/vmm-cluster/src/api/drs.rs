//! DRS API — view and manage DRS recommendations and rules.
//!
//! Thin handlers delegating to DrsService.

use axum::{Json, extract::{State, Path, Query}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::drs_service::DrsService;
use crate::services::audit::AuditService;

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let recs = DrsService::list_pending(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(recs).unwrap()))
}

/// POST /api/drs/{id}/apply — Apply a DRS recommendation (trigger migration).
pub async fn apply(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;

    let (vm_id, target_host_id) = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        DrsService::get_apply_target(&db, id)
            .map_err(|e| AppError(StatusCode::NOT_FOUND, e))?
    };

    {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        DrsService::mark_applied(&db, id)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        AuditService::log(&db, user.id, "drs.apply", "drs_recommendation", &id.to_string(), None);
    }

    let state_clone = state.clone();
    tokio::spawn(async move {
        crate::services::migration::MigrationService::migrate_vm(
            &state_clone, &vm_id, &target_host_id, "drs", Some(user.id),
        ).await;
    });

    Ok(Json(serde_json::json!({"ok": true, "action": "migration_started"})))
}

/// POST /api/drs/{id}/dismiss — Dismiss a DRS recommendation.
pub async fn dismiss(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    DrsService::dismiss(&db, id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    AuditService::log(&db, user.id, "drs.dismiss", "drs_recommendation", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── DRS Rules ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RulesQuery {
    pub cluster_id: Option<String>,
}

pub async fn list_rules(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Query(q): Query<RulesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let rules = DrsService::list_rules(&db, q.cluster_id.as_deref())
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(rules).unwrap()))
}

#[derive(Deserialize)]
pub struct CreateRuleRequest {
    pub cluster_id: String,
    pub name: String,
    #[serde(default = "default_metric")]
    pub metric: String,
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: i64,
    #[serde(default = "default_priority")]
    pub priority: String,
}
fn default_metric() -> String { "cpu_usage".into() }
fn default_threshold() -> f64 { 80.0 }
fn default_action() -> String { "recommend".into() }
fn default_cooldown() -> i64 { 3600 }
fn default_priority() -> String { "medium".into() }

pub async fn create_rule(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateRuleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let id = DrsService::create_rule(&db, &body.cluster_id, &body.name, &body.metric,
        body.threshold, &body.action, body.cooldown_secs, &body.priority)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "drs.rule.create", "drs_rule", &id.to_string(), Some(&body.name));
    Ok(Json(serde_json::json!({"id": id})))
}

#[derive(Deserialize)]
pub struct UpdateRuleRequest {
    pub enabled: Option<bool>,
    pub threshold: Option<f64>,
    pub action: Option<String>,
    pub priority: Option<String>,
}

pub async fn update_rule(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateRuleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    DrsService::update_rule(&db, id, body.enabled, body.threshold,
        body.action.as_deref(), body.priority.as_deref())
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "drs.rule.update", "drs_rule", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn delete_rule(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    DrsService::delete_rule(&db, id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "drs.rule.delete", "drs_rule", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}
