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
/// Rate limit: max chunks repaired per cycle to avoid thundering herd
const MAX_REPAIRS_PER_CYCLE: u32 = 30;

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
             LIMIT ?1"
        ).unwrap();

        stmt.query_map(rusqlite::params![MAX_REPAIRS_PER_CYCLE], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    if under_replicated.is_empty() {
        return;
    }

    tracing::info!("Repair: {} under-replicated chunks found (max {} per cycle)",
        under_replicated.len(), MAX_REPAIRS_PER_CYCLE);

    let mut repaired = 0u32;
    for (chunk_id, file_id, chunk_index, volume_id, ftt, synced_nodes) in under_replicated {
        if repaired >= MAX_REPAIRS_PER_CYCLE {
            break;
        }
        let needed = (ftt + 1).saturating_sub(synced_nodes);
        for _ in 0..needed {
            if repair_single_chunk(state, client, chunk_id, file_id, chunk_index, &volume_id).await {
                repaired += 1;
            }
        }
    }

    if repaired > 0 {
        tracing::info!("Repair: {} chunks repaired this cycle", repaired);
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
    // Find a source node that has a synced copy — prefer replicas that passed
    // a recent integrity check (have a sha256 stored).
    let source_node_id = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT cr.node_id FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE cr.chunk_id = ?1 AND cr.state = 'synced'
             ORDER BY CASE WHEN fc.sha256 != '' THEN 0 ELSE 1 END
             LIMIT 1",
            rusqlite::params![chunk_id],
            |row| row.get::<_, String>(0),
        ).ok()
    };

    let source_node_id = match source_node_id {
        Some(id) => id,
        None => {
            tracing::warn!("Repair: no source for chunk {} (file {}, index {})", chunk_id, file_id, chunk_index);
            return false;
        }
    };

    // Find nodes that already have this chunk
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
                // Verify received data against expected SHA256 if known
                let expected_sha = {
                    let db = state.db.lock().unwrap();
                    db.query_row(
                        "SELECT sha256 FROM file_chunks WHERE id = ?1",
                        rusqlite::params![chunk_id], |row| row.get::<_, String>(0),
                    ).ok().filter(|s| !s.is_empty())
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
                            let now = chrono::Utc::now().to_rfc3339();
                            db.execute("BEGIN IMMEDIATE", []).ok();
                            db.execute(
                                "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
                                 VALUES (?1, ?2, ?3, 'synced', ?4)",
                                rusqlite::params![chunk_id, &backend_id, &state.node_id, &now],
                            ).ok();
                            // Update SHA256 if we didn't have one
                            if expected_sha.is_none() {
                                db.execute(
                                    "UPDATE file_chunks SET sha256 = ?1 WHERE id = ?2",
                                    rusqlite::params![&actual_sha, chunk_id],
                                ).ok();
                            }
                            db.execute("COMMIT", []).ok();
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
        db.query_row(
            "SELECT b.path, COALESCE(fc.sha256, '') FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE cr.chunk_id = ?1 AND cr.node_id = ?2 AND cr.state = 'synced'
             LIMIT 1",
            rusqlite::params![chunk_id, &state.node_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok()
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
            db.execute(
                "UPDATE chunk_replicas SET state = 'error' WHERE chunk_id = ?1 AND node_id = ?2",
                rusqlite::params![chunk_id, &state.node_id],
            ).ok();
            return false;
        }
    }

    // Push to a peer that doesn't have it
    let target_peer = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .find(|p| !nodes_with_chunk.contains(&p.node_id))
        .map(|p| (p.node_id.clone(), p.address.clone()));

    if let Some((target_node_id, target_addr)) = target_peer {
        if client.push_chunk(&target_addr, volume_id, file_id, chunk_index, data).await.is_ok() {
            // Record in our LOCAL DB that this peer now has the chunk.
            // Without this, the repair engine would keep thinking the chunk is under-replicated.
            let db = state.db.lock().unwrap();
            let now = chrono::Utc::now().to_rfc3339();
            // Use a placeholder backend_id for remote nodes (the actual backend_id is in their DB)
            db.execute(
                "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
                 VALUES (?1, ?2, ?3, 'synced', ?4)",
                rusqlite::params![chunk_id, "", &target_node_id, &now],
            ).ok();

            tracing::info!("Repair: pushed chunk {} (file {}, idx {}) to {} (verified, tracked locally)",
                chunk_id, file_id, chunk_index, target_node_id);
            return true;
        }
    }

    false
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
