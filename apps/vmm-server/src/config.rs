//! Server configuration (parsed from TOML file).

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub auth: AuthSection,
    #[serde(default)]
    pub storage: StorageSection,
    #[serde(default)]
    pub network: NetworkSection,
    #[serde(default)]
    pub vms: VmsSection,
    #[serde(default)]
    pub logging: LoggingSection,
    #[serde(default)]
    pub api: ApiSection,
}

#[derive(Debug, Deserialize)]
pub struct ServerSection {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthSection {
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
    #[serde(default = "default_session_timeout")]
    pub session_timeout_hours: u64,
    #[serde(default)]
    pub allow_registration: bool,
}

#[derive(Debug, Deserialize)]
pub struct StorageSection {
    #[serde(default = "default_pool_path")]
    pub default_pool: PathBuf,
    #[serde(default = "default_iso_path")]
    pub iso_pool: PathBuf,
    #[serde(default = "default_max_disk")]
    pub max_disk_size_gb: u64,
}

#[derive(Debug, Deserialize)]
pub struct NetworkSection {
    #[serde(default = "default_net_mode")]
    pub default_mode: String,
    pub bridge_interface: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VmsSection {
    #[serde(default = "default_config_dir")]
    pub config_dir: PathBuf,
    #[serde(default = "default_max_vms")]
    pub max_vms: u32,
    #[serde(default = "default_ram")]
    pub default_ram_mb: u32,
    #[serde(default = "default_cpus")]
    pub default_cpus: u32,
    #[serde(default)]
    pub bios_search_paths: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct LoggingSection {
    #[serde(default = "default_log_level")]
    pub level: String,
    pub file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct ApiSection {
    #[serde(default = "default_cli_access")]
    pub cli_access_enabled: bool,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
}

// ── Defaults ─────────────────────────────────────────────────────────────

fn default_cli_access() -> bool { true }

fn default_bind() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 8443 }
fn default_jwt_secret() -> String { "change-me-in-production".into() }
fn default_session_timeout() -> u64 { 24 }
fn default_pool_path() -> PathBuf { PathBuf::from("/var/lib/vmm/images") }
fn default_iso_path() -> PathBuf { PathBuf::from("/var/lib/vmm/isos") }
fn default_max_disk() -> u64 { 2048 }
fn default_net_mode() -> String { "slirp".into() }
fn default_config_dir() -> PathBuf { PathBuf::from("/var/lib/vmm/vms") }
fn default_max_vms() -> u32 { 50 }
fn default_ram() -> u32 { 2048 }
fn default_cpus() -> u32 { 2 }
fn default_log_level() -> String { "info".into() }

impl Default for ServerSection {
    fn default() -> Self {
        Self { bind: default_bind(), port: default_port(), tls_cert: None, tls_key: None }
    }
}
impl Default for AuthSection {
    fn default() -> Self {
        Self { jwt_secret: default_jwt_secret(), session_timeout_hours: default_session_timeout(), allow_registration: false }
    }
}
impl Default for StorageSection {
    fn default() -> Self {
        Self { default_pool: default_pool_path(), iso_pool: default_iso_path(), max_disk_size_gb: default_max_disk() }
    }
}
impl Default for NetworkSection {
    fn default() -> Self {
        Self { default_mode: default_net_mode(), bridge_interface: None }
    }
}
impl Default for VmsSection {
    fn default() -> Self {
        Self { config_dir: default_config_dir(), max_vms: default_max_vms(), default_ram_mb: default_ram(), default_cpus: default_cpus(), bios_search_paths: Vec::new() }
    }
}
impl Default for LoggingSection {
    fn default() -> Self {
        Self { level: default_log_level(), file: None }
    }
}
impl Default for ApiSection {
    fn default() -> Self {
        Self { cli_access_enabled: default_cli_access(), allowed_ips: Vec::new() }
    }
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: Default::default(), auth: Default::default(), storage: Default::default(),
            network: Default::default(), vms: Default::default(), logging: Default::default(),
            api: Default::default(),
        }
    }
}

impl ServerConfig {
    /// Load config from TOML file, falling back to defaults for missing values.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            tracing::warn!("Config file not found: {}, using defaults", path.display());
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config: {}", e))?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config: {}", e))
    }
}
