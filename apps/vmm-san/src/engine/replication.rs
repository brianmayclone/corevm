//! Background replication engine — processes stale chunk replicas and syncs them.
//!
//! Operates autonomously without vmm-cluster — peers talk directly to each other.
//! Works at chunk level: pulls/pushes individual chunks, not whole files.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::peer::client::PeerClient;
use crate::storage::chunk;

/// Spawn the replication engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(5));
        let client = PeerClient::new(&state.config.peer.secret);

        loop {
            tick.tick().await;
            let quorum = *state.quorum_status.read().unwrap();
            if quorum == crate::state::QuorumStatus::Fenced
                || quorum == crate::state::QuorumStatus::Sanitizing {
                tracing::trace!("Node fenced, skipping replication cycle");
                continue;
            }
            process_stale_chunk_replicas(&state, &client).await;
        }
    });
}

/// Stale chunk replica: a chunk on some node/backend that needs re-syncing.
struct StaleChunkReplica {
    chunk_id: i64,
    file_id: i64,
    chunk_index: u32,
    volume_id: String,
    backend_id: String,
    backend_path: String,
    node_id: String,
}

/// Process all stale chunk replicas — re-sync them from a healthy source.
async fn process_stale_chunk_replicas(state: &CoreSanState, client: &PeerClient) {
    let stale = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT cr.chunk_id, fc.file_id, fc.chunk_index, fm.volume_id,
                    cr.backend_id, b.path, cr.node_id
             FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN file_map fm ON fm.id = fc.file_id
             JOIN backends b ON b.id = cr.backend_id
             WHERE cr.state = 'stale'
             ORDER BY fc.file_id, fc.chunk_index
             LIMIT 200"
        ).unwrap();
        stmt.query_map([], |row| Ok(StaleChunkReplica {
            chunk_id: row.get(0)?,
            file_id: row.get(1)?,
            chunk_index: row.get(2)?,
            volume_id: row.get(3)?,
            backend_id: row.get(4)?,
            backend_path: row.get(5)?,
            node_id: row.get(6)?,
        })).unwrap().filter_map(|r| r.ok()).collect::<Vec<_>>()
    };

    if stale.is_empty() {
        return;
    }

    tracing::debug!("Replication: {} stale chunk replicas to sync", stale.len());

    for replica in stale {
        if replica.node_id == state.node_id {
            sync_local_chunk(state, client, &replica).await;
        } else {
            sync_remote_chunk(state, client, &replica).await;
        }
    }
}

/// Sync a stale local chunk replica by pulling chunk data from a peer.
async fn sync_local_chunk(
    state: &CoreSanState,
    client: &PeerClient,
    replica: &StaleChunkReplica,
) {
    // Find a peer that has a synced copy of this chunk
    let source_node_id = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT cr.node_id FROM chunk_replicas cr
             WHERE cr.chunk_id = ?1 AND cr.state = 'synced' AND cr.node_id != ?2
             LIMIT 1",
            rusqlite::params![replica.chunk_id, &state.node_id],
            |row| row.get::<_, String>(0),
        ).ok()
    };

    let source_node_id = match source_node_id {
        Some(id) => id,
        None => return,
    };

    let peer_addr = match state.peers.get(&source_node_id) {
        Some(p) => p.address.clone(),
        None => return,
    };

    match client.pull_chunk(
        &peer_addr, &replica.volume_id, replica.file_id, replica.chunk_index,
    ).await {
        Ok(data) => {
            // Write chunk to local backend
            let path = chunk::chunk_path(
                &replica.backend_path, &replica.volume_id, replica.file_id, replica.chunk_index,
            );
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }

            // Atomic write
            let tmp = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
            if std::fs::write(&tmp, &data).is_ok() {
                if let Ok(f) = std::fs::File::open(&tmp) {
                    f.sync_all().ok();
                }
                if std::fs::rename(&tmp, &path).is_ok() {
                    let db = state.db.lock().unwrap();
                    let now = chrono::Utc::now().to_rfc3339();
                    db.execute(
                        "UPDATE chunk_replicas SET state = 'synced', synced_at = ?1
                         WHERE chunk_id = ?2 AND backend_id = ?3",
                        rusqlite::params![&now, replica.chunk_id, &replica.backend_id],
                    ).ok();

                    // Update chunk sha256
                    use sha2::{Sha256, Digest};
                    let sha256 = format!("{:x}", Sha256::digest(&data));
                    db.execute(
                        "UPDATE file_chunks SET sha256 = ?1 WHERE id = ?2",
                        rusqlite::params![&sha256, replica.chunk_id],
                    ).ok();

                    tracing::debug!("Synced local chunk: {}/{}/idx{}",
                        replica.volume_id, replica.file_id, replica.chunk_index);
                } else {
                    std::fs::remove_file(&tmp).ok();
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to sync local chunk {}/{}/idx{}: {}",
                replica.volume_id, replica.file_id, replica.chunk_index, e);
        }
    }
}

/// Push a local synced chunk to a remote peer that has a stale replica.
async fn sync_remote_chunk(
    state: &CoreSanState,
    client: &PeerClient,
    replica: &StaleChunkReplica,
) {
    // Find local synced replica of this chunk to use as source
    let local_source = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT b.path FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             WHERE cr.chunk_id = ?1 AND cr.node_id = ?2 AND cr.state = 'synced'
             LIMIT 1",
            rusqlite::params![replica.chunk_id, &state.node_id],
            |row| row.get::<_, String>(0),
        ).ok()
    };

    let source_backend_path = match local_source {
        Some(p) => p,
        None => return,
    };

    let chunk_path = chunk::chunk_path(
        &source_backend_path, &replica.volume_id, replica.file_id, replica.chunk_index,
    );
    let data = match std::fs::read(&chunk_path) {
        Ok(d) => d,
        Err(_) => return,
    };

    let peer_addr = match state.peers.get(&replica.node_id) {
        Some(p) => p.address.clone(),
        None => return,
    };

    match client.push_chunk(
        &peer_addr, &replica.volume_id, replica.file_id, replica.chunk_index, data,
    ).await {
        Ok(_) => {
            tracing::debug!("Pushed chunk to peer: {}/{}/idx{} -> {}",
                replica.volume_id, replica.file_id, replica.chunk_index, replica.node_id);
        }
        Err(e) => {
            tracing::warn!("Failed to push chunk {}/{}/idx{} to {}: {}",
                replica.volume_id, replica.file_id, replica.chunk_index, replica.node_id, e);
        }
    }
}
