//! UDP network discovery listener — receives beacons from vmm-server and vmm-san nodes.
//!
//! Listens on the discovery port for broadcast packets. Discovered nodes are stored
//! in memory and available via the API for the UI to show "available nodes" for
//! one-click host registration and CoreSAN setup.

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vmm_core::cluster::{DiscoveryBeacon, DISCOVERY_MAGIC, DISCOVERY_PORT};

/// A discovered node from the network.
#[derive(Clone, Debug, serde::Serialize)]
pub struct DiscoveredNode {
    pub service: String,
    pub hostname: String,
    pub address: String,
    pub version: String,
    pub managed: bool,
    pub cluster_id: String,
    pub san_node_id: String,
    pub san_volumes: u32,
    pub last_seen: String,
    /// How many seconds ago this node was last seen
    pub age_secs: u64,
}

/// In-memory store of discovered nodes, keyed by address.
pub struct DiscoveryStore {
    nodes: Mutex<HashMap<String, (DiscoveryBeacon, Instant)>>,
}

impl DiscoveryStore {
    pub fn new() -> Self {
        Self { nodes: Mutex::new(HashMap::new()) }
    }

    fn insert(&self, beacon: DiscoveryBeacon) {
        let key = format!("{}:{}", beacon.service, beacon.address);
        self.nodes.lock().unwrap().insert(key, (beacon, Instant::now()));
    }

    /// Get all discovered nodes, filtering out stale entries (older than 60 seconds).
    pub fn list(&self) -> Vec<DiscoveredNode> {
        let mut nodes = self.nodes.lock().unwrap();
        let cutoff = Instant::now() - Duration::from_secs(60);

        // Remove stale entries
        nodes.retain(|_, (_, seen)| *seen > cutoff);

        nodes.values().map(|(b, seen)| {
            DiscoveredNode {
                service: b.service.clone(),
                hostname: b.hostname.clone(),
                address: b.address.clone(),
                version: b.version.clone(),
                managed: b.managed,
                cluster_id: b.cluster_id.clone(),
                san_node_id: b.san_node_id.clone(),
                san_volumes: b.san_volumes,
                last_seen: b.timestamp.clone(),
                age_secs: seen.elapsed().as_secs(),
            }
        }).collect()
    }

    /// Get only unmanaged vmm-server nodes (candidates for host registration).
    pub fn unmanaged_servers(&self) -> Vec<DiscoveredNode> {
        self.list().into_iter()
            .filter(|n| n.service == "vmm-server" && !n.managed)
            .collect()
    }

    /// Get all discovered vmm-san instances.
    pub fn san_nodes(&self) -> Vec<DiscoveredNode> {
        self.list().into_iter()
            .filter(|n| n.service == "vmm-san")
            .collect()
    }
}

/// Spawn the discovery listener as a background task.
pub fn spawn(store: Arc<DiscoveryStore>) {
    std::thread::spawn(move || {
        let socket = match UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Discovery listener: cannot bind UDP port {}: {}", DISCOVERY_PORT, e);
                return;
            }
        };

        tracing::info!("Discovery listener started on UDP port {}", DISCOVERY_PORT);

        let mut buf = [0u8; 4096];
        loop {
            let (len, src) = match socket.recv_from(&mut buf) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if len < 5 || &buf[..4] != DISCOVERY_MAGIC {
                continue;
            }

            let json = &buf[4..len];
            match serde_json::from_slice::<DiscoveryBeacon>(json) {
                Ok(beacon) => {
                    tracing::debug!("Discovery: {} {} at {} (managed={})",
                        beacon.service, beacon.hostname, beacon.address, beacon.managed);
                    store.insert(beacon);
                }
                Err(e) => {
                    tracing::debug!("Discovery: invalid beacon from {}: {}", src, e);
                }
            }
        }
    });
}
