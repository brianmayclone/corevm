//! Cluster communication types shared between vmm-server (Agent) and vmm-cluster (Authority).
//!
//! These types define the protocol for cluster ↔ node communication.
//! vmm-cluster is the central authority (like vCenter), vmm-server nodes are agents (like ESXi).

use serde::{Serialize, Deserialize};

// ── Backend Mode ────────────────────────────────────────────────────────

/// Backend mode reported in /api/system/info.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendMode {
    /// Standalone vmm-server (no cluster).
    Standalone,
    /// vmm-server managed by a cluster (agent mode).
    Managed,
    /// vmm-cluster central authority.
    Cluster,
}

// ── Agent Registration (Cluster → Node) ─────────────────────────────────

/// Request sent from vmm-cluster to vmm-server to register it as a managed agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentRegisterRequest {
    pub cluster_id: String,
    pub cluster_url: String,
    pub agent_token: String,
    pub node_id: String,
}

/// Response from vmm-server after successful registration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentRegisterResponse {
    pub node_id: String,
    pub hostname: String,
    pub version: String,
}

// ── Host Hardware Info (Node → Cluster) ─────────────────────────────────

/// Static hardware information reported by the agent on registration and heartbeat.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostHardwareInfo {
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub total_ram_mb: u64,
    pub hw_virtualization: bool,
}

// ── Heartbeat / Status (Node → Cluster) ─────────────────────────────────

/// Full host status returned by GET /agent/status.
/// This is the heartbeat payload — the cluster polls this every 10 seconds.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostStatus {
    pub node_id: String,
    pub hostname: String,
    pub version: String,
    pub uptime_secs: u64,
    /// Hardware info (static, but included for convenience).
    pub hardware: HostHardwareInfo,
    /// Current free RAM in MB.
    pub free_ram_mb: u64,
    /// CPU usage as percentage (0.0–100.0).
    pub cpu_usage_pct: f32,
    /// Status of all VMs on this host.
    pub vms: Vec<AgentVmStatus>,
    /// Status of all mounted datastores on this host.
    pub datastores: Vec<AgentDatastoreStatus>,
}

/// Runtime status of a single VM on the agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentVmStatus {
    pub id: String,
    /// "running", "stopped", "paused", "stopping", "error"
    pub state: String,
    pub cpu_usage_pct: f32,
    pub ram_used_mb: u32,
    pub uptime_secs: u64,
}

/// Status of a single datastore mount on the agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentDatastoreStatus {
    pub datastore_id: String,
    pub mount_path: String,
    pub mounted: bool,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

// ── VM Provisioning Commands (Cluster → Node) ───────────────────────────

/// Command to provision (create) a VM on a host.
/// The cluster is the authority — it tells the node exactly what to create.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvisionVmRequest {
    pub vm_id: String,
    pub config: serde_json::Value,
}

/// Response from the agent after provisioning a VM.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvisionVmResponse {
    pub vm_id: String,
    pub success: bool,
    pub error: Option<String>,
}

// ── Storage Commands (Cluster → Node) ───────────────────────────────────

/// Command to mount a datastore on a host.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MountDatastoreRequest {
    pub datastore_id: String,
    pub store_type: String,
    pub mount_source: String,
    pub mount_opts: String,
    pub mount_path: String,
}

/// Command to unmount a datastore on a host.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnmountDatastoreRequest {
    pub datastore_id: String,
    pub mount_path: String,
}

/// Command to create a disk image on a host.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateDiskRequest {
    pub disk_id: String,
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub format: String,
}

/// Command to resize a disk image on a host.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResizeDiskRequest {
    pub path: String,
    pub new_size_bytes: u64,
}

// ── Network / Bridge Commands (Cluster → Node) ─────────────────────────

/// Command to set up a bridge + optional VXLAN overlay on a node.
/// Sent when a virtual network is created or a new node joins the cluster.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetupBridgeRequest {
    /// Unique network ID from the cluster (used for naming: e.g. "sdn42").
    pub network_id: i64,
    /// Linux bridge name to create (e.g. "sdn42").
    pub bridge_name: String,
    /// Subnet in CIDR notation (e.g. "10.0.50.0/24") — for informational use.
    pub subnet: String,
    /// Optional VLAN ID (1–4094). If set, the bridge is VLAN-tagged.
    pub vlan_id: Option<i32>,
    /// Optional VXLAN configuration for cross-host overlay networking.
    pub vxlan: Option<VxlanConfig>,
}

/// VXLAN overlay configuration for cross-host VM communication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VxlanConfig {
    /// VXLAN Network Identifier (VNI) — must be unique per virtual network.
    pub vni: u32,
    /// Multicast group for BUM traffic (e.g. "239.1.1.1"), or empty for unicast.
    pub group: String,
    /// UDP port for VXLAN (default: 4789).
    pub port: u16,
    /// Local IP to use as VTEP source (the host's main IP).
    pub local_ip: String,
}

/// Command to tear down a bridge on a node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeardownBridgeRequest {
    pub network_id: i64,
    pub bridge_name: String,
}

// ── Generic Agent Response ──────────────────────────────────────────────

/// Simple success/error response from agent operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AgentResponse {
    pub fn ok() -> Self {
        Self { success: true, error: None }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self { success: false, error: Some(msg.into()) }
    }
}

// ── Managed Mode Info ───────────────────────────────────────────────────

/// Persistent cluster configuration stored on the managed node.
/// Saved to `cluster.json` in the node's config directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManagedNodeConfig {
    pub managed: bool,
    pub cluster_id: String,
    pub cluster_url: String,
    pub agent_token: String,
    pub node_id: String,
}

// ── Direct Host-to-Host Migration ───────────────────────────────────────

/// Command from cluster to source host: send VM disks to target host.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MigrationSendRequest {
    pub vm_id: String,
    /// One-time token for this migration (expires after use).
    pub migration_token: String,
    /// Target host address to send disks to.
    pub target_address: String,
    /// Disk paths to transfer.
    pub disk_paths: Vec<String>,
    /// VM config JSON to send along with disks.
    pub config_json: String,
}

/// Command from cluster to target host: expect incoming migration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MigrationReceiveRequest {
    pub vm_id: String,
    /// One-time token — must match the source's token.
    pub migration_token: String,
    /// Source host address to expect connection from.
    pub source_address: String,
}

/// Progress report from agent during migration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MigrationProgress {
    pub vm_id: String,
    pub migration_token: String,
    pub bytes_sent: u64,
    pub bytes_total: u64,
    pub status: String,  // "transferring", "completed", "failed"
    pub error: Option<String>,
}
