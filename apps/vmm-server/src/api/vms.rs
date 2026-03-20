//! VM management API endpoints.

use axum::{extract::{Path, State}, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use std::sync::Arc;

use crate::state::{AppState, VmInstance, VmState};
use crate::auth::middleware::{AuthUser, AppError, require_operator};
use crate::services::vm::VmService;
use vmm_core::config::VmConfig;

#[derive(Serialize)]
pub struct VmSummary {
    pub id: String,
    pub name: String,
    pub state: VmState,
    pub guest_os: String,
    pub ram_mb: u32,
    pub cpu_cores: u32,
    pub owner_id: i64,
}

#[derive(Serialize)]
pub struct VmDetail {
    pub id: String,
    pub name: String,
    pub state: VmState,
    pub config: VmConfig,
    pub owner_id: i64,
    pub created_at: String,
}

/// GET /api/vms
pub async fn list(_auth: AuthUser, State(state): State<Arc<AppState>>) -> Result<Json<Vec<VmSummary>>, AppError> {
    let db = state.db.lock().unwrap();
    let records = VmService::list(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let vms = records.into_iter().map(|r| {
        let vm_state = state.vms.get(&r.id).map(|v| v.state).unwrap_or(VmState::Stopped);
        VmSummary {
            id: r.id, name: r.name, state: vm_state,
            guest_os: r.config.guest_os.to_config_str().to_string(),
            ram_mb: r.config.ram_mb, cpu_cores: r.config.cpu_cores, owner_id: r.owner_id,
        }
    }).collect();
    Ok(Json(vms))
}

/// POST /api/vms
pub async fn create(auth: AuthUser, State(state): State<Arc<AppState>>, Json(mut config): Json<VmConfig>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    if config.uuid.is_empty() {
        config.uuid = uuid::Uuid::new_v4().to_string().replace("-", "");
    }
    let db = state.db.lock().unwrap();
    VmService::create(&db, &config, auth.id).map_err(|e| AppError(StatusCode::CONFLICT, e))?;
    state.vms.insert(config.uuid.clone(), VmInstance {
        id: config.uuid.clone(), config: config.clone(), state: VmState::Stopped,
        vm_handle: None, control: None, framebuffer: None, serial_tx: None, vm_thread: None,
    });
    Ok(Json(serde_json::json!({"id": config.uuid, "name": config.name})))
}

/// GET /api/vms/:id
pub async fn get(_auth: AuthUser, State(state): State<Arc<AppState>>, Path(vm_id): Path<String>) -> Result<Json<VmDetail>, AppError> {
    let db = state.db.lock().unwrap();
    let r = VmService::get(&db, &vm_id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    let vm_state = state.vms.get(&r.id).map(|v| v.state).unwrap_or(VmState::Stopped);
    Ok(Json(VmDetail { id: r.id, name: r.name, state: vm_state, config: r.config, owner_id: r.owner_id, created_at: r.created_at }))
}

/// PUT /api/vms/:id
pub async fn update(auth: AuthUser, State(state): State<Arc<AppState>>, Path(vm_id): Path<String>, Json(config): Json<VmConfig>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    if let Some(vm) = state.vms.get(&vm_id) {
        if vm.state != VmState::Stopped {
            return Err(AppError(StatusCode::CONFLICT, "VM must be stopped to update config".into()));
        }
    }
    let db = state.db.lock().unwrap();
    VmService::update(&db, &vm_id, &config).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    if let Some(mut vm) = state.vms.get_mut(&vm_id) { vm.config = config; }
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /api/vms/:id
pub async fn delete(auth: AuthUser, State(state): State<Arc<AppState>>, Path(vm_id): Path<String>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    if let Some(vm) = state.vms.get(&vm_id) {
        if vm.state != VmState::Stopped {
            return Err(AppError(StatusCode::CONFLICT, "VM must be stopped before deletion".into()));
        }
    }
    let db = state.db.lock().unwrap();
    VmService::delete(&db, &vm_id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    state.vms.remove(&vm_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /api/vms/:id/start
pub async fn start(auth: AuthUser, State(state): State<Arc<AppState>>, Path(vm_id): Path<String>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let config = {
        let vm = state.vms.get(&vm_id).ok_or_else(|| AppError(StatusCode::NOT_FOUND, "VM not found".into()))?;
        if vm.state != VmState::Stopped {
            return Err(AppError(StatusCode::CONFLICT, "VM is not stopped".into()));
        }
        vm.config.clone()
    };
    let bios_paths = state.config.vms.bios_search_paths.clone();
    let running = tokio::task::spawn_blocking(move || {
        crate::vm::manager::start_vm(&config, &bios_paths)
    }).await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Spawn error: {}", e)))?
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    if let Some(mut vm) = state.vms.get_mut(&vm_id) {
        vm.state = VmState::Running;
        vm.vm_handle = Some(running.handle);
        vm.control = Some(running.control);
        vm.framebuffer = Some(running.framebuffer);
        vm.serial_tx = Some(running.serial_tx);
        vm.vm_thread = Some(running.thread);
    }
    tracing::info!("VM {} started", vm_id);
    Ok(Json(serde_json::json!({"ok": true, "state": "running"})))
}

/// POST /api/vms/:id/stop
pub async fn stop(auth: AuthUser, State(state): State<Arc<AppState>>, Path(vm_id): Path<String>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let vm = state.vms.get(&vm_id).ok_or_else(|| AppError(StatusCode::NOT_FOUND, "VM not found".into()))?;
    if vm.state != VmState::Running { return Err(AppError(StatusCode::CONFLICT, "VM is not running".into())); }
    if let Some(ref control) = vm.control { control.request_stop(); }
    drop(vm);
    if let Some(mut vm) = state.vms.get_mut(&vm_id) { vm.state = VmState::Stopping; }
    tracing::info!("VM {} stop requested", vm_id);
    Ok(Json(serde_json::json!({"ok": true, "state": "stopping"})))
}

/// POST /api/vms/:id/force-stop
pub async fn force_stop(auth: AuthUser, State(state): State<Arc<AppState>>, Path(vm_id): Path<String>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    if !state.vms.contains_key(&vm_id) { return Err(AppError(StatusCode::NOT_FOUND, "VM not found".into())); }
    if let Some(mut vm) = state.vms.get_mut(&vm_id) {
        if let Some(ref control) = vm.control {
            control.set_exit_reason("Force stopped".into());
            control.set_exited();
        }
        vm.state = VmState::Stopped;
        vm.vm_handle = None; vm.control = None; vm.framebuffer = None;
        vm.serial_tx = None; vm.vm_thread = None;
    }
    tracing::info!("VM {} force-stopped", vm_id);
    Ok(Json(serde_json::json!({"ok": true, "state": "stopped"})))
}
