//! Push-based replication — immediately distributes chunks to all peers.
//!
//! Instead of waiting for the 5-second stale-replica poll, the push replicator
//! reads chunk data from local backends and pushes individual chunks to peers.
//! This is the "massively fast" replication path.
//!
//! ## Architecture:
//! - A tokio channel (mpsc) receives write events from FUSE/API write paths
//! - A background task processes the channel and pushes chunks + metadata to peers
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
    pub file_id: i64,
    pub version: i64,
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
        if quorum == crate::state::QuorumStatus::Fenced
            || quorum == crate::state::QuorumStatus::Sanitizing {
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

        // Gather file metadata and chunk info for this file
        let (file_meta, chunks) = {
            let db = state.db.lock().unwrap();
            let meta = db.query_row(
                "SELECT fm.size_bytes, fm.sha256, fm.version, fm.chunk_count,
                        v.chunk_size_bytes, v.ftt, v.local_raid
                 FROM file_map fm
                 JOIN volumes v ON v.id = fm.volume_id
                 WHERE fm.id = ?1",
                rusqlite::params![event.file_id],
                |row| Ok(serde_json::json!({
                    "volume_id": event.volume_id,
                    "rel_path": event.rel_path,
                    "file_id": event.file_id,
                    "size_bytes": row.get::<_, u64>(0)?,
                    "sha256": row.get::<_, String>(1)?,
                    "version": row.get::<_, i64>(2)?,
                    "chunk_count": row.get::<_, u32>(3)?,
                    "chunk_size_bytes": row.get::<_, u64>(4)?,
                    "ftt": row.get::<_, u32>(5)?,
                    "local_raid": row.get::<_, String>(6)?,
                })),
            );

            let meta = match meta {
                Ok(m) => m,
                Err(_) => continue, // file was deleted before we could replicate
            };

            // Get all local synced chunk replicas for this file
            let mut stmt = db.prepare(
                "SELECT fc.chunk_index, fc.size_bytes, fc.sha256, b.path
                 FROM file_chunks fc
                 JOIN chunk_replicas cr ON cr.chunk_id = fc.id
                 JOIN backends b ON b.id = cr.backend_id
                 WHERE fc.file_id = ?1 AND cr.node_id = ?2 AND cr.state = 'synced'
                 GROUP BY fc.chunk_index"
            ).unwrap();
            let chunks: Vec<(u32, u64, String, String)> = stmt.query_map(
                rusqlite::params![event.file_id, &state.node_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).unwrap().filter_map(|r| r.ok()).collect();

            (meta, chunks)
        };

        // Push to all target peers concurrently
        for (target_node_id, peer_addr) in &targets {
            // 1. Push file metadata first (so peer knows about the file/chunks)
            if let Err(e) = client.push_file_meta(&peer_addr, &file_meta).await {
                tracing::warn!("Meta sync to {} failed: {}", target_node_id, e);
                continue;
            }

            // 2. Push each chunk
            let mut all_ok = true;
            for (chunk_index, _size, _sha256, backend_path) in &chunks {
                let chunk_path = crate::storage::chunk::chunk_path(
                    backend_path, &event.volume_id, event.file_id, *chunk_index,
                );
                let data = match std::fs::read(&chunk_path) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("Cannot read chunk {} for push: {}", chunk_path.display(), e);
                        all_ok = false;
                        continue;
                    }
                };

                let client = PeerClient::new(&state.config.peer.secret);
                match client.push_chunk(
                    &peer_addr, &event.volume_id, event.file_id, *chunk_index, data,
                ).await {
                    Ok(_) => {
                        tracing::info!("Replicated chunk {}/{}/idx{} → {}",
                            event.volume_id, event.file_id, chunk_index, target_node_id);
                    }
                    Err(e) => {
                        tracing::warn!("Chunk push failed {}/{}/idx{} → {}: {}",
                            event.volume_id, event.file_id, chunk_index, target_node_id, e);
                        all_ok = false;
                    }
                }
            }

            if all_ok && !chunks.is_empty() {
                tracing::info!("Replicated {}/{} v{} ({} chunks) → {}",
                    event.volume_id, event.rel_path, event.version,
                    chunks.len(), target_node_id);
            }
        }
    }

    tracing::warn!("Push replicator channel closed");
}

/// Clean up old write_log entries periodically.
/// Only deletes entries where ALL chunks of the file have been replicated to at least
/// one other node (i.e., not just time-based deletion).
pub fn spawn_log_cleanup(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(300));
        loop {
            tick.tick().await;
            let db = state.db.lock().unwrap();

            // Delete write_log entries older than 1 hour ONLY IF the file's chunks
            // have at least one synced replica on another node.
            // This prevents deleting logs for files that never got replicated.
            let deleted = db.execute(
                "DELETE FROM write_log WHERE written_at < datetime('now', '-1 hour')
                 AND file_id IN (
                    SELECT fm.id FROM file_map fm
                    WHERE NOT EXISTS (
                        SELECT 1 FROM file_chunks fc
                        JOIN volumes v ON v.id = fm.volume_id
                        WHERE fc.file_id = fm.id AND v.ftt > 0
                          AND (SELECT COUNT(DISTINCT cr.node_id) FROM chunk_replicas cr
                               WHERE cr.chunk_id = fc.id AND cr.state = 'synced') < 2
                    )
                 )",
                [],
            ).unwrap_or(0);

            // Also clean up very old entries (>24h) unconditionally as a safety net
            // to prevent unbounded growth if replication is permanently stuck
            let forced = db.execute(
                "DELETE FROM write_log WHERE written_at < datetime('now', '-24 hours')",
                [],
            ).unwrap_or(0);

            if deleted > 0 || forced > 0 {
                tracing::debug!("Write log cleanup: {} replicated, {} forced-expired", deleted, forced);
            }
        }
    });
}
