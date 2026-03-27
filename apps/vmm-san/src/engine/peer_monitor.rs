//! Peer monitor — heartbeat between CoreSAN peers.
//!
//! Operates completely independently of vmm-cluster. CoreSAN peers
//! maintain their own heartbeat loop and detect failures autonomously.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::{CoreSanState, PeerStatus, QuorumStatus};
use crate::peer::client::PeerClient;

const HEARTBEAT_INTERVAL_SECS: u64 = 5;
const MAX_MISSED_HEARTBEATS: u32 = 3;

/// Pure quorum calculation — no state, no IO. Testable.
pub fn calculate_quorum_status(
    total_nodes: usize,
    reachable_nodes: usize,
    witness_allowed: Option<bool>,
) -> QuorumStatus {
    if total_nodes <= 1 {
        return QuorumStatus::Solo;
    }
    let majority = (total_nodes / 2) + 1;
    if reachable_nodes >= majority {
        return if reachable_nodes == total_nodes {
            QuorumStatus::Active
        } else {
            QuorumStatus::Degraded
        };
    }
    if witness_allowed == Some(true) {
        return QuorumStatus::Degraded;
    }
    QuorumStatus::Fenced
}

/// Pure leader calculation — no state, no IO. Testable.
pub fn calculate_is_leader(
    our_node_id: &str,
    online_peer_ids: &[&str],
    quorum: QuorumStatus,
    total_peers: usize,
) -> bool {
    if quorum == QuorumStatus::Fenced {
        return false;
    }
    // Solo node is always leader
    if quorum == QuorumStatus::Solo {
        return true;
    }
    // If we know about peers but none are online yet, don't claim leadership
    // (avoids split-brain during startup when all nodes think they're alone)
    if total_peers > 0 && online_peer_ids.is_empty() {
        return false;
    }
    online_peer_ids.iter().all(|peer_id| *peer_id >= our_node_id)
}

/// Spawn the peer monitor as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        let client = PeerClient::new(&state.config.peer.secret);
        let mut fenced_cycles: u32 = 0;

        loop {
            tick.tick().await;
            heartbeat_all_peers(&state, &client).await;

            // Compute quorum
            let new_quorum = compute_quorum(&state).await;
            let old_quorum = *state.quorum_status.read().unwrap();

            // Hysteresis: require 2 consecutive fenced cycles before transitioning
            let effective_quorum = if new_quorum == QuorumStatus::Fenced {
                fenced_cycles += 1;
                if fenced_cycles >= 2 {
                    QuorumStatus::Fenced
                } else {
                    old_quorum
                }
            } else {
                fenced_cycles = 0;
                new_quorum
            };

            // Log state transitions
            if effective_quorum != old_quorum {
                match effective_quorum {
                    QuorumStatus::Fenced => {
                        tracing::error!("Node FENCED: no quorum, witness denied");
                    }
                    QuorumStatus::Active if old_quorum == QuorumStatus::Fenced => {
                        tracing::info!("Node recovered from fenced state");
                    }
                    QuorumStatus::Degraded if old_quorum == QuorumStatus::Fenced => {
                        tracing::info!("Node recovered from fenced state");
                    }
                    QuorumStatus::Active if old_quorum == QuorumStatus::Degraded => {
                        tracing::info!("All peers reachable, quorum fully healthy");
                    }
                    QuorumStatus::Degraded => {
                        let unreachable = state.peers.iter()
                            .filter(|p| p.status != PeerStatus::Online).count();
                        tracing::warn!("Quorum degraded: {} peer(s) unreachable", unreachable);
                    }
                    _ => {}
                }
                *state.quorum_status.write().unwrap() = effective_quorum;
            }

            // Leader election
            let new_leader = compute_is_leader(&state, effective_quorum);
            let old_leader = state.is_leader.load(std::sync::atomic::Ordering::Relaxed);
            if new_leader != old_leader {
                state.is_leader.store(new_leader, std::sync::atomic::Ordering::Relaxed);
                if new_leader {
                    tracing::info!("This node is now the leader");
                } else {
                    tracing::info!("This node is no longer the leader");
                }
            }
        }
    });
}

async fn heartbeat_all_peers(state: &CoreSanState, client: &PeerClient) {
    let peer_ids: Vec<String> = state.peers.iter()
        .map(|p| p.node_id.clone())
        .collect();

    if peer_ids.is_empty() {
        return;
    }

    let uptime = state.started_at.elapsed().as_secs();
    let our_address = format!("http://{}:{}",
        crate::engine::discovery::get_local_ip_cached(), state.config.server.port);
    let is_leader = state.is_leader.load(std::sync::atomic::Ordering::Relaxed);

    for peer_id in peer_ids {
        let (address, hostname) = match state.peers.get(&peer_id) {
            Some(p) => (p.address.clone(), p.hostname.clone()),
            None => continue,
        };

        match client.heartbeat(&address, &state.node_id, &state.hostname, uptime, &our_address, is_leader).await {
            Ok(_) => {
                if let Some(mut peer) = state.peers.get_mut(&peer_id) {
                    if peer.status != PeerStatus::Online {
                        tracing::info!("Peer {} ({}) at {} is now online", hostname, peer_id, address);
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
            Err(e) => {
                if let Some(mut peer) = state.peers.get_mut(&peer_id) {
                    peer.missed_heartbeats += 1;

                    if peer.missed_heartbeats == 1 {
                        tracing::warn!("Heartbeat to {} ({}) at {} failed: {}",
                            hostname, peer_id, address, e);
                    }

                    if peer.missed_heartbeats >= MAX_MISSED_HEARTBEATS
                        && peer.status != PeerStatus::Offline
                    {
                        tracing::warn!("Peer {} ({}) at {} is now OFFLINE ({} missed heartbeats)",
                            hostname, peer_id, address, peer.missed_heartbeats);
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

/// Compute quorum status based on reachable peers and optional witness.
async fn compute_quorum(state: &CoreSanState) -> QuorumStatus {
    let total_peers = state.peers.len();
    let total_nodes = 1 + total_peers;
    let reachable_peers = state.peers.iter()
        .filter(|p| p.status == PeerStatus::Online)
        .count();
    let reachable = 1 + reachable_peers;

    // Try witness if no majority
    let majority = (total_nodes / 2) + 1;
    let witness_allowed = if reachable < majority {
        let witness_url = &state.config.cluster.witness_url;
        if !witness_url.is_empty() {
            match PeerClient::witness_check(witness_url, &state.node_id).await {
                Ok(allowed) => {
                    tracing::debug!("Witness check: allowed={}", allowed);
                    Some(allowed)
                }
                Err(e) => {
                    tracing::warn!("Witness unreachable: {}", e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    calculate_quorum_status(total_nodes, reachable, witness_allowed)
}

/// Determine if this node is the leader (lowest node_id among Active/Degraded nodes).
fn compute_is_leader(state: &CoreSanState, quorum: QuorumStatus) -> bool {
    let total_peers = state.peers.len();
    let online_ids: Vec<String> = state.peers.iter()
        .filter(|p| p.status == PeerStatus::Online)
        .map(|p| p.node_id.clone())
        .collect();
    let refs: Vec<&str> = online_ids.iter().map(|s| s.as_str()).collect();
    calculate_is_leader(&state.node_id, &refs, quorum, total_peers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_node() {
        assert_eq!(calculate_quorum_status(1, 1, None), QuorumStatus::Solo);
    }

    #[test]
    fn two_nodes_all_online() {
        assert_eq!(calculate_quorum_status(2, 2, None), QuorumStatus::Active);
    }

    #[test]
    fn two_nodes_one_offline_no_witness() {
        assert_eq!(calculate_quorum_status(2, 1, None), QuorumStatus::Fenced);
    }

    #[test]
    fn two_nodes_one_offline_witness_allows() {
        assert_eq!(calculate_quorum_status(2, 1, Some(true)), QuorumStatus::Degraded);
    }

    #[test]
    fn two_nodes_one_offline_witness_denies() {
        assert_eq!(calculate_quorum_status(2, 1, Some(false)), QuorumStatus::Fenced);
    }

    #[test]
    fn three_nodes_all_online() {
        assert_eq!(calculate_quorum_status(3, 3, None), QuorumStatus::Active);
    }

    #[test]
    fn three_nodes_one_offline() {
        assert_eq!(calculate_quorum_status(3, 2, None), QuorumStatus::Degraded);
    }

    #[test]
    fn three_nodes_two_offline() {
        assert_eq!(calculate_quorum_status(3, 1, None), QuorumStatus::Fenced);
    }

    #[test]
    fn five_nodes_two_offline() {
        assert_eq!(calculate_quorum_status(5, 3, None), QuorumStatus::Degraded);
    }

    #[test]
    fn five_nodes_three_offline() {
        assert_eq!(calculate_quorum_status(5, 2, None), QuorumStatus::Fenced);
    }

    #[test]
    fn ten_nodes_four_offline() {
        assert_eq!(calculate_quorum_status(10, 6, None), QuorumStatus::Degraded);
    }

    #[test]
    fn leader_lowest_id() {
        assert!(calculate_is_leader("aaa", &["bbb", "ccc"], QuorumStatus::Active, 2));
    }

    #[test]
    fn leader_not_lowest() {
        assert!(!calculate_is_leader("ccc", &["aaa", "bbb"], QuorumStatus::Active, 2));
    }

    #[test]
    fn leader_fenced_never() {
        assert!(!calculate_is_leader("aaa", &["bbb"], QuorumStatus::Fenced, 1));
    }

    #[test]
    fn leader_solo_always() {
        assert!(calculate_is_leader("aaa", &[], QuorumStatus::Solo, 0));
    }

    #[test]
    fn leader_no_online_peers_not_leader() {
        // If we have peers but none are online, don't claim leadership
        assert!(!calculate_is_leader("aaa", &[], QuorumStatus::Degraded, 2));
    }
}
