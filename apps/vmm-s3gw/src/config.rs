use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct S3GwConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub san: SanSection,
    #[serde(default)]
    pub tls: TlsSection,
    #[serde(default)]
    pub logging: LoggingSection,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerSection {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_region")]
    pub region: String,
}

fn default_listen() -> String {
    "0.0.0.0:9000".to_string()
}

fn default_region() -> String {
    "us-east-1".to_string()
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            region: default_region(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SanSection {
    #[serde(default = "default_mgmt_socket")]
    pub mgmt_socket: String,
    #[serde(default = "default_object_socket_dir")]
    pub object_socket_dir: String,
}

fn default_mgmt_socket() -> String {
    "/run/vmm-san/mgmt.sock".to_string()
}

fn default_object_socket_dir() -> String {
    "/run/vmm-san".to_string()
}

impl Default for SanSection {
    fn default() -> Self {
        Self {
            mgmt_socket: default_mgmt_socket(),
            object_socket_dir: default_object_socket_dir(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct TlsSection {
    pub cert: Option<String>,
    pub key: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingSection {
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

impl S3GwConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            tracing::warn!("Config file {} not found, using defaults", path.display());
            // Return default config using toml deserialization of empty string
            let config: S3GwConfig =
                toml::from_str("").map_err(|e| format!("default config error: {}", e))?;
            return Ok(config);
        }
        let contents =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
        let config: S3GwConfig =
            toml::from_str(&contents).map_err(|e| format!("parse {}: {}", path.display(), e))?;
        Ok(config)
    }
}
