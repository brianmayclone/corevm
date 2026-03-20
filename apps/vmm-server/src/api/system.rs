//! System information endpoints.

use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct SystemInfo {
    pub version: &'static str,
    pub platform: &'static str,
    pub arch: &'static str,
    pub hw_virtualization: bool,
    pub cpu_count: usize,
}

/// GET /api/system/info — basic server/host information.
pub async fn info() -> Json<SystemInfo> {
    let hw_virt = libcorevm::ffi::corevm_has_hw_support() != 0;
    Json(SystemInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        hw_virtualization: hw_virt,
        cpu_count: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    })
}
