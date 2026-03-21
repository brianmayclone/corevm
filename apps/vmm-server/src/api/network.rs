//! Network management API — host interfaces, bridges, VLANs.

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::middleware::{AuthUser, AppError};

#[derive(Serialize)]
pub struct NetworkInterface {
    pub name: String,
    pub kind: String,          // "ethernet", "bridge", "loopback", "virtual", "wireless"
    pub mac: String,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub mtu: u32,
    pub state: String,         // "up", "down"
    pub speed_mbps: Option<u32>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Serialize)]
pub struct NetworkStats {
    pub total_interfaces: usize,
    pub active_interfaces: usize,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
}

/// GET /api/network/interfaces — list all host network interfaces.
pub async fn list_interfaces(_auth: AuthUser) -> Result<Json<Vec<NetworkInterface>>, AppError> {
    let ifaces = read_host_interfaces()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(ifaces))
}

/// GET /api/network/stats
pub async fn network_stats(_auth: AuthUser) -> Result<Json<NetworkStats>, AppError> {
    let ifaces = read_host_interfaces()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let active = ifaces.iter().filter(|i| i.state == "up").count();
    let total_rx = ifaces.iter().map(|i| i.rx_bytes).sum();
    let total_tx = ifaces.iter().map(|i| i.tx_bytes).sum();
    Ok(Json(NetworkStats {
        total_interfaces: ifaces.len(),
        active_interfaces: active,
        total_rx_bytes: total_rx,
        total_tx_bytes: total_tx,
    }))
}

/// Read host network interfaces from /sys/class/net (Linux).
fn read_host_interfaces() -> Result<Vec<NetworkInterface>, String> {
    let mut interfaces = Vec::new();

    let net_dir = std::path::Path::new("/sys/class/net");
    if !net_dir.exists() {
        return Ok(interfaces);
    }

    let entries = std::fs::read_dir(net_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let base = entry.path();

        let read_file = |name: &str| -> String {
            std::fs::read_to_string(base.join(name)).unwrap_or_default().trim().to_string()
        };

        let mac = read_file("address");
        let mtu: u32 = read_file("mtu").parse().unwrap_or(1500);
        let operstate = read_file("operstate");
        let state = if operstate == "up" { "up" } else { "down" };
        let speed_str = read_file("speed");
        let speed_mbps: Option<u32> = speed_str.parse().ok().filter(|&s: &u32| s > 0 && s < 1_000_000);

        // RX/TX bytes
        let rx_bytes: u64 = read_file("statistics/rx_bytes").parse().unwrap_or(0);
        let tx_bytes: u64 = read_file("statistics/tx_bytes").parse().unwrap_or(0);

        // Determine interface kind
        let kind = if name == "lo" {
            "loopback"
        } else if base.join("bridge").exists() {
            "bridge"
        } else if base.join("wireless").exists() || base.join("phy80211").exists() {
            "wireless"
        } else if name.starts_with("veth") || name.starts_with("virbr") || name.starts_with("docker")
            || name.starts_with("br-") || name.starts_with("vnet") || name.starts_with("tap") {
            "virtual"
        } else {
            "ethernet"
        };

        // Get IPv4 address via ip command (fast, no external deps)
        let ipv4 = get_ipv4_address(&name);
        let ipv6 = get_ipv6_address(&name);

        interfaces.push(NetworkInterface {
            name, kind: kind.to_string(), mac, ipv4, ipv6, mtu, state: state.to_string(),
            speed_mbps, rx_bytes, tx_bytes,
        });
    }

    // Sort: ethernet first, then bridges, then virtual, loopback last
    interfaces.sort_by(|a, b| {
        let order = |k: &str| match k { "ethernet" => 0, "wireless" => 1, "bridge" => 2, "virtual" => 3, "loopback" => 4, _ => 5 };
        order(&a.kind).cmp(&order(&b.kind)).then(a.name.cmp(&b.name))
    });

    Ok(interfaces)
}

fn get_ipv4_address(iface: &str) -> Option<String> {
    let output = std::process::Command::new("ip")
        .args(["-4", "addr", "show", iface])
        .output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("inet ") {
            return trimmed.split_whitespace().nth(1).map(|s| s.to_string());
        }
    }
    None
}

fn get_ipv6_address(iface: &str) -> Option<String> {
    let output = std::process::Command::new("ip")
        .args(["-6", "addr", "show", iface, "scope", "global"])
        .output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("inet6 ") {
            return trimmed.split_whitespace().nth(1).map(|s| s.to_string());
        }
    }
    None
}
