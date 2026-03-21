//! Application state shared across all request handlers.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use dashmap::DashMap;
use tokio::sync::broadcast;
use libcorevm::runtime::VmControlHandle;

use crate::config::ServerConfig;
use crate::vm::event_handler::FrameBufferData;

/// Central application state — passed to all axum handlers via State extractor.
pub struct AppState {
    /// Running/stopped VMs indexed by UUID.
    pub vms: DashMap<String, VmInstance>,
    /// SQLite database connection (behind Mutex for thread safety).
    pub db: Mutex<rusqlite::Connection>,
    /// Server configuration (immutable after startup).
    pub config: ServerConfig,
    /// JWT signing secret.
    pub jwt_secret: String,
    /// Server start time (for uptime).
    pub started_at: std::time::Instant,
}

/// Runtime state for a single VM.
pub struct VmInstance {
    pub id: String,
    pub config: vmm_core::config::VmConfig,
    pub state: VmState,
    /// libcorevm handle (valid only when running).
    pub vm_handle: Option<u64>,
    /// Control handle for stop/pause/resume.
    pub control: Option<VmControlHandle>,
    /// Shared framebuffer (for WebSocket console).
    pub framebuffer: Option<Arc<Mutex<FrameBufferData>>>,
    /// Serial output broadcast (for serial WebSocket).
    pub serial_tx: Option<broadcast::Sender<Vec<u8>>>,
    /// VM execution thread.
    pub vm_thread: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VmState {
    Stopped,
    Running,
    Paused,
    Stopping,
}
