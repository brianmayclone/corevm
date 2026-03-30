//! Repair engine — detects under-replicated chunks and creates new replicas.
//!
//! Runs periodically on the leader node. When a node loses chunks (disk failure,
//! hot-remove), the repair engine copies chunks from surviving nodes to restore FTT.
//! Uses the chunk-level API endpoints for peer-to-peer chunk transfers.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::peer::client::PeerClient;
use crate::storage::chunk;

/// Spawn the repair engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        // Short initial delay to let peers connect
        tokio::time::sleep(Duration::from_secs(10)).await;
        let mut tick = interval(Duration::from_secs(10));
        let client = PeerClient::new(&state.config.peer.secret);

        loop {
            tick.tick().await;
            run_chunk_repair(&state, &client).await;
            update_protection_status(&state);
        }
    });
}

/// Batch size for querying under-replicated chunks.
const QUERY_BATCH_SIZE: u32 = 500;

async fn run_chunk_repair(state: &CoreSanState, client: &PeerClient) {
    use crate::services::chunk::ChunkService;

    let mut total_repaired = 0u32;

    loop {
        let under_replicated = {
            let db = state.db.lock().unwrap();
            ChunkService::find_under_replicated(&db, QUERY_BATCH_SIZE)
        };

        if under_replicated.is_empty() {
            break;
        }

        if total_repaired == 0 {
            tracing::info!("Repair: {} under-replicated chunks found", under_replicated.len());
        }

        let mut batch_repaired = 0u32;
        for (chunk_id, file_id, chunk_index, volume_id, ftt, synced_nodes) in under_replicated {
            let needed = (ftt + 1).saturating_sub(synced_nodes);
            for _ in 0..needed {
                if repair_single_chunk(state, client, chunk_id, file_id, chunk_index, &volume_id).await {
                    batch_repaired += 1;
                }
            }
        }

        total_repaired += batch_repaired;

        if batch_repaired == 0 {
            // No progress — peer likely offline, wait and retry next cycle
            tracing::warn!("Repair: no progress (peer offline?), will retry next cycle");
            break;
        }

        tracing::info!("Repair: {} chunks repaired so far", total_repaired);
    }

    if total_repaired > 0 {
        tracing::info!("Repair: cycle complete, {} chunks repaired total", total_repaired);
    }
}

/// Returns true if repair succeeded.
async fn repair_single_chunk(
    state: &CoreSanState,
    client: &PeerClient,
    chunk_id: i64,
    file_id: i64,
    chunk_index: u32,
    volume_id: &str,
) -> bool {
    use crate::services::chunk::ChunkService;

    let source_node_id = {
        let db = state.db.lock().unwrap();
        ChunkService::find_chunk_source(&db, chunk_id)
    };

    let source_node_id = match source_node_id {
        Some(id) => id,
        None => {
            tracing::warn!("Repair: no source for chunk {} (file {}, index {})", chunk_id, file_id, chunk_index);
            return false;
        }
    };

    let nodes_with_chunk: Vec<String> = {
        let db = state.db.lock().unwrap();
        ChunkService::nodes_with_chunk(&db, chunk_id)
    };

    // Try local node first (if we don't have it)
    if !nodes_with_chunk.contains(&state.node_id) {
        if source_node_id == state.node_id {
            return false;
        }

        let peer_addr = match state.peers.get(&source_node_id) {
            Some(p) => p.address.clone(),
            None => return false,
        };

        // Pull the chunk via the chunk API endpoint (peer verifies SHA256 before serving)
        match client.pull_chunk(&peer_addr, volume_id, file_id, chunk_index).await {
            Ok(data) => {
                let expected_sha = {
                    let db = state.db.lock().unwrap();
                    ChunkService::get_chunk_sha256(&db, chunk_id)
                };

                use sha2::{Sha256, Digest};
                let actual_sha = format!("{:x}", Sha256::digest(&data));

                if let Some(ref expected) = expected_sha {
                    if *expected != actual_sha {
                        tracing::warn!("Repair: SHA256 MISMATCH from peer {} for chunk {} — rejecting",
                            source_node_id, chunk_id);
                        return false;
                    }
                }

                // Find a local backend to store it
                let local_backend = {
                    let db = state.db.lock().unwrap();
                    let local_raid: String = db.query_row(
                        "SELECT local_raid FROM volumes WHERE id = ?1",
                        rusqlite::params![volume_id], |row| row.get(0),
                    ).unwrap_or_else(|_| "stripe".into());
                    let placements = chunk::place_chunk(&db, volume_id, &state.node_id, chunk_index, &local_raid);
                    placements.into_iter().next()
                };

                if let Some((backend_id, backend_path)) = local_backend {
                    let dst = chunk::chunk_path(&backend_path, volume_id, file_id, chunk_index);
                    if let Some(parent) = dst.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }

                    // Atomic write: tmp + fsync + rename
                    let tmp = dst.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                    if std::fs::write(&tmp, &data).is_ok() {
                        if let Ok(f) = std::fs::File::open(&tmp) {
                            f.sync_all().ok();
                        }
                        if std::fs::rename(&tmp, &dst).is_ok() {
                            let db = state.db.lock().unwrap();
                            log_err!(ChunkService::set_replica_synced(&db, chunk_id, &backend_id, &state.node_id),
                                "repair: set_replica_synced");
                            if expected_sha.is_none() {
                                log_err!(ChunkService::update_chunk_sha256_by_id(&db, chunk_id, &actual_sha),
                                    "repair: update_chunk_sha256");
                            }
                            tracing::info!("Repair: pulled chunk {} (file {}, idx {}) from {} (verified sha={})",
                                chunk_id, file_id, chunk_index, source_node_id, &actual_sha[..8]);
                            return true;
                        } else {
                            std::fs::remove_file(&tmp).ok();
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Repair: failed to pull chunk {}: {}", chunk_id, e);
            }
        }
        return false;
    }

    // We have it locally — verify integrity before pushing to peer
    let local_chunk_info = {
        let db = state.db.lock().unwrap();
        ChunkService::find_local_chunk_path(&db, chunk_id, &state.node_id)
    };

    let (backend_path, expected_sha) = match local_chunk_info {
        Some(info) => info,
        None => return false,
    };

    let src = chunk::chunk_path(&backend_path, volume_id, file_id, chunk_index);
    let data = match std::fs::read(&src) {
        Ok(d) => d,
        Err(_) => return false,
    };

    // Verify our own copy before spreading it
    if !expected_sha.is_empty() {
        use sha2::{Sha256, Digest};
        let local_sha = format!("{:x}", Sha256::digest(&data));
        if local_sha != expected_sha {
            tracing::warn!("Repair: LOCAL chunk {} is corrupt (expected={}, actual={}) — skipping push",
                chunk_id, &expected_sha[..8], &local_sha[..8]);
            let db = state.db.lock().unwrap();
            log_err!(ChunkService::mark_replica_error(&db, chunk_id, &state.node_id),
                "repair: mark local chunk error");
            return false;
        }
    }

    // Push to a peer that doesn't have it
    let target_peer = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .find(|p| !nodes_with_chunk.contains(&p.node_id))
        .map(|p| (p.node_id.clone(), p.address.clone()));

    if let Some((target_node_id, target_addr)) = target_peer {
        // Get rel_path so receiver can resolve its local file_id
        let rel_path = {
            let db = state.db.lock().unwrap();
            db.query_row("SELECT rel_path FROM file_map WHERE id = ?1",
                rusqlite::params![file_id], |row| row.get::<_, String>(0))
                .unwrap_or_default()
        };

        if client.push_chunk_full(&target_addr, volume_id, file_id, chunk_index, data, &rel_path, &state.node_id).await.is_ok() {
            // Record in our LOCAL DB that this peer now has the chunk.
            // Without this, the repair engine would keep thinking the chunk is under-replicated.
            let db = state.db.lock().unwrap();
            log_err!(ChunkService::track_remote_replica(&db, chunk_id, &target_node_id),
                "repair: track remote replica");

            tracing::info!("Repair: pushed chunk {} (file {}, idx {}) to {} (verified, tracked locally)",
                chunk_id, file_id, chunk_index, target_node_id);
            return true;
        }
    }

    false
}

/// Update protection_status for all files based on current chunk replica state.
fn update_protection_status(state: &CoreSanState) {
    use crate::services::file::FileService;

    let db = state.db.lock().unwrap();
    let files = FileService::list_files_with_ftt(&db);

    for (file_id, _volume_id, ftt) in files {
        let status = chunk::compute_protection_status(&db, file_id, ftt);
        log_err!(FileService::update_protection_status(&db, file_id, status),
            "repair: update_protection_status");
    }
}
