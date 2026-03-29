//! Status and health endpoints for CoreSAN.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;
use crate::state::CoreSanState;

#[derive(Serialize)]
pub struct StatusResponse {
    pub running: bool,
    pub node_id: String,
    pub address: String,
    pub hostname: String,
    pub uptime_secs: u64,
    pub volumes: Vec<VolumeStatusSummary>,
    pub peer_count: u32,
    pub available_disks: u32,
    pub claimed_disks: u32,
    pub benchmark_summary: Option<BenchmarkSummary>,
    pub quorum_status: String,
    pub is_leader: bool,
}

#[derive(Serialize)]
pub struct VolumeStatusSummary {
    pub volume_id: String,
    pub volume_name: String,
    pub ftt: u32,
    pub local_raid: String,
    pub chunk_size_bytes: u64,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub status: String,
    pub backend_count: u32,
    pub total_chunks: u64,
    pub synced_chunks: u64,
    pub stale_chunks: u64,
    pub protected_files: u64,
    pub degraded_files: u64,
}

#[derive(Serialize)]
pub struct BenchmarkSummary {
    pub avg_bandwidth_mbps: f64,
    pub avg_latency_us: f64,
    pub worst_peer: Option<String>,
    pub measured_at: String,
}

#[derive(Serialize)]
pub struct DashboardResponse {
    pub status: StatusResponse,
    pub total_capacity_bytes: u64,
    pub used_capacity_bytes: u64,
    pub total_files: u64,
    pub replication_pending: u64,
    pub integrity_errors: u64,
}

/// GET /api/status — full node status (used by vmm-server heartbeat).
pub async fn status(
    State(state): State<Arc<CoreSanState>>,
) -> Json<StatusResponse> {
    let db = state.db.lock().unwrap();
    let volumes = query_volume_summaries(&db);
    let peer_count = state.peers.len() as u32;
    let benchmark_summary = query_benchmark_summary(&db);

    // Count disks via discover_disks (now correctly reads claimed_disks table)
    let disks = crate::storage::disk::discover_disks(&db);
    let available_disks = disks.iter().filter(|d| matches!(d.status,
        crate::storage::disk::DiskStatus::Available | crate::storage::disk::DiskStatus::HasData { .. }
    )).count() as u32;
    let claimed_disks = disks.iter().filter(|d| matches!(d.status,
        crate::storage::disk::DiskStatus::Claimed { .. }
    )).count() as u32;

    let local_addr = format!("http://{}:{}",
        crate::engine::discovery::get_local_ip_cached(), state.config.server.port);

    let quorum_status = format!("{:?}", *state.quorum_status.read().unwrap()).to_lowercase();
    let is_leader = state.is_leader.load(std::sync::atomic::Ordering::Relaxed);

    Json(StatusResponse {
        running: true,
        node_id: state.node_id.clone(),
        address: local_addr,
        hostname: state.hostname.clone(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        volumes,
        peer_count,
        available_disks,
        claimed_disks,
        benchmark_summary,
        quorum_status,
        is_leader,
    })
}

/// GET /api/health — minimal health check.
pub async fn health() -> StatusCode {
    StatusCode::OK
}

/// GET /api/dashboard — aggregated dashboard data.
pub async fn dashboard(
    State(state): State<Arc<CoreSanState>>,
) -> Json<DashboardResponse> {
    let db = state.db.lock().unwrap();
    let volumes = query_volume_summaries(&db);
    let benchmark_summary = query_benchmark_summary(&db);

    let total_capacity_bytes: u64 = volumes.iter().map(|v| v.total_bytes).sum();
    let free_bytes: u64 = volumes.iter().map(|v| v.free_bytes).sum();
    let used_capacity_bytes = total_capacity_bytes.saturating_sub(free_bytes);

    let total_files: u64 = db.query_row(
        "SELECT COUNT(*) FROM file_map", [], |row| row.get(0),
    ).unwrap_or(0);

    let replication_pending: u64 = db.query_row(
        "SELECT COUNT(*) FROM chunk_replicas WHERE state != 'synced'", [], |row| row.get(0),
    ).unwrap_or(0);

    let integrity_errors: u64 = db.query_row(
        "SELECT COUNT(*) FROM integrity_log WHERE passed = 0", [], |row| row.get(0),
    ).unwrap_or(0);

    let disks2 = crate::storage::disk::discover_disks(&db);
    let local_addr2 = format!("http://{}:{}",
        crate::engine::discovery::get_local_ip_cached(), state.config.server.port);
    let quorum_status2 = format!("{:?}", *state.quorum_status.read().unwrap()).to_lowercase();
    let is_leader2 = state.is_leader.load(std::sync::atomic::Ordering::Relaxed);

    let status = StatusResponse {
        running: true,
        node_id: state.node_id.clone(),
        address: local_addr2,
        hostname: state.hostname.clone(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        volumes,
        peer_count: state.peers.len() as u32,
        available_disks: disks2.iter().filter(|d| matches!(d.status,
            crate::storage::disk::DiskStatus::Available | crate::storage::disk::DiskStatus::HasData { .. }
        )).count() as u32,
        claimed_disks: disks2.iter().filter(|d| matches!(d.status,
            crate::storage::disk::DiskStatus::Claimed { .. }
        )).count() as u32,
        benchmark_summary,
        quorum_status: quorum_status2,
        is_leader: is_leader2,
    };

    Json(DashboardResponse {
        status,
        total_capacity_bytes,
        used_capacity_bytes,
        total_files,
        replication_pending,
        integrity_errors,
    })
}

fn query_volume_summaries(db: &rusqlite::Connection) -> Vec<VolumeStatusSummary> {
    let mut stmt = db.prepare(
        "SELECT v.id, v.name, v.ftt, v.local_raid, v.chunk_size_bytes, v.status,
                (SELECT COUNT(*) FROM backends WHERE status = 'online') AS backend_count
         FROM volumes v"
    ).unwrap();

    let volumes: Vec<VolumeStatusSummary> = stmt.query_map([], |row| {
        let vol_id: String = row.get(0)?;
        let ftt: u32 = row.get(2)?;
        Ok((vol_id, row.get(1)?, ftt, row.get::<_, String>(3)?, row.get::<_, u64>(4)?,
            row.get::<_, String>(5)?, row.get::<_, u32>(6)?))
    }).unwrap().filter_map(|r| r.ok()).map(|(vol_id, name, ftt, local_raid, chunk_size, status, bcount)| {
        // Calculate usable capacity based on RAID policy
        // mirror: usable = smallest backend (data mirrored to all)
        // stripe: usable = sum of all backends (data striped across)
        // stripe_mirror: usable = sum / 2 (striped across pairs)
        let (total, free) = query_usable_capacity(db, &local_raid);
        let (total_chunks, synced_chunks, stale_chunks) = query_chunk_counts(db, &vol_id);
        let (protected_files, degraded_files) = query_protection_counts(db, &vol_id, ftt);
        VolumeStatusSummary {
            volume_id: vol_id,
            volume_name: name,
            ftt,
            local_raid,
            chunk_size_bytes: chunk_size,
            total_bytes: total,
            free_bytes: free,
            status,
            backend_count: bcount,
            total_chunks,
            synced_chunks,
            stale_chunks,
            protected_files,
            degraded_files,
        }
    }).collect();

    volumes
}

/// Calculate usable capacity based on RAID policy.
fn query_usable_capacity(db: &rusqlite::Connection, local_raid: &str) -> (u64, u64) {
    let backends: Vec<(u64, u64)> = {
        let mut stmt = db.prepare(
            "SELECT total_bytes, free_bytes FROM backends WHERE status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    if backends.is_empty() {
        return (0, 0);
    }

    match local_raid {
        "mirror" => {
            // Mirror: usable capacity = smallest backend
            let min_total = backends.iter().map(|(t, _)| *t).min().unwrap_or(0);
            let min_free = backends.iter().map(|(_, f)| *f).min().unwrap_or(0);
            (min_total, min_free)
        }
        "stripe_mirror" => {
            // Stripe-mirror: usable = total sum / 2
            let sum_total: u64 = backends.iter().map(|(t, _)| *t).sum();
            let sum_free: u64 = backends.iter().map(|(_, f)| *f).sum();
            (sum_total / 2, sum_free / 2)
        }
        _ => {
            // Stripe or no RAID: usable = sum of all
            let sum_total: u64 = backends.iter().map(|(t, _)| *t).sum();
            let sum_free: u64 = backends.iter().map(|(_, f)| *f).sum();
            (sum_total, sum_free)
        }
    }
}

fn query_file_sync_counts(db: &rusqlite::Connection, volume_id: &str) -> (u64, u64) {
    let synced: u64 = db.query_row(
        "SELECT COUNT(DISTINCT fm.id) FROM file_map fm
         JOIN file_chunks fc ON fc.file_id = fm.id
         JOIN chunk_replicas cr ON cr.chunk_id = fc.id
         WHERE fm.volume_id = ?1 AND cr.state = 'synced'",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    let syncing: u64 = db.query_row(
        "SELECT COUNT(DISTINCT fm.id) FROM file_map fm
         JOIN file_chunks fc ON fc.file_id = fm.id
         JOIN chunk_replicas cr ON cr.chunk_id = fc.id
         WHERE fm.volume_id = ?1 AND cr.state IN ('syncing', 'stale')",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    (synced, syncing)
}

fn query_benchmark_summary(db: &rusqlite::Connection) -> Option<BenchmarkSummary> {
    let row = db.query_row(
        "SELECT AVG(bandwidth_mbps), AVG(latency_us), MAX(measured_at)
         FROM benchmark_results
         WHERE measured_at > datetime('now', '-10 minutes')",
        [], |row| {
            Ok((
                row.get::<_, Option<f64>>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    ).ok()?;

    let (avg_bw, avg_lat, measured_at) = row;
    let avg_bw = avg_bw?;
    let measured_at = measured_at?;

    // Find worst peer (highest latency)
    let worst_peer: Option<String> = db.query_row(
        "SELECT to_node_id FROM benchmark_results
         WHERE measured_at > datetime('now', '-10 minutes')
         ORDER BY latency_us DESC LIMIT 1",
        [], |row| row.get(0),
    ).ok();

    Some(BenchmarkSummary {
        avg_bandwidth_mbps: avg_bw,
        avg_latency_us: avg_lat.unwrap_or(0.0),
        worst_peer,
        measured_at,
    })
}

fn query_volume_raid_info(db: &rusqlite::Connection, volume_id: &str) -> (u32, String, u64) {
    db.query_row(
        "SELECT ftt, local_raid, chunk_size_bytes FROM volumes WHERE id = ?1",
        rusqlite::params![volume_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap_or((1, "stripe".into(), 67108864))
}

fn query_chunk_counts(db: &rusqlite::Connection, volume_id: &str) -> (u64, u64, u64) {
    let total: u64 = db.query_row(
        "SELECT COUNT(*) FROM file_chunks fc JOIN file_map fm ON fm.id = fc.file_id WHERE fm.volume_id = ?1",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    let synced: u64 = db.query_row(
        "SELECT COUNT(DISTINCT fc.id) FROM file_chunks fc
         JOIN file_map fm ON fm.id = fc.file_id
         JOIN chunk_replicas cr ON cr.chunk_id = fc.id
         WHERE fm.volume_id = ?1 AND cr.state = 'synced'",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    let stale: u64 = db.query_row(
        "SELECT COUNT(DISTINCT fc.id) FROM file_chunks fc
         JOIN file_map fm ON fm.id = fc.file_id
         JOIN chunk_replicas cr ON cr.chunk_id = fc.id
         WHERE fm.volume_id = ?1 AND cr.state IN ('stale', 'syncing')",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    (total, synced, stale)
}

fn query_protection_counts(db: &rusqlite::Connection, volume_id: &str, ftt: u32) -> (u64, u64) {
    if ftt == 0 {
        return (0, 0);
    }
    let required = ftt + 1;

    let protected: u64 = db.query_row(
        "SELECT COUNT(*) FROM file_map fm
         WHERE fm.volume_id = ?1 AND fm.protection_status = 'protected'",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    let degraded: u64 = db.query_row(
        "SELECT COUNT(*) FROM file_map fm
         WHERE fm.volume_id = ?1 AND fm.protection_status = 'degraded'",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    (protected, degraded)
}

// ── Network Configuration ────────────────────────────────────────────

#[derive(Serialize)]
pub struct NetworkConfigResponse {
    pub san_interface: String,
    pub san_ip: String,
    pub san_netmask: String,
    pub san_gateway: String,
    pub san_mtu: u32,
    pub san_teaming: String,
}

#[derive(Serialize)]
pub struct NetworkInterface {
    pub name: String,
    pub mac: String,
    pub ipv4: String,
    pub state: String,
    pub speed_mbps: Option<u32>,
    pub mtu: u32,
}

/// GET /api/network/config — current SAN network configuration.
pub async fn get_network_config(
    State(state): State<Arc<CoreSanState>>,
) -> Json<NetworkConfigResponse> {
    Json(NetworkConfigResponse {
        san_interface: state.config.network.san_interface.clone(),
        san_ip: state.config.network.san_ip.clone(),
        san_netmask: state.config.network.san_netmask.clone(),
        san_gateway: state.config.network.san_gateway.clone(),
        san_mtu: state.config.network.san_mtu,
        san_teaming: state.config.network.san_teaming.clone(),
    })
}

/// PUT /api/network/config — update SAN network configuration from cluster.
/// Used by vmm-cluster to push viSwitch SAN interface assignments.
pub async fn update_network_config(
    State(state): State<Arc<CoreSanState>>,
    axum::Json(body): axum::Json<UpdateNetworkConfigRequest>,
) -> axum::Json<serde_json::Value> {
    // Update in-memory config (state.config is read-only after load, so we log the change)
    // In production, this would persist to the TOML config file and trigger rebind.
    tracing::info!(
        "[san-network] Config update from cluster: interfaces={:?}, teaming={}, mtu={}",
        body.interfaces, body.teaming, body.mtu
    );

    // Write to config file for persistence across restarts
    let san_interface = body.interfaces.join(",");
    let san_ip = body.san_ip.as_deref().unwrap_or("");
    let san_netmask = body.san_netmask.as_deref().unwrap_or("");
    let san_gateway = body.san_gateway.as_deref().unwrap_or("");
    let config_updates = format!(
        "\n[network]\nsan_interface = \"{}\"\nsan_ip = \"{}\"\nsan_netmask = \"{}\"\nsan_gateway = \"{}\"\nsan_teaming = \"{}\"\nsan_mtu = {}\n",
        san_interface, san_ip, san_netmask, san_gateway, body.teaming, body.mtu
    );

    // Try to update the config file
    let config_path = state.config_path.as_ref().map(|p| p.as_path());
    if let Some(path) = config_path {
        if let Ok(mut content) = std::fs::read_to_string(path) {
            // Remove existing [network] section if present
            if let Some(start) = content.find("[network]") {
                let end = content[start + 9..].find("\n[")
                    .map(|e| start + 9 + e)
                    .unwrap_or(content.len());
                content.replace_range(start..end, "");
            }
            content.push_str(&config_updates);
            if let Err(e) = std::fs::write(path, &content) {
                tracing::warn!("[san-network] Failed to write config: {}", e);
            }
        }
    }

    axum::Json(serde_json::json!({"ok": true, "san_interface": san_interface}))
}

#[derive(serde::Deserialize)]
pub struct UpdateNetworkConfigRequest {
    /// List of interface names to bind SAN traffic to.
    pub interfaces: Vec<String>,
    /// Teaming policy: "none", "roundrobin", "failover".
    #[serde(default)]
    pub teaming: String,
    /// Static IP for SAN (optional — if empty, keep existing/DHCP).
    pub san_ip: Option<String>,
    /// Subnet mask (e.g. "/24" or "255.255.255.0").
    pub san_netmask: Option<String>,
    /// Gateway for SAN network.
    pub san_gateway: Option<String>,
    /// MTU for SAN traffic.
    #[serde(default)]
    pub mtu: u32,
}

/// GET /api/network/interfaces — list all network interfaces on this host.
pub async fn list_interfaces() -> Json<Vec<NetworkInterface>> {
    let interfaces = discover_interfaces();
    Json(interfaces)
}

fn discover_interfaces() -> Vec<NetworkInterface> {
    let sys_net = std::path::Path::new("/sys/class/net");
    let entries = match std::fs::read_dir(sys_net) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut result = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "lo" { continue; }

        let iface_path = sys_net.join(&name);

        let mac = std::fs::read_to_string(iface_path.join("address"))
            .map(|s| s.trim().to_string()).unwrap_or_default();

        let state = std::fs::read_to_string(iface_path.join("operstate"))
            .map(|s| s.trim().to_string()).unwrap_or_else(|_| "unknown".into());

        let mtu: u32 = std::fs::read_to_string(iface_path.join("mtu"))
            .ok().and_then(|s| s.trim().parse().ok()).unwrap_or(1500);

        let speed_mbps: Option<u32> = std::fs::read_to_string(iface_path.join("speed"))
            .ok().and_then(|s| s.trim().parse().ok());

        // Get IPv4 from ip command
        let ipv4 = std::process::Command::new("ip")
            .args(["-4", "-o", "addr", "show", &name])
            .output().ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.split_whitespace().nth(3).map(|ip| ip.to_string()))
            .unwrap_or_default();

        result.push(NetworkInterface {
            name, mac, ipv4, state, speed_mbps, mtu,
        });
    }

    result
}
