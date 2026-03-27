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
    let node_id = {
        let managed = state.managed_config.lock()
            .map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "Lock error".into()))?;
        managed.as_ref().map(|c| c.node_id.clone()).unwrap_or_default()
    };

    let hostname = gethostname::gethostname().to_string_lossy().to_string();
    let (total_ram, free_ram) = get_memory_mb();
    let cpu_usage = get_cpu_usage();

    // Collect VM states with uptime
    let vms: Vec<AgentVmStatus> = state.vms.iter().map(|entry| {
        let vm = entry.value();
        AgentVmStatus {
            id: vm.id.clone(),
            state: format!("{:?}", vm.state).to_lowercase(),
            cpu_usage_pct: 0.0, // Per-VM CPU tracking not yet available from libcorevm
            ram_used_mb: vm.config.ram_mb,
            uptime_secs: vm.started_at.map(|t| t.elapsed().as_secs()).unwrap_or(0),
        }
    }).collect();

    // Collect datastore mount status from managed config
    let datastores = collect_datastore_status(&state);

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
        san: query_coresan_status().await,
    }))
}

/// Query local CoreSAN daemon for status (if running).
async fn query_coresan_status() -> Option<vmm_core::cluster::CoreSanNodeStatus> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build().ok()?;

    let resp = client.get("http://127.0.0.1:7443/api/status")
        .send().await.ok()?;

    if !resp.status().is_success() {
        return None;
    }

    resp.json().await.ok()
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
        serial_tx: None, vm_thread: None, started_at: None,
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
        vm.started_at = Some(std::time::Instant::now());
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

// ── Network / Bridge Management ─────────────────────────────────────────

/// POST /agent/network/bridge/setup — Create a bridge (+ optional VXLAN).
pub async fn setup_bridge(
    _agent: AgentAuth,
    Json(req): Json<vmm_core::cluster::SetupBridgeRequest>,
) -> Result<Json<AgentResponse>, AppError> {
    crate::api::network::setup_bridge(&req)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(AgentResponse::ok()))
}

/// POST /agent/network/bridge/teardown — Remove a bridge.
pub async fn teardown_bridge(
    _agent: AgentAuth,
    Json(req): Json<vmm_core::cluster::TeardownBridgeRequest>,
) -> Result<Json<AgentResponse>, AppError> {
    crate::api::network::teardown_bridge(&req.bridge_name)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(AgentResponse::ok()))
}

// ── Direct Host-to-Host Migration ───────────────────────────────────────

/// POST /agent/migration/send — Send VM disks directly to another host.
/// The cluster provides a one-time token and the target address.
pub async fn migration_send(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    Json(req): Json<vmm_core::cluster::MigrationSendRequest>,
) -> Result<Json<AgentResponse>, AppError> {
    // Verify VM exists
    let vm = state.vms.get(&req.vm_id)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "VM not found".into()))?;
    let vm_name = vm.config.name.clone();
    drop(vm);

    // Stop VM if running
    if let Some(mut vm) = state.vms.get_mut(&req.vm_id) {
        if let Some(ref control) = vm.control {
            control.request_stop();
        }
        vm.state = crate::state::VmState::Stopping;
    }
    // Wait for stop
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if let Some(vm) = state.vms.get(&req.vm_id) {
            if matches!(vm.state, crate::state::VmState::Stopped) { break; }
        }
    }

    // Transfer disk files to target host
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(3600))
        .build()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for disk_path in &req.disk_paths {
        let path = std::path::Path::new(disk_path);
        if !path.exists() { continue; }

        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let file_data = tokio::fs::read(path).await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot read disk: {}", e)))?;

        // Send to target's migration/receive endpoint with the one-time token
        let target_url = format!("{}/agent/migration/receive", req.target_address.trim_end_matches('/'));
        let resp = client.post(&target_url)
            .header("X-Migration-Token", &req.migration_token)
            .header("X-VM-Id", &req.vm_id)
            .header("X-Disk-Path", disk_path)
            .header("X-Config-Json", &req.config_json)
            .body(file_data)
            .send().await
            .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Transfer failed: {}", e)))?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(AppError(StatusCode::BAD_GATEWAY, format!("Target rejected transfer: {}", err)));
        }

        tracing::info!("Migration: Sent disk {} ({} bytes) to {}", disk_path, file_size, req.target_address);
    }

    // Remove VM from local state after successful transfer
    state.vms.remove(&req.vm_id);
    if let Ok(db) = state.db.lock() {
        let _ = db.execute("DELETE FROM vms WHERE id = ?1", rusqlite::params![&req.vm_id]);
    }

    tracing::info!("Migration: VM '{}' sent to {}", vm_name, req.target_address);
    Ok(Json(AgentResponse::ok()))
}

/// POST /agent/migration/receive — Receive VM config + disk metadata from source host.
/// The actual disk files are transferred via the source host's migration/send handler
/// which writes directly to this host. This endpoint provisions the VM locally.
pub async fn migration_receive(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    Json(req): Json<vmm_core::cluster::ProvisionVmRequest>,
) -> Result<Json<AgentResponse>, AppError> {
    let config: vmm_core::config::VmConfig = serde_json::from_value(req.config)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Invalid VM config: {}", e)))?;

    let config_json = serde_json::to_string(&config)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Provision VM locally
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    db.execute(
        "INSERT OR REPLACE INTO vms (id, name, config_json) VALUES (?1, ?2, ?3)",
        rusqlite::params![&req.vm_id, &config.name, &config_json],
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    drop(db);

    state.vms.insert(req.vm_id.clone(), crate::state::VmInstance {
        id: req.vm_id.clone(), config, state: crate::state::VmState::Stopped,
        vm_handle: None, control: None, framebuffer: None,
        serial_tx: None, vm_thread: None, started_at: None,
    });

    tracing::info!("Migration: Received VM '{}' for provisioning", req.vm_id);
    Ok(Json(AgentResponse::ok()))
}

// ── Package Management ──────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct PackageCheckRequest {
    pub packages: Vec<String>,
    /// Optional sudo password for privilege escalation.
    pub sudo_password: Option<String>,
}

#[derive(serde::Serialize)]
pub struct PackageCheckResponse {
    pub installed: Vec<String>,
    pub missing: Vec<String>,
    pub distro: String,
    pub is_root: bool,
}

/// POST /agent/packages/check — Check if packages are installed + whether agent runs as root.
pub async fn check_packages(
    _agent: AgentAuth,
    Json(req): Json<PackageCheckRequest>,
) -> Json<PackageCheckResponse> {
    let mut installed = Vec::new();
    let mut missing = Vec::new();
    let distro = detect_distro();
    let is_root = is_running_as_root();

    for pkg in &req.packages {
        if is_package_installed(pkg, &distro) {
            installed.push(pkg.clone());
        } else {
            missing.push(pkg.clone());
        }
    }

    Json(PackageCheckResponse { installed, missing, distro, is_root })
}

/// POST /agent/packages/install — Install packages with optional sudo.
pub async fn install_packages(
    _agent: AgentAuth,
    Json(req): Json<PackageCheckRequest>,
) -> Result<Json<AgentResponse>, AppError> {
    let distro = detect_distro();
    let is_root = is_running_as_root();

    for pkg in &req.packages {
        if is_package_installed(pkg, &distro) { continue; }

        let (program, args) = match distro.as_str() {
            "debian" | "ubuntu" => ("apt-get", vec!["install", "-y", pkg.as_str()]),
            "rhel" | "centos" | "fedora" | "rocky" | "almalinux" => ("yum", vec!["install", "-y", pkg.as_str()]),
            _ => return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Unsupported distribution: {}", distro))),
        };

        let result = if is_root {
            std::process::Command::new(program).args(&args).output()
        } else if let Some(ref sudo_pass) = req.sudo_password {
            // Use sudo -S (read password from stdin)
            let full_cmd = format!("{} {}", program, args.join(" "));
            let mut child = std::process::Command::new("sudo")
                .args(&["-S", "sh", "-c", &full_cmd])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot run sudo: {}", e)))?;

            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = writeln!(stdin, "{}", sudo_pass);
            }
            child.wait_with_output()
        } else {
            return Err(AppError(StatusCode::FORBIDDEN,
                "Agent is not running as root. Provide sudo_password for privilege escalation.".into()));
        };

        match result {
            Ok(output) if output.status.success() => {
                tracing::info!("Package '{}' installed successfully", pkg);
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to install '{}': {}", pkg, stderr.trim())));
            }
            Err(e) => {
                return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Cannot run package manager: {}", e)));
            }
        }
    }

    Ok(Json(AgentResponse::ok()))
}

/// POST /agent/exec — Execute a shell command with optional sudo.
pub async fn exec_command(
    _agent: AgentAuth,
    Json(req): Json<ExecRequest>,
) -> Result<Json<ExecResponse>, AppError> {
    if req.command.trim().is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "Empty command".into()));
    }

    let is_root = is_running_as_root();

    let output = if is_root || req.sudo_password.is_none() {
        // Run directly (as root or without sudo)
        std::process::Command::new("sh")
            .args(&["-c", &req.command])
            .output()
    } else {
        // Run via sudo -S
        let mut child = std::process::Command::new("sudo")
            .args(&["-S", "sh", "-c", &req.command])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot spawn sudo: {}", e)))?;

        if let Some(ref pass) = req.sudo_password {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = writeln!(stdin, "{}", pass);
            }
        }
        child.wait_with_output()
    };

    match output {
        Ok(out) => {
            let exit_code = out.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            tracing::info!("Exec '{}' → exit={}", req.command, exit_code);
            Ok(Json(ExecResponse { exit_code, stdout, stderr }))
        }
        Err(e) => Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Command failed: {}", e))),
    }
}

#[derive(serde::Deserialize)]
pub struct ExecRequest {
    pub command: String,
    pub timeout_secs: Option<u32>,
    /// Optional sudo password for privilege escalation.
    pub sudo_password: Option<String>,
}

#[derive(serde::Serialize)]
pub struct ExecResponse {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

fn is_running_as_root() -> bool {
    #[cfg(target_os = "linux")]
    { unsafe { libc::geteuid() == 0 } }
    #[cfg(not(target_os = "linux"))]
    { false }
}

fn detect_distro() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
            for line in content.lines() {
                if let Some(id) = line.strip_prefix("ID=") {
                    return id.trim_matches('"').to_lowercase();
                }
            }
        }
    }
    "unknown".into()
}

fn is_package_installed(package: &str, distro: &str) -> bool {
    match distro {
        "debian" | "ubuntu" => {
            std::process::Command::new("dpkg")
                .args(&["-l", package])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
        "rhel" | "centos" | "fedora" => {
            std::process::Command::new("rpm")
                .args(&["-q", package])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
        _ => false,
    }
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

/// Collect mounted datastore status by checking if known mount paths are actually mounted.
fn collect_datastore_status(state: &AppState) -> Vec<AgentDatastoreStatus> {
    // Read mount points the cluster told us about from the local DB
    let db = match state.db.lock() { Ok(db) => db, Err(_) => return Vec::new() };

    // Check storage_pools table for shared pools (these are cluster datastores)
    let mut stmt = match db.prepare(
        "SELECT id, name, path, pool_type FROM storage_pools WHERE shared = 1"
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let pools: Vec<(String, String)> = match stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(2)?))
    }) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    let mut result = Vec::new();
    for (pool_id, mount_path) in &pools {
        let path = std::path::Path::new(mount_path);
        let mounted = path.exists() && is_mountpoint(mount_path);
        let (total, free) = if mounted { get_fs_stats(mount_path) } else { (0, 0) };

        result.push(AgentDatastoreStatus {
            datastore_id: pool_id.clone(),
            mount_path: mount_path.clone(),
            mounted,
            total_bytes: total,
            free_bytes: free,
        });
    }
    result
}

/// Check if a path is a mount point using `mountpoint -q`.
fn is_mountpoint(path: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("mountpoint")
            .arg("-q")
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    { let _ = path; false }
}

/// Get filesystem total/free bytes for a path.
fn get_fs_stats(path: &str) -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            let c_path = std::ffi::CString::new(path).unwrap();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                let total = stat.f_blocks as u64 * stat.f_frsize as u64;
                let free = stat.f_bavail as u64 * stat.f_frsize as u64;
                return (total, free);
            }
        }
    }
    let _ = path;
    (0, 0)
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

// ── Log retrieval ────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct LogQuery {
    pub service: Option<String>,
    pub lines: Option<usize>,
}

#[derive(serde::Serialize)]
pub struct ServiceLogEntry {
    pub service: String,
    pub lines: Vec<String>,
    pub log_file: String,
    pub available: bool,
}

#[derive(serde::Serialize)]
pub struct HostLogsResponse {
    pub hostname: String,
    pub services: Vec<ServiceLogEntry>,
}

/// GET /agent/logs — return log file contents for requested services.
///
/// Query params:
///   service  — comma-separated list (vmm-server,vmm-san,vmm-cluster) or omit for all
///   lines    — number of tail lines (default 200)
pub async fn logs(
    State(state): State<Arc<AppState>>,
    _agent: AgentAuth,
    axum::extract::Query(q): axum::extract::Query<LogQuery>,
) -> Result<Json<HostLogsResponse>, AppError> {
    let hostname = gethostname::gethostname().to_string_lossy().to_string();
    let tail = q.lines.unwrap_or(200);

    let requested: Vec<&str> = match &q.service {
        Some(s) => s.split(',').map(|s| s.trim()).collect(),
        None => vec!["vmm-server", "vmm-san", "vmm-cluster"],
    };

    // Standard log file locations (match what the installer/build-iso configures)
    let log_paths: &[(&str, &[&str])] = &[
        ("vmm-server", &["/var/log/vmm/vmm-server.log", "/var/log/vmm-server.log"]),
        ("vmm-san",    &["/var/log/vmm/vmm-san.log", "/var/log/vmm-san.log"]),
        ("vmm-cluster",&["/var/log/vmm/vmm-cluster.log", "/var/log/vmm-cluster.log"]),
    ];

    let mut services = Vec::new();

    for (name, paths) in log_paths {
        if !requested.contains(name) {
            continue;
        }

        // Also check configured path for vmm-server
        let extra_path: Option<String> = if *name == "vmm-server" {
            state.config.logging.log_file.as_ref().map(|p| p.to_string_lossy().to_string())
        } else {
            None
        };

        let found = extra_path.as_deref()
            .into_iter()
            .chain(paths.iter().copied())
            .find(|p| std::path::Path::new(p).exists());

        if let Some(path) = found {
            let lines = read_tail_lines(path, tail);
            services.push(ServiceLogEntry {
                service: name.to_string(),
                lines,
                log_file: path.to_string(),
                available: true,
            });
        } else {
            services.push(ServiceLogEntry {
                service: name.to_string(),
                lines: vec![],
                log_file: paths[0].to_string(),
                available: false,
            });
        }
    }

    Ok(Json(HostLogsResponse { hostname, services }))
}

fn read_tail_lines(path: &str, n: usize) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let all: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            let skip = all.len().saturating_sub(n);
            all.into_iter().skip(skip).collect()
        }
        Err(_) => vec![],
    }
}
