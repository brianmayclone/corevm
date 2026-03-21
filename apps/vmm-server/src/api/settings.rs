//! Settings API — server configuration, time, security.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};

// ── Server Settings ──────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ServerSettings {
    pub bind: String,
    pub port: u16,
    pub session_timeout_hours: u64,
    pub max_disk_size_gb: u64,
    pub log_level: String,
    pub version: String,
    pub uptime_secs: u64,
}

/// GET /api/settings/server
pub async fn get_server(auth: AuthUser, State(state): State<Arc<AppState>>) -> Result<Json<ServerSettings>, AppError> {
    require_admin(&auth)?;
    let cfg = &state.config;
    let uptime = state.started_at.elapsed().as_secs();
    Ok(Json(ServerSettings {
        bind: cfg.server.bind.clone(),
        port: cfg.server.port,
        session_timeout_hours: cfg.auth.session_timeout_hours,
        max_disk_size_gb: cfg.storage.max_disk_size_gb,
        log_level: cfg.logging.level.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: uptime,
    }))
}

// ── Date & Time ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TimeSettings {
    pub current_time: String,
    pub timezone: String,
    pub ntp_enabled: bool,
    pub ntp_servers: Vec<String>,
}

/// GET /api/settings/time
pub async fn get_time(auth: AuthUser) -> Result<Json<TimeSettings>, AppError> {
    require_admin(&auth)?;

    let now = chrono::Local::now();
    let tz = std::env::var("TZ").unwrap_or_else(|_| {
        std::fs::read_to_string("/etc/timezone")
            .unwrap_or_else(|_| "UTC".into()).trim().to_string()
    });

    // Check NTP status
    let ntp_enabled = std::process::Command::new("timedatectl")
        .arg("show").arg("--property=NTP").arg("--value")
        .output().ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim() == "yes")
        .unwrap_or(false);

    // Read NTP servers from timesyncd config
    let ntp_servers = std::fs::read_to_string("/etc/systemd/timesyncd.conf")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("NTP=") || l.starts_with("#NTP="))
        .and_then(|l| l.split('=').nth(1))
        .map(|s| s.split_whitespace().map(String::from).collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["ntp.ubuntu.com".into(), "pool.ntp.org".into()]);

    Ok(Json(TimeSettings {
        current_time: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        timezone: tz,
        ntp_enabled,
        ntp_servers,
    }))
}

#[derive(Deserialize)]
pub struct SetTimezone {
    pub timezone: String,
}

/// PUT /api/settings/time/timezone
pub async fn set_timezone(auth: AuthUser, Json(req): Json<SetTimezone>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    std::env::set_var("TZ", &req.timezone);
    Ok(Json(serde_json::json!({"ok": true, "timezone": req.timezone})))
}

// ── Security Settings ────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SecuritySettings {
    pub max_login_attempts: u32,
    pub lockout_duration_secs: u64,
    pub password_min_length: u32,
    pub require_uppercase: bool,
    pub require_numbers: bool,
    pub api_keys_enabled: bool,
}

/// GET /api/settings/security
pub async fn get_security(auth: AuthUser) -> Result<Json<SecuritySettings>, AppError> {
    require_admin(&auth)?;
    Ok(Json(SecuritySettings {
        max_login_attempts: 5,
        lockout_duration_secs: 300,
        password_min_length: 8,
        require_uppercase: true,
        require_numbers: true,
        api_keys_enabled: false,
    }))
}

// ── Groups ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub role: String,
    pub description: String,
    pub member_count: i64,
}

/// GET /api/settings/groups
pub async fn list_groups(auth: AuthUser, State(state): State<Arc<AppState>>) -> Result<Json<Vec<Group>>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();

    let mut stmt = db.prepare(
        "SELECT g.id, g.name, g.role, g.description, COUNT(gm.user_id) as member_count \
         FROM groups g LEFT JOIN group_members gm ON g.id = gm.group_id \
         GROUP BY g.id ORDER BY g.name"
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let groups = stmt.query_map([], |row| {
        Ok(Group {
            id: row.get(0)?, name: row.get(1)?, role: row.get(2)?,
            description: row.get(3)?, member_count: row.get(4)?,
        })
    }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .filter_map(|r| r.ok()).collect();

    Ok(Json(groups))
}

#[derive(Deserialize)]
pub struct CreateGroup {
    pub name: String,
    pub role: String,
    pub description: String,
}

/// POST /api/settings/groups
pub async fn create_group(auth: AuthUser, State(state): State<Arc<AppState>>, Json(req): Json<CreateGroup>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    db.execute(
        "INSERT INTO groups (name, role, description) VALUES (?1, ?2, ?3)",
        rusqlite::params![req.name, req.role, req.description],
    ).map_err(|e| AppError(StatusCode::CONFLICT, e.to_string()))?;
    let id = db.last_insert_rowid();
    Ok(Json(serde_json::json!({"id": id, "name": req.name})))
}

/// DELETE /api/settings/groups/:id
pub async fn delete_group(auth: AuthUser, State(state): State<Arc<AppState>>, axum::extract::Path(group_id): axum::extract::Path<i64>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    db.execute("DELETE FROM groups WHERE id = ?1", rusqlite::params![group_id])
        .map_err(|e| AppError(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(serde_json::json!({"ok": true})))
}
