//! Agent API handlers — executed on behalf of the cluster.
//!
//! These handlers are thin wrappers that delegate to existing services.
//! All require AgentAuth (X-Agent-Token validation).

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use std::sync::Arc;
use crate::state::AppState;
use crate::auth::middleware::AppError;
use crate::agent::auth::AgentAuth;
use vmm_core::cluster::*;

/// GET /agent/status — Full host status (heartbeat payload).
pub async fn status(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
) -> Result<Json<HostStatus>, AppError> {
    let managed = state.managed_config.lock()
        .map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "Lock error".into()))?;
    let node_id = managed.as_ref().map(|c| c.node_id.clone()).unwrap_or_default();

    let hostname = gethostname::gethostname().to_string_lossy().to_string();
    let (total_ram, free_ram) = get_memory_mb();
    let cpu_usage = get_cpu_usage();

    // Collect VM states
    let vms: Vec<AgentVmStatus> = state.vms.iter().map(|entry| {
        let vm = entry.value();
        AgentVmStatus {
            id: vm.id.clone(),
            state: format!("{:?}", vm.state).to_lowercase(),
            cpu_usage_pct: 0.0, // TODO: per-VM CPU tracking
            ram_used_mb: vm.config.ram_mb,
            uptime_secs: 0, // TODO: track VM start time
        }
    }).collect();

    // Collect datastore/storage pool status
    let datastores = Vec::new(); // TODO: Phase 3 — report mounted datastores

    Ok(Json(HostStatus {
        node_id,
        hostname,
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        hardware: HostHardwareInfo {
            cpu_model: get_cpu_model(),
            cpu_cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1),
            cpu_threads: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1),
            total_ram_mb: total_ram,
            hw_virtualization: libcorevm::ffi::corevm_has_hw_support() != 0,
        },
        free_ram_mb: free_ram,
        cpu_usage_pct: cpu_usage,
        vms,
        datastores,
    }))
}

/// GET /agent/hardware — Static hardware info.
pub async fn hardware(
    _agent: AgentAuth,
) -> Json<HostHardwareInfo> {
    let (total_ram, _) = get_memory_mb();
    Json(HostHardwareInfo {
        cpu_model: get_cpu_model(),
        cpu_cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1),
        cpu_threads: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1),
        total_ram_mb: total_ram,
        hw_virtualization: libcorevm::ffi::corevm_has_hw_support() != 0,
    })
}

/// GET /agent/vms — List all VMs on this host.
pub async fn list_vms(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
) -> Json<Vec<AgentVmStatus>> {
    let vms: Vec<AgentVmStatus> = state.vms.iter().map(|entry| {
        let vm = entry.value();
        AgentVmStatus {
            id: vm.id.clone(),
            state: format!("{:?}", vm.state).to_lowercase(),
            cpu_usage_pct: 0.0,
            ram_used_mb: vm.config.ram_mb,
            uptime_secs: 0,
        }
    }).collect();
    Json(vms)
}

/// GET /agent/vms/{id}/screenshot — Framebuffer screenshot.
pub async fn screenshot(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    Path(id): Path<String>,
) -> Result<axum::response::Response, AppError> {
    use axum::response::IntoResponse;

    let vm = state.vms.get(&id)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "VM not found".into()))?;
    let fb = vm.framebuffer.as_ref()
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "No framebuffer".into()))?
        .clone();
    drop(vm);

    let fb_lock = fb.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "FB lock".into()))?;
    if fb_lock.width == 0 || fb_lock.height == 0 || fb_lock.pixels.is_empty() {
        return Err(AppError(StatusCode::NOT_FOUND, "No framebuffer data".into()));
    }

    let mut png_buf: Vec<u8> = Vec::new();
    {
        use image::ImageEncoder;
        let encoder = image::codecs::png::PngEncoder::new(&mut png_buf);
        encoder.write_image(&fb_lock.pixels, fb_lock.width, fb_lock.height, image::ExtendedColorType::Rgba8)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("PNG encode: {}", e)))?;
    }

    Ok(([(axum::http::header::CONTENT_TYPE, "image/png")], png_buf).into_response())
}

/// GET /agent/storage/pools — List storage pools.
pub async fn list_storage_pools(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let pools = crate::services::storage::StorageService::list_pools(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(pools).unwrap()))
}

/// POST /agent/vms/provision — Create/provision a VM on this host.
pub async fn provision_vm(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    Json(req): Json<ProvisionVmRequest>,
) -> Result<Json<ProvisionVmResponse>, AppError> {
    let config: vmm_core::config::VmConfig = serde_json::from_value(req.config)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Invalid VM config: {}", e)))?;

    // Save to local DB
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let config_json = serde_json::to_string(&config)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    db.execute(
        "INSERT OR REPLACE INTO vms (id, name, config_json) VALUES (?1, ?2, ?3)",
        rusqlite::params![&req.vm_id, &config.name, &config_json],
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Add to in-memory state
    state.vms.insert(req.vm_id.clone(), crate::state::VmInstance {
        id: req.vm_id.clone(),
        config,
        state: crate::state::VmState::Stopped,
        vm_handle: None, control: None, framebuffer: None,
        serial_tx: None, vm_thread: None,
    });

    Ok(Json(ProvisionVmResponse {
        vm_id: req.vm_id,
        success: true,
        error: None,
    }))
}

/// POST /agent/vms/{id}/start — Start a VM.
pub async fn start_vm(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    Path(id): Path<String>,
) -> Result<Json<AgentResponse>, AppError> {
    use crate::state::VmState;

    let config = {
        let vm = state.vms.get(&id)
            .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "VM not found".into()))?;
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

    let control_for_watcher = running.control.clone();

    if let Some(mut vm) = state.vms.get_mut(&id) {
        vm.state = VmState::Running;
        vm.vm_handle = Some(running.handle);
        vm.control = Some(running.control);
        vm.framebuffer = Some(running.framebuffer);
        vm.serial_tx = Some(running.serial_tx);
        vm.vm_thread = Some(running.thread);
    }

    // Watcher task to detect VM exit
    let watcher_state = state.clone();
    let watcher_id = id.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if control_for_watcher.is_exited() { break; }
        }
        if let Some(mut vm) = watcher_state.vms.get_mut(&watcher_id) {
            vm.state = VmState::Stopped;
            vm.vm_handle = None; vm.control = None; vm.framebuffer = None;
            vm.serial_tx = None; vm.vm_thread = None;
        }
    });

    Ok(Json(AgentResponse::ok()))
}

/// POST /agent/vms/{id}/stop — Graceful stop.
pub async fn stop_vm(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    Path(id): Path<String>,
) -> Result<Json<AgentResponse>, AppError> {
    use crate::state::VmState;

    let vm = state.vms.get(&id)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "VM not found".into()))?;
    if vm.state != VmState::Running {
        return Err(AppError(StatusCode::CONFLICT, "VM is not running".into()));
    }
    if let Some(ref control) = vm.control { control.request_stop(); }
    drop(vm);
    if let Some(mut vm) = state.vms.get_mut(&id) { vm.state = VmState::Stopping; }
    Ok(Json(AgentResponse::ok()))
}

/// POST /agent/vms/{id}/force-stop — Hard stop.
pub async fn force_stop_vm(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    Path(id): Path<String>,
) -> Result<Json<AgentResponse>, AppError> {
    use crate::state::VmState;

    if let Some(mut vm) = state.vms.get_mut(&id) {
        if let Some(ref control) = vm.control {
            control.set_exit_reason("Force stopped by cluster".into());
            control.set_exited();
        }
        vm.state = VmState::Stopped;
        vm.vm_handle = None; vm.control = None; vm.framebuffer = None;
        vm.serial_tx = None; vm.vm_thread = None;
    }
    Ok(Json(AgentResponse::ok()))
}

/// POST /agent/vms/{id}/destroy — Remove VM completely.
pub async fn destroy_vm(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    Path(id): Path<String>,
) -> Result<Json<AgentResponse>, AppError> {
    use crate::state::VmState;

    // Force-stop if running
    if let Some(mut vm) = state.vms.get_mut(&id) {
        if let Some(ref control) = vm.control {
            control.set_exit_reason("Destroyed by cluster".into());
            control.set_exited();
        }
        vm.state = VmState::Stopped;
        vm.vm_handle = None; vm.control = None; vm.framebuffer = None;
        vm.serial_tx = None; vm.vm_thread = None;
    }
    // Remove from memory
    state.vms.remove(&id);
    // Remove from local DB
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let _ = db.execute("DELETE FROM vms WHERE id = ?1", rusqlite::params![&id]);
    Ok(Json(AgentResponse::ok()))
}

/// PUT /agent/vms/{id}/config — Update VM configuration.
pub async fn update_vm_config(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    Path(id): Path<String>,
    Json(config): Json<serde_json::Value>,
) -> Result<Json<AgentResponse>, AppError> {
    let vm_config: vmm_core::config::VmConfig = serde_json::from_value(config.clone())
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Invalid config: {}", e)))?;

    // Update in local DB
    let config_json = serde_json::to_string(&vm_config)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    db.execute(
        "UPDATE vms SET config_json = ?1, name = ?2 WHERE id = ?3",
        rusqlite::params![&config_json, &vm_config.name, &id],
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Update in memory
    if let Some(mut vm) = state.vms.get_mut(&id) {
        vm.config = vm_config;
    }

    Ok(Json(AgentResponse::ok()))
}

/// POST /agent/storage/mount — Mount a datastore.
pub async fn mount_datastore(
    _agent: AgentAuth,
    Json(req): Json<MountDatastoreRequest>,
) -> Result<Json<AgentResponse>, AppError> {
    // Create mount point
    std::fs::create_dir_all(&req.mount_path)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot create mount point: {}", e)))?;

    // Execute mount command
    let mount_args = match req.store_type.as_str() {
        "nfs" => format!("-t nfs -o {} {} {}", req.mount_opts, req.mount_source, req.mount_path),
        "cephfs" => format!("-t ceph -o {} {} {}", req.mount_opts, req.mount_source, req.mount_path),
        "glusterfs" => format!("-t glusterfs -o {} {} {}", req.mount_opts, req.mount_source, req.mount_path),
        _ => return Err(AppError(StatusCode::BAD_REQUEST, format!("Unsupported store type: {}", req.store_type))),
    };

    let output = std::process::Command::new("mount")
        .args(mount_args.split_whitespace())
        .output()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Mount failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(Json(AgentResponse::err(format!("Mount failed: {}", stderr))));
    }

    Ok(Json(AgentResponse::ok()))
}

/// POST /agent/storage/unmount — Unmount a datastore.
pub async fn unmount_datastore(
    _agent: AgentAuth,
    Json(req): Json<UnmountDatastoreRequest>,
) -> Result<Json<AgentResponse>, AppError> {
    let output = std::process::Command::new("umount")
        .arg(&req.mount_path)
        .output()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Unmount failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(Json(AgentResponse::err(format!("Unmount failed: {}", stderr))));
    }

    Ok(Json(AgentResponse::ok()))
}

/// POST /agent/storage/create-disk — Create a disk image.
pub async fn create_disk(
    _agent: AgentAuth,
    Json(req): Json<CreateDiskRequest>,
) -> Result<Json<AgentResponse>, AppError> {
    // Create parent directory if needed
    if let Some(parent) = std::path::Path::new(&req.path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Create raw disk image (sparse file)
    let f = std::fs::File::create(&req.path)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot create disk: {}", e)))?;
    f.set_len(req.size_bytes)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot set disk size: {}", e)))?;

    Ok(Json(AgentResponse::ok()))
}

/// DELETE /agent/storage/delete-disk — Delete a disk image.
pub async fn delete_disk(
    _agent: AgentAuth,
    Json(path): Json<String>,
) -> Result<Json<AgentResponse>, AppError> {
    std::fs::remove_file(&path)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot delete disk: {}", e)))?;
    Ok(Json(AgentResponse::ok()))
}

// ── System info helpers ─────────────────────────────────────────────────

fn get_memory_mb() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            let mut total = 0u64;
            let mut avail = 0u64;
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    total = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if line.starts_with("MemAvailable:") {
                    avail = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                }
            }
            return (total / 1024, avail / 1024);
        }
    }
    (0, 0)
}

fn get_cpu_usage() -> f32 {
    // Simplified — return 0.0, real implementation would read /proc/stat
    0.0
}

fn get_cpu_model() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in content.lines() {
                if line.starts_with("model name") {
                    if let Some(name) = line.split(':').nth(1) {
                        return name.trim().to_string();
                    }
                }
            }
        }
    }
    "Unknown CPU".to_string()
}
