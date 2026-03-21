//! Cluster configuration (parsed from TOML file).

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct ClusterConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub auth: AuthSection,
    #[serde(default)]
    pub data: DataSection,
    #[serde(default)]
    pub logging: LoggingSection,
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
}

#[derive(Debug, Deserialize)]
pub struct DataSection {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct LoggingSection {
    #[serde(default = "default_log_level")]
    pub level: String,
    pub file: Option<PathBuf>,
}

// ── Defaults ─────────────────────────────────────────────────────────────

fn default_bind() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 9443 }
fn default_jwt_secret() -> String { "change-me-in-production".into() }
fn default_session_timeout() -> u64 { 24 }
fn default_data_dir() -> PathBuf { PathBuf::from("/var/lib/vmm-cluster") }
fn default_log_level() -> String { "info".into() }

impl Default for ServerSection {
    fn default() -> Self {
        Self { bind: default_bind(), port: default_port(), tls_cert: None, tls_key: None }
    }
}
impl Default for AuthSection {
    fn default() -> Self {
        Self { jwt_secret: default_jwt_secret(), session_timeout_hours: default_session_timeout() }
    }
}
impl Default for DataSection {
    fn default() -> Self {
        Self { data_dir: default_data_dir() }
    }
}
impl Default for LoggingSection {
    fn default() -> Self {
        Self { level: default_log_level(), file: None }
    }
}
impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            server: Default::default(), auth: Default::default(),
            data: Default::default(), logging: Default::default(),
        }
    }
}

impl ClusterConfig {
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
