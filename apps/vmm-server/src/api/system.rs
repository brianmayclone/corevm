//! System information endpoints.

use axum::{extract::{State, Query}, Json};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use crate::state::AppState;
use crate::auth::middleware::AuthUser;

#[derive(Serialize)]
pub struct SystemInfo {
    pub version: &'static str,
    pub platform: &'static str,
    pub arch: &'static str,
    pub hostname: String,
    pub hw_virtualization: bool,
    pub cpu_count: usize,
    pub total_ram_mb: u64,
    pub free_ram_mb: u64,
    pub total_disk_bytes: u64,
    pub free_disk_bytes: u64,
    /// Backend mode: "standalone" or "managed" (by cluster)
    pub mode: String,
    /// URL of the managing cluster (only when mode == "managed")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_url: Option<String>,
}

/// GET /api/system/info
pub async fn info(State(state): State<Arc<AppState>>) -> Json<SystemInfo> {
    let hw_virt = libcorevm::ffi::corevm_has_hw_support() != 0;
    let (total_ram, free_ram) = get_host_memory();
    let (total_disk, free_disk) = get_host_disk();

    let (mode, cluster_url) = {
        let managed = state.managed_config.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref config) = *managed {
            ("managed".to_string(), Some(config.cluster_url.clone()))
        } else {
            ("standalone".to_string(), None)
        }
    };

    let hostname = gethostname::gethostname().to_string_lossy().to_string();

    Json(SystemInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        hostname,
        hw_virtualization: hw_virt,
        cpu_count: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        total_ram_mb: total_ram,
        free_ram_mb: free_ram,
        total_disk_bytes: total_disk,
        free_disk_bytes: free_disk,
        mode,
        cluster_url,
    })
}

#[derive(Serialize)]
pub struct DashboardStats {
    pub total_vms: usize,
    pub running_vms: usize,
    pub stopped_vms: usize,
    pub cpu_count: usize,
    pub total_ram_mb: u64,
    pub used_ram_mb: u64,
    pub total_disk_bytes: u64,
    pub used_disk_bytes: u64,
}

/// GET /api/system/stats — aggregated dashboard stats.
pub async fn stats(State(state): State<Arc<AppState>>) -> Json<DashboardStats> {
    let total_vms = state.vms.len();
    let running_vms = state.vms.iter().filter(|v| v.state == crate::state::VmState::Running).count();
    let stopped_vms = total_vms - running_vms;
    let used_ram_mb: u64 = state.vms.iter()
        .filter(|v| v.state == crate::state::VmState::Running)
        .map(|v| v.config.ram_mb as u64)
        .sum();

    let (total_ram, free_ram) = get_host_memory();
    let (total_disk, free_disk) = get_host_disk();

    Json(DashboardStats {
        total_vms,
        running_vms,
        stopped_vms,
        cpu_count: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        total_ram_mb: total_ram,
        used_ram_mb: total_ram.saturating_sub(free_ram),
        total_disk_bytes: total_disk,
        used_disk_bytes: total_disk.saturating_sub(free_disk),
    })
}

fn get_host_memory() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            let mut total = 0u64;
            let mut avail = 0u64;
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    total = parse_meminfo_kb(line);
                } else if line.starts_with("MemAvailable:") {
                    avail = parse_meminfo_kb(line);
                }
            }
            return (total / 1024, avail / 1024); // KB → MB
        }
    }
    (0, 0)
}

#[cfg(target_os = "linux")]
fn parse_meminfo_kb(line: &str) -> u64 {
    line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn get_host_disk() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            let path = std::ffi::CString::new("/").unwrap();
            if libc::statvfs(path.as_ptr(), &mut stat) == 0 {
                let total = stat.f_blocks as u64 * stat.f_frsize as u64;
                let free = stat.f_bavail as u64 * stat.f_frsize as u64;
                return (total, free);
            }
        }
    }
    (0, 0)
}

// ── Activity Feed ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ActivityQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
}
fn default_limit() -> u32 { 20 }

/// GET /api/system/activity — recent audit log entries.
pub async fn activity(
    _auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<Vec<crate::services::audit::AuditEntry>>, crate::auth::middleware::AppError> {
    let db = state.db.lock().unwrap();
    let entries = crate::services::audit::AuditService::recent(&db, q.limit)
        .map_err(|e| crate::auth::middleware::AppError(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(entries))
}
