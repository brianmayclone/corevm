//! UDP network discovery — broadcasts CoreSAN presence on the LAN.
//!
//! Sends a periodic UDP broadcast so vmm-cluster can auto-discover
//! CoreSAN instances and show them in the UI.

use std::sync::Arc;
use std::net::UdpSocket;
use tokio::time::{interval, Duration};
use vmm_core::cluster::{DiscoveryBeacon, DISCOVERY_MAGIC, DISCOVERY_PORT};
use crate::state::CoreSanState;

const BEACON_INTERVAL_SECS: u64 = 10;

/// Spawn the discovery beacon as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(BEACON_INTERVAL_SECS));

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

fn send_beacon(state: &CoreSanState, socket: &UdpSocket, addr: &str) {
    let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let port = state.config.server.port;
    let address = format!("http://{}:{}", local_ip, port);

    let vol_count = {
        let db = state.db.lock().unwrap();
        db.query_row("SELECT COUNT(*) FROM volumes", [], |row| row.get::<_, u32>(0))
            .unwrap_or(0)
    };

    let beacon = DiscoveryBeacon {
        service: "vmm-san".into(),
        hostname: state.hostname.clone(),
        address,
        version: env!("CARGO_PKG_VERSION").into(),
        managed: false, // CoreSAN is always autonomous
        cluster_id: String::new(),
        san_node_id: state.node_id.clone(),
        san_volumes: vol_count,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    let json = match serde_json::to_vec(&beacon) {
        Ok(j) => j,
        Err(_) => return,
    };

    let mut packet = Vec::with_capacity(4 + json.len());
    packet.extend_from_slice(DISCOVERY_MAGIC);
    packet.extend_from_slice(&json);

    socket.send_to(&packet, addr).ok();
}

fn get_local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}
