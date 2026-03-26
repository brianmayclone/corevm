//! Peer monitor — heartbeat between CoreSAN peers.
//!
//! Operates completely independently of vmm-cluster. CoreSAN peers
//! maintain their own heartbeat loop and detect failures autonomously.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::{CoreSanState, PeerStatus};
use crate::peer::client::PeerClient;

const HEARTBEAT_INTERVAL_SECS: u64 = 5;
const MAX_MISSED_HEARTBEATS: u32 = 3;

/// Spawn the peer monitor as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        let client = PeerClient::new(&state.config.peer.secret);

        loop {
            tick.tick().await;
            heartbeat_all_peers(&state, &client).await;
        }
    });
}

async fn heartbeat_all_peers(state: &CoreSanState, client: &PeerClient) {
    let peer_ids: Vec<String> = state.peers.iter()
        .map(|p| p.node_id.clone())
        .collect();

    let uptime = state.started_at.elapsed().as_secs();

    for peer_id in peer_ids {
        let address = match state.peers.get(&peer_id) {
            Some(p) => p.address.clone(),
            None => continue,
        };

        match client.heartbeat(&address, &state.node_id, &state.hostname, uptime).await {
            Ok(_) => {
                if let Some(mut peer) = state.peers.get_mut(&peer_id) {
                    if peer.status != PeerStatus::Online {
                        tracing::info!("Peer {} ({}) is now online", peer.hostname, peer_id);
                    }
                    peer.status = PeerStatus::Online;
                    peer.missed_heartbeats = 0;
                }
                // Update DB
                let db = state.db.lock().unwrap();
                let now = chrono::Utc::now().to_rfc3339();
                db.execute(
                    "UPDATE peers SET status = 'online', last_heartbeat = ?1 WHERE node_id = ?2",
                    rusqlite::params![&now, &peer_id],
                ).ok();
            }
            Err(_) => {
                if let Some(mut peer) = state.peers.get_mut(&peer_id) {
                    peer.missed_heartbeats += 1;

                    if peer.missed_heartbeats >= MAX_MISSED_HEARTBEATS
                        && peer.status != PeerStatus::Offline
                    {
                        tracing::warn!("Peer {} ({}) is now OFFLINE ({} missed heartbeats)",
                            peer.hostname, peer_id, peer.missed_heartbeats);
                        peer.status = PeerStatus::Offline;

                        // Update DB
                        let db = state.db.lock().unwrap();
                        db.execute(
                            "UPDATE peers SET status = 'offline' WHERE node_id = ?1",
                            rusqlite::params![&peer_id],
                        ).ok();

                        // Mark all backends on this peer as offline
                        db.execute(
                            "UPDATE backends SET status = 'offline' WHERE node_id = ?1",
                            rusqlite::params![&peer_id],
                        ).ok();

                        // Release all write leases held by the offline node
                        crate::engine::write_lease::release_all_leases_for_node(&db, &peer_id);
                    }
                }
            }
        }
    }
}
