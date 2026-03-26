//! Repair engine — detects under-replicated chunks and creates new replicas.
//!
//! Runs periodically. When a node loses chunks (disk failure, hot-remove),
//! the repair engine copies chunks from surviving nodes to restore FTT.
//! This is the cross-node self-healing mechanism.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::peer::client::PeerClient;
use crate::storage::chunk;

/// Spawn the repair engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    let repair_interval = state.config.integrity.repair_interval_secs;
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(repair_interval));
        let client = PeerClient::new(&state.config.peer.secret);

        loop {
            tick.tick().await;
            if !state.is_leader.load(std::sync::atomic::Ordering::Relaxed) {
                tracing::trace!("Not leader, skipping repair cycle");
                continue;
            }
            run_chunk_repair(&state, &client).await;
            update_protection_status(&state);
        }
    });
}

/// Find chunks that have fewer synced replicas across nodes than required by FTT.
async fn run_chunk_repair(state: &CoreSanState, client: &PeerClient) {
    let under_replicated: Vec<(i64, i64, u32, String, u32, u32)> = {
        let db = state.db.lock().unwrap();

        // Find chunks where the number of distinct nodes with synced replicas < FTT+1
        let mut stmt = db.prepare(
            "SELECT fc.id, fc.file_id, fc.chunk_index, fm.volume_id, v.ftt,
                    (SELECT COUNT(DISTINCT cr.node_id) FROM chunk_replicas cr
                     WHERE cr.chunk_id = fc.id AND cr.state = 'synced') AS synced_nodes
             FROM file_chunks fc
             JOIN file_map fm ON fm.id = fc.file_id
             JOIN volumes v ON v.id = fm.volume_id
             WHERE synced_nodes < (v.ftt + 1)
             LIMIT 200"
        ).unwrap();

        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    if under_replicated.is_empty() {
        return;
    }

    tracing::info!("Repair: {} under-replicated chunks found", under_replicated.len());

    for (chunk_id, file_id, chunk_index, volume_id, ftt, synced_nodes) in under_replicated {
        let needed = (ftt + 1).saturating_sub(synced_nodes);
        for _ in 0..needed {
            repair_single_chunk(state, client, chunk_id, file_id, chunk_index, &volume_id).await;
        }
    }
}

async fn repair_single_chunk(
    state: &CoreSanState,
    client: &PeerClient,
    chunk_id: i64,
    file_id: i64,
    chunk_index: u32,
    volume_id: &str,
) {
    // Find a source node that has a synced copy of this chunk
    let source = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT cr.node_id, b.path FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             WHERE cr.chunk_id = ?1 AND cr.state = 'synced'
             LIMIT 1",
            rusqlite::params![chunk_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok()
    };

    let (source_node_id, source_backend_path) = match source {
        Some(s) => s,
        None => {
            tracing::warn!("Repair: no source for chunk {} (file {}, index {})", chunk_id, file_id, chunk_index);
            return;
        }
    };

    // Find a target node that does NOT have this chunk
    let nodes_with_chunk: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT DISTINCT node_id FROM chunk_replicas WHERE chunk_id = ?1 AND state = 'synced'"
        ).unwrap();
        stmt.query_map(rusqlite::params![chunk_id], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    // Try local node first (if we don't have it)
    if !nodes_with_chunk.contains(&state.node_id) {
        // Pull from source and store locally
        if source_node_id == state.node_id {
            // Source is local but somehow we're missing it? Skip.
            return;
        }

        let peer_addr = match state.peers.get(&source_node_id) {
            Some(p) => p.address.clone(),
            None => return,
        };

        // Pull the chunk data from the peer
        let src_path = chunk::chunk_path(&source_backend_path, volume_id, file_id, chunk_index);
        let rel = format!(".coresan/{}/{}/chunk_{:06}", volume_id, file_id, chunk_index);

        match client.pull_file(&peer_addr, volume_id, &rel).await {
            Ok(data) => {
                // Find a local backend to store it
                let local_backend = {
                    let db = state.db.lock().unwrap();
                    db.query_row(
                        "SELECT id, path FROM backends WHERE node_id = ?1 AND status = 'online'
                         ORDER BY free_bytes DESC LIMIT 1",
                        rusqlite::params![&state.node_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    ).ok()
                };

                if let Some((backend_id, backend_path)) = local_backend {
                    let dst = chunk::chunk_path(&backend_path, volume_id, file_id, chunk_index);
                    if let Some(parent) = dst.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    if std::fs::write(&dst, &data).is_ok() {
                        let db = state.db.lock().unwrap();
                        let now = chrono::Utc::now().to_rfc3339();
                        db.execute(
                            "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
                             VALUES (?1, ?2, ?3, 'synced', ?4)",
                            rusqlite::params![chunk_id, &backend_id, &state.node_id, &now],
                        ).ok();
                        tracing::info!("Repair: pulled chunk {} (file {}, idx {}) from {}",
                            chunk_id, file_id, chunk_index, source_node_id);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Repair: failed to pull chunk {}: {}", chunk_id, e);
            }
        }
        return;
    }

    // We have it locally — push to a peer that doesn't
    let target_peer = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .find(|p| !nodes_with_chunk.contains(&p.node_id))
        .map(|p| (p.node_id.clone(), p.address.clone()));

    if let Some((target_node_id, target_addr)) = target_peer {
        let src = chunk::chunk_path(&source_backend_path, volume_id, file_id, chunk_index);
        if let Ok(data) = std::fs::read(&src) {
            let rel = format!(".coresan/{}/{}/chunk_{:06}", volume_id, file_id, chunk_index);
            if client.push_file(&target_addr, volume_id, &rel, data).await.is_ok() {
                tracing::info!("Repair: pushed chunk {} (file {}, idx {}) to {}",
                    chunk_id, file_id, chunk_index, target_node_id);
            }
        }
    }
}

/// Update protection_status for all files based on current chunk replica state.
fn update_protection_status(state: &CoreSanState) {
    let db = state.db.lock().unwrap();

    // Get all files with their volume's FTT
    let files: Vec<(i64, String, u32)> = {
        let mut stmt = db.prepare(
            "SELECT fm.id, fm.volume_id, v.ftt FROM file_map fm
             JOIN volumes v ON v.id = fm.volume_id
             WHERE fm.chunk_count > 0"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    for (file_id, _volume_id, ftt) in files {
        let status = chunk::compute_protection_status(&db, file_id, ftt);
        db.execute(
            "UPDATE file_map SET protection_status = ?1 WHERE id = ?2",
            rusqlite::params![status, file_id],
        ).ok();
    }
}
