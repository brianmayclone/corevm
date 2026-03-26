//! UDP network discovery — broadcasts this node's presence on the LAN.
//!
//! Sends a periodic UDP broadcast beacon so vmm-cluster can auto-discover
//! unmanaged vmm-server instances without manual address entry.

use std::sync::Arc;
use std::net::UdpSocket;
use tokio::time::{interval, Duration};
use vmm_core::cluster::{DiscoveryBeacon, DISCOVERY_MAGIC, DISCOVERY_PORT};
use crate::state::AppState;

const BEACON_INTERVAL_SECS: u64 = 10;

/// Spawn the discovery beacon as a background task.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(BEACON_INTERVAL_SECS));

        // Create UDP socket for broadcasting
        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Discovery: cannot create UDP socket: {}", e);
                return;
            }
        };
        if let Err(e) = socket.set_broadcast(true) {
            tracing::warn!("Discovery: cannot enable broadcast: {}", e);
            return;
        }

        let broadcast_addr = format!("255.255.255.255:{}", DISCOVERY_PORT);
        tracing::info!("Discovery beacon started (UDP broadcast every {}s on port {})",
            BEACON_INTERVAL_SECS, DISCOVERY_PORT);

        loop {
            tick.tick().await;
            send_beacon(&state, &socket, &broadcast_addr);
        }
    });
}

fn send_beacon(state: &AppState, socket: &UdpSocket, addr: &str) {
    let managed = state.managed_config.lock().ok()
        .and_then(|c| c.as_ref().map(|m| (m.managed, m.cluster_id.clone())));

    let (is_managed, cluster_id) = match managed {
        Some((true, cid)) => (true, cid),
        _ => (false, String::new()),
    };

    let bind = &state.config.server.bind;
    let port = state.config.server.port;

    // Determine our externally-reachable address
    let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let address = format!("http://{}:{}", local_ip, port);

    let beacon = DiscoveryBeacon {
        service: "vmm-server".into(),
        hostname: gethostname::gethostname().to_string_lossy().to_string(),
        address,
        version: env!("CARGO_PKG_VERSION").into(),
        managed: is_managed,
        cluster_id,
        san_node_id: String::new(),
        san_volumes: 0,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    let json = match serde_json::to_vec(&beacon) {
        Ok(j) => j,
        Err(_) => return,
    };

    // Packet format: CVMD + JSON
    let mut packet = Vec::with_capacity(4 + json.len());
    packet.extend_from_slice(DISCOVERY_MAGIC);
    packet.extend_from_slice(&json);

    socket.send_to(&packet, addr).ok();
}

/// Get a non-loopback local IP address.
fn get_local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Connect to a public IP to determine which interface would be used
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}
