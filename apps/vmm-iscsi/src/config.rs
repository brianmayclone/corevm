use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct IscsiConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub san: SanSection,
    #[serde(default)]
    pub logging: LoggingSection,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerSection {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_node_name")]
    pub node_name: String,
}

fn default_listen() -> String { "0.0.0.0:3260".to_string() }
fn default_node_name() -> String { "iqn.2026-04.io.corevm".to_string() }

impl Default for ServerSection {
    fn default() -> Self {
        Self { listen: default_listen(), node_name: default_node_name() }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SanSection {
    #[serde(default = "default_mgmt_socket")]
    pub mgmt_socket: String,
    #[serde(default = "default_block_socket_dir")]
    pub block_socket_dir: String,
}

fn default_mgmt_socket() -> String { "/run/vmm-san/mgmt.sock".to_string() }
fn default_block_socket_dir() -> String { "/run/vmm-san".to_string() }

impl Default for SanSection {
    fn default() -> Self {
        Self { mgmt_socket: default_mgmt_socket(), block_socket_dir: default_block_socket_dir() }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingSection {
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String { "info".to_string() }

impl Default for LoggingSection {
    fn default() -> Self { Self { level: default_log_level() } }
}

impl IscsiConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            tracing::warn!("Config file {} not found, using defaults", path.display());
            let config: IscsiConfig = toml::from_str("").map_err(|e| format!("default config error: {}", e))?;
            return Ok(config);
        }
        let contents = std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
        let config: IscsiConfig = toml::from_str(&contents).map_err(|e| format!("parse {}: {}", path.display(), e))?;
        Ok(config)
    }
}
