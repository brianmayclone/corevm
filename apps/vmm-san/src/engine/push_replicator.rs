//! Push-based replication — immediately distributes writes to all peers.
//!
//! Instead of waiting for the 5-second stale-replica poll, the push replicator
//! watches the write_log and immediately pushes data to peers that have backends
//! for the same volume. This is the "massively fast" replication path.
//!
//! ## Architecture:
//! - A tokio channel (mpsc) receives write events from FUSE/API write paths
//! - A background task processes the channel and pushes to peers concurrently
//! - On failure, the stale-replica poller (engine/replication.rs) catches up later

use std::sync::Arc;
use tokio::sync::mpsc;
use crate::state::CoreSanState;
use crate::peer::client::PeerClient;

/// A write event that needs to be pushed to peers.
#[derive(Clone, Debug)]
pub struct WriteEvent {
    pub volume_id: String,
    pub rel_path: String,
    pub version: i64,
    pub data: Arc<Vec<u8>>,
    pub writer_node_id: String,
}

/// Channel sender — cloned into FUSE and API handlers.
pub type WriteSender = mpsc::UnboundedSender<WriteEvent>;

/// Spawn the push replicator with an existing receiver (channel created in main).
pub fn spawn_with_rx(
    state: Arc<CoreSanState>,
    rx: mpsc::UnboundedReceiver<WriteEvent>,
) {
    tokio::spawn(async move {
        run_push_replicator(state, rx).await;
    });
}

async fn run_push_replicator(
    state: Arc<CoreSanState>,
    mut rx: mpsc::UnboundedReceiver<WriteEvent>,
) {
    let client = PeerClient::new(&state.config.peer.secret);

    while let Some(event) = rx.recv().await {
        // Skip push if node is fenced
        let quorum = *state.quorum_status.read().unwrap();
        if quorum == crate::state::QuorumStatus::Fenced {
            tracing::trace!("Node fenced, dropping push event");
            continue;
        }

        // Find all online peers to replicate to
        let targets: Vec<(String, String)> = state.peers.iter()
            .filter(|p| p.status == crate::state::PeerStatus::Online)
            .filter(|p| p.node_id != event.writer_node_id)
            .map(|p| (p.node_id.clone(), p.address.clone()))
            .collect();

        if targets.is_empty() {
            continue;
        }

        // Push to all target peers concurrently
        let mut handles = Vec::new();
        for (target_node_id, peer_addr) in targets {
            let client = PeerClient::new(&state.config.peer.secret);
            let event = event.clone();

            let handle = tokio::spawn(async move {
                match client.push_file(
                    &peer_addr,
                    &event.volume_id,
                    &event.rel_path,
                    event.data.as_ref().clone(),
                ).await {
                    Ok(_) => {
                        tracing::info!("Replicated {}/{} v{} → {} ({} bytes)",
                            event.volume_id, event.rel_path, event.version,
                            target_node_id, event.data.len());
                        Some(target_node_id)
                    }
                    Err(e) => {
                        tracing::warn!("Push-replication failed for {}/{} → {}: {}",
                            event.volume_id, event.rel_path, target_node_id, e);
                        None
                    }
                }
            });
            handles.push(handle);
        }

        // Wait for all pushes to complete (or fail)
        for handle in handles {
            if let Ok(Some(_node_id)) = handle.await {
                // Successfully pushed — the remote node's file write API
                // will update its own file_replicas table
            }
        }
    }

    tracing::warn!("Push replicator channel closed");
}

/// Also clean up old write_log entries periodically.
pub fn spawn_log_cleanup(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(300));
        loop {
            tick.tick().await;
            let db = state.db.lock().unwrap();
            // Keep last 1 hour of write log
            db.execute(
                "DELETE FROM write_log WHERE written_at < datetime('now', '-1 hour')",
                [],
            ).ok();
        }
    });
}
