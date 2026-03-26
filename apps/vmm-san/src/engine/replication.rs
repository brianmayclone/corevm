//! Background replication engine — processes stale replicas and syncs files.
//!
//! Operates autonomously without vmm-cluster — peers talk directly to each other.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::peer::client::PeerClient;
use crate::storage::file_map;

/// Spawn the replication engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(5));
        let client = PeerClient::new(&state.config.peer.secret);

        loop {
            tick.tick().await;
            let quorum = *state.quorum_status.read().unwrap();
            if quorum == crate::state::QuorumStatus::Fenced {
                tracing::trace!("Node fenced, skipping replication cycle");
                continue;
            }
            process_stale_replicas(&state, &client).await;
        }
    });
}

/// Process all stale replicas — re-sync them from a healthy source.
async fn process_stale_replicas(state: &CoreSanState, client: &PeerClient) {
    let stale = {
        let db = state.db.lock().unwrap();
        file_map::find_stale_replicas(&db)
    };

    for replica in stale {
        if replica.node_id == state.node_id {
            sync_local_replica(state, client, &replica).await;
        } else {
            sync_remote_replica(state, client, &replica).await;
        }
    }
}

/// Sync a stale local replica by pulling data from a peer.
async fn sync_local_replica(
    state: &CoreSanState,
    client: &PeerClient,
    replica: &file_map::StaleReplica,
) {
    // Find a peer that has a synced copy — lock scope limited to this block
    let source = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT b.node_id, b.path FROM file_replicas fr
             JOIN backends b ON b.id = fr.backend_id
             WHERE fr.file_id = ?1 AND fr.state = 'synced' AND b.node_id != ?2
             LIMIT 1",
            rusqlite::params![replica.file_id, &state.node_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok()
    };

    let (source_node, _) = match source {
        Some(s) => s,
        None => return,
    };

    let peer_addr = match state.peers.get(&source_node) {
        Some(p) => p.address.clone(),
        None => return,
    };

    match client.pull_file(&peer_addr, &replica.volume_id, &replica.rel_path).await {
        Ok(data) => {
            let dest = std::path::Path::new(&replica.backend_path).join(&replica.rel_path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            if std::fs::write(&dest, &data).is_ok() {
                let db = state.db.lock().unwrap();
                let now = chrono::Utc::now().to_rfc3339();
                db.execute(
                    "UPDATE file_replicas SET state = 'synced', synced_at = ?1
                     WHERE file_id = ?2 AND backend_id = ?3",
                    rusqlite::params![&now, replica.file_id, &replica.backend_id],
                ).ok();
                tracing::debug!("Synced local replica: {}/{}", replica.volume_id, replica.rel_path);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to sync local replica {}/{}: {}",
                replica.volume_id, replica.rel_path, e);
        }
    }
}

/// Push a local synced copy to a remote peer that has a stale replica.
async fn sync_remote_replica(
    state: &CoreSanState,
    client: &PeerClient,
    replica: &file_map::StaleReplica,
) {
    let local_path = {
        let db = state.db.lock().unwrap();
        file_map::find_local_replica(&db, &replica.volume_id, &replica.rel_path, &state.node_id)
    };

    let local_path = match local_path {
        Some(p) => p,
        None => return,
    };

    let data = match std::fs::read(&local_path) {
        Ok(d) => d,
        Err(_) => return,
    };

    let peer_addr = match state.peers.get(&replica.node_id) {
        Some(p) => p.address.clone(),
        None => return,
    };

    match client.push_file(&peer_addr, &replica.volume_id, &replica.rel_path, data).await {
        Ok(_) => {
            tracing::debug!("Pushed replica to peer: {}/{} -> {}",
                replica.volume_id, replica.rel_path, replica.node_id);
        }
        Err(e) => {
            tracing::warn!("Failed to push replica {}/{} to {}: {}",
                replica.volume_id, replica.rel_path, replica.node_id, e);
        }
    }
}
