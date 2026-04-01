//! CoreSAN configuration (parsed from TOML file).

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct CoreSanConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub data: DataSection,
    #[serde(default)]
    pub peer: PeerSection,
    #[serde(default)]
    pub network: NetworkSection,
    #[serde(default)]
    pub replication: ReplicationSection,
    #[serde(default)]
    pub benchmark: BenchmarkSection,
    #[serde(default)]
    pub integrity: IntegritySection,
    #[serde(default)]
    pub dedup: DedupSection,
    #[serde(default)]
    pub logging: LoggingSection,
    #[serde(default)]
    pub cluster: ClusterSection,
}

#[derive(Debug, Deserialize)]
pub struct ServerSection {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct DataSection {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_fuse_root")]
    pub fuse_root: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct PeerSection {
    #[serde(default = "default_peer_port")]
    pub port: u16,
    #[serde(default)]
    pub secret: String,
}

#[derive(Debug, Deserialize)]
pub struct NetworkSection {
    /// NIC(s) to bind SAN traffic to (e.g. "eth1" or "eth1,eth2" for multi-NIC).
    /// Empty = all interfaces.
    #[serde(default)]
    pub san_interface: String,
    /// Static IP for the SAN interface. Empty = use existing/DHCP.
    #[serde(default)]
    pub san_ip: String,
    /// Subnet mask (e.g. "255.255.255.0" or "/24"). Empty = use existing.
    #[serde(default)]
    pub san_netmask: String,
    /// Gateway for the SAN network. Empty = no gateway (direct L2).
    #[serde(default)]
    pub san_gateway: String,
    /// MTU for SAN traffic (0 = default, 9000 = jumbo frames recommended).
    #[serde(default)]
    pub san_mtu: u32,
    /// Multi-NIC teaming policy: "none", "roundrobin", "failover". Empty = none.
    #[serde(default)]
    pub san_teaming: String,
}

#[derive(Debug, Deserialize)]
pub struct ReplicationSection {
    #[serde(default = "default_sync_mode")]
    pub sync_mode: String,
}

#[derive(Debug, Deserialize)]
pub struct BenchmarkSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_benchmark_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_bandwidth_test_size")]
    pub bandwidth_test_size_mb: u32,
}

#[derive(Debug, Deserialize)]
pub struct IntegritySection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_integrity_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_repair_interval")]
    pub repair_interval_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct DedupSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_dedup_interval")]
    pub interval_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct LoggingSection {
    #[serde(default = "default_log_level")]
    pub level: String,
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct ClusterSection {
    /// URL of vmm-cluster for witness tie-breaking (e.g. "https://10.0.0.1:9443").
    /// Empty = no witness, pure majority quorum only.
    #[serde(default)]
    pub witness_url: String,
}

impl Default for ClusterSection {
    fn default() -> Self {
        Self { witness_url: String::new() }
    }
}

// ── Defaults ─────────────────────────────────────────────────────────────

fn default_bind() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 7443 }
fn default_data_dir() -> PathBuf { PathBuf::from("/var/lib/vmm-san") }
fn default_fuse_root() -> PathBuf { PathBuf::from("/vmm/san") }
fn default_peer_port() -> u16 { 7444 }
fn default_sync_mode() -> String { "async".into() }
fn default_true() -> bool { true }
fn default_benchmark_interval() -> u64 { 300 }
fn default_bandwidth_test_size() -> u32 { 64 }
fn default_integrity_interval() -> u64 { 3600 }
fn default_repair_interval() -> u64 { 60 }
fn default_dedup_interval() -> u64 { 300 }
fn default_log_level() -> String { "info".into() }

impl Default for ServerSection {
    fn default() -> Self {
        Self { bind: default_bind(), port: default_port() }
    }
}
impl Default for DataSection {
    fn default() -> Self {
        Self { data_dir: default_data_dir(), fuse_root: default_fuse_root() }
    }
}
impl Default for PeerSection {
    fn default() -> Self {
        Self { port: default_peer_port(), secret: String::new() }
    }
}
impl Default for NetworkSection {
    fn default() -> Self {
        Self {
            san_interface: String::new(),
            san_ip: String::new(),
            san_netmask: String::new(),
            san_gateway: String::new(),
            san_mtu: 0,
            san_teaming: String::new(),
        }
    }
}
impl Default for ReplicationSection {
    fn default() -> Self {
        Self { sync_mode: default_sync_mode() }
    }
}
impl Default for BenchmarkSection {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_benchmark_interval(),
            bandwidth_test_size_mb: default_bandwidth_test_size(),
        }
    }
}
impl Default for IntegritySection {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_integrity_interval(),
            repair_interval_secs: default_repair_interval(),
        }
    }
}
impl Default for DedupSection {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_dedup_interval(),
        }
    }
}
impl Default for LoggingSection {
    fn default() -> Self {
        Self { level: default_log_level(), log_file: None }
    }
}
impl Default for CoreSanConfig {
    fn default() -> Self {
        Self {
            server: Default::default(),
            data: Default::default(),
            peer: Default::default(),
            network: Default::default(),
            replication: Default::default(),
            benchmark: Default::default(),
            integrity: Default::default(),
            dedup: Default::default(),
            logging: Default::default(),
            cluster: Default::default(),
        }
    }
}

impl CoreSanConfig {
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
