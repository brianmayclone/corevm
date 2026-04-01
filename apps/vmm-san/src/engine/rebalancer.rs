//! Local rebalancer — redistributes chunks across local disks when backends change.
//!
//! Handles:
//! 1. Backend goes offline/degraded → evacuate chunks to other local backends
//! 2. Backend draining → move all chunks off before removal
//!
//! Safety guarantees:
//! - Only runs when quorum is Active or Solo (not during Fenced/Sanitizing)
//! - Verifies SHA256 after copy before removing the old replica
//! - Uses atomic write (tmp + fsync + rename) for the destination

use std::sync::Arc;
use sha2::{Sha256, Digest};
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::storage::chunk;

const REBALANCE_INTERVAL_SECS: u64 = 30;
const MAX_CHUNKS_PER_CYCLE: u32 = 50;

/// Spawn the rebalancer as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(REBALANCE_INTERVAL_SECS));
        loop {
            tick.tick().await;

            // Only rebalance when quorum is healthy — prevent split-brain rebalancing
            let quorum = *state.quorum_status.read().unwrap();
            match quorum {
                crate::state::QuorumStatus::Active | crate::state::QuorumStatus::Solo => {}
                _ => {
                    tracing::trace!("Rebalancer: skipping (quorum={:?})", quorum);
                    continue;
                }
            }

            run_rebalance(&state);
        }
    });
}

fn run_rebalance(state: &CoreSanState) {
    use crate::services::chunk::ChunkService;

    let db = state.db.write();

    // Phase 1: Repair missing local mirror copies
    repair_local_mirrors(&db, state);

    // Phase 2: Evacuate chunks from degraded/draining backends
    let chunks_to_move = ChunkService::find_chunks_on_bad_backends(&db, &state.node_id, MAX_CHUNKS_PER_CYCLE);

    if chunks_to_move.is_empty() {
        return;
    }

    tracing::info!("Rebalancer: {} chunks to evacuate from degraded/draining backends", chunks_to_move.len());

    let mut moved = 0u32;
    let mut failed = 0u32;

    for ctm in &chunks_to_move {
        let (chunk_id, file_id, chunk_index) = (ctm.chunk_id, ctm.file_id, ctm.chunk_index);
        let (old_backend_id, old_backend_path, volume_id) = (&ctm.backend_id, &ctm.backend_path, &ctm.volume_id);

        // Find a healthy target backend on this node
        let target = db.query_row(
            "SELECT id, path FROM backends
             WHERE node_id = ?1 AND status = 'online' AND id != ?2
             ORDER BY free_bytes DESC LIMIT 1",
            rusqlite::params![&state.node_id, old_backend_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        );

        let (target_id, target_path) = match target {
            Ok(t) => t,
            Err(_) => {
                tracing::warn!("Rebalancer: no healthy target backend for chunk {}", chunk_id);
                failed += 1;
                continue;
            }
        };

        // Check if this chunk is deduplicated
        let dedup_sha256: Option<String> = db.query_row(
            "SELECT dedup_sha256 FROM file_chunks WHERE id = ?1",
            rusqlite::params![chunk_id], |row| row.get(0),
        ).ok().flatten();

        // Read source chunk (resolve dedup path if applicable)
        let src = if let Some(ref dsha) = dedup_sha256 {
            chunk::dedup_chunk_path(old_backend_path, volume_id, dsha)
        } else {
            chunk::chunk_path(old_backend_path, volume_id, file_id, chunk_index)
        };
        let src_data = match std::fs::read(&src) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Rebalancer: cannot read source chunk {}: {}", src.display(), e);
                // Mark source as error since it's unreadable
                log_err!(ChunkService::mark_replica_error(&db, chunk_id, &state.node_id),
                    "rebalancer: mark unreadable chunk");
                failed += 1;
                continue;
            }
        };

        let src_sha = format!("{:x}", Sha256::digest(&src_data));

        // Atomic write to target: tmp → fsync → rename
        let dst = if let Some(ref dsha) = dedup_sha256 {
            chunk::dedup_chunk_path(&target_path, volume_id, dsha)
        } else {
            chunk::chunk_path(&target_path, volume_id, file_id, chunk_index)
        };
        if let Some(parent) = dst.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                failed += 1;
                continue;
            }
        }

        let tmp = dst.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
        if std::fs::write(&tmp, &src_data).is_err() {
            std::fs::remove_file(&tmp).ok();
            failed += 1;
            continue;
        }
        if let Ok(f) = std::fs::File::open(&tmp) {
            f.sync_all().ok();
        }
        if std::fs::rename(&tmp, &dst).is_err() {
            std::fs::remove_file(&tmp).ok();
            failed += 1;
            continue;
        }

        // Verify destination: read back and compare SHA256
        let dst_data = match std::fs::read(&dst) {
            Ok(d) => d,
            Err(_) => {
                std::fs::remove_file(&dst).ok();
                failed += 1;
                continue;
            }
        };
        let dst_sha = format!("{:x}", Sha256::digest(&dst_data));

        if src_sha != dst_sha {
            tracing::error!("Rebalancer: SHA256 MISMATCH after copy! chunk {} src={} dst={}",
                chunk_id, &src_sha[..8], &dst_sha[..8]);
            std::fs::remove_file(&dst).ok();
            failed += 1;
            continue;
        }

        // Copy verified — update DB atomically via ChunkService
        if let Err(e) = ChunkService::move_replica(&db, chunk_id, old_backend_id, &target_id, &state.node_id) {
            tracing::error!("Rebalancer: DB move_replica failed for chunk {}: {}", chunk_id, e);
            failed += 1;
            // Remove the destination copy since DB wasn't updated
            let _ = std::fs::remove_file(&dst);
            continue;
        }

        // Update dedup_store if this was a deduplicated chunk
        if let Some(ref dsha) = dedup_sha256 {
            db.execute(
                "INSERT INTO dedup_store (sha256, volume_id, backend_id, size_bytes, ref_count)
                 SELECT ?1, ?2, ?3, size_bytes, ref_count FROM dedup_store
                 WHERE sha256 = ?1 AND volume_id = ?2 AND backend_id = ?4
                 ON CONFLICT(sha256, volume_id, backend_id) DO UPDATE SET ref_count = excluded.ref_count",
                rusqlite::params![dsha, volume_id, &target_id, old_backend_id],
            ).ok();
            db.execute(
                "DELETE FROM dedup_store WHERE sha256 = ?1 AND volume_id = ?2 AND backend_id = ?3",
                rusqlite::params![dsha, volume_id, old_backend_id],
            ).ok();
        }

        // Only NOW remove the old file — DB already points to the new location
        std::fs::remove_file(&src).ok();

        moved += 1;
        tracing::debug!("Rebalancer: moved chunk {} (verified SHA256={}) {} → {}",
            chunk_id, &src_sha[..8], old_backend_id, target_id);
    }

    if moved > 0 || failed > 0 {
        tracing::info!("Rebalancer: {} moved, {} failed", moved, failed);
    }

    // Check if any draining backends are now empty
    let drained: Vec<String> = {
        let mut stmt = db.prepare(
            "SELECT b.id FROM backends b
             WHERE b.node_id = ?1 AND b.status = 'draining'
               AND NOT EXISTS (SELECT 1 FROM chunk_replicas cr WHERE cr.backend_id = b.id)"
        ).unwrap();
        stmt.query_map(rusqlite::params![&state.node_id], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    for backend_id in drained {
        tracing::info!("Rebalancer: backend {} fully drained", backend_id);
        log_err!(db.execute(
            "UPDATE backends SET status = 'offline' WHERE id = ?1",
            rusqlite::params![&backend_id],
        ), "rebalancer: mark drained backend offline");
    }
}

/// Repair missing local mirror copies.
/// For volumes with local_raid=mirror, every chunk should exist on ALL local online backends.
/// If a chunk only exists on some backends, copy it to the missing ones.
fn repair_local_mirrors(db: &rusqlite::Connection, state: &CoreSanState) {
    use crate::services::chunk::ChunkService;

    // Find volumes with mirror or stripe_mirror that have local backends
    let mirror_volumes: Vec<(String, String)> = {
        let mut stmt = db.prepare(
            "SELECT id, local_raid FROM volumes WHERE local_raid IN ('mirror', 'stripe_mirror') AND status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    if mirror_volumes.is_empty() {
        return;
    }

    // Get all local online backends
    let local_backends: Vec<(String, String)> = {
        let mut stmt = db.prepare(
            "SELECT id, path FROM backends WHERE node_id = ?1 AND status = 'online'"
        ).unwrap();
        stmt.query_map(rusqlite::params![&state.node_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    if local_backends.len() < 2 {
        return; // Mirror needs at least 2 backends
    }

    let mut repaired = 0u32;

    for (volume_id, _raid) in &mirror_volumes {
        // Find chunks that exist on some local backends but not all
        // For each chunk on this node, check if it's on every local backend
        let local_chunks: Vec<(i64, i64, u32)> = {
            let mut stmt = db.prepare(
                "SELECT DISTINCT fc.id, fc.file_id, fc.chunk_index
                 FROM chunk_replicas cr
                 JOIN file_chunks fc ON fc.id = cr.chunk_id
                 JOIN file_map fm ON fm.id = fc.file_id
                 WHERE cr.node_id = ?1 AND fm.volume_id = ?2 AND cr.state = 'synced'
                   AND cr.backend_id != ''"
            ).unwrap();
            stmt.query_map(rusqlite::params![&state.node_id, volume_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            }).unwrap().filter_map(|r| r.ok()).collect()
        };

        for (chunk_id, file_id, chunk_index) in &local_chunks {
            if repaired >= MAX_CHUNKS_PER_CYCLE {
                return;
            }

            // Which backends have this chunk?
            let backends_with_chunk: Vec<String> = {
                let mut stmt = db.prepare(
                    "SELECT backend_id FROM chunk_replicas
                     WHERE chunk_id = ?1 AND node_id = ?2 AND state = 'synced' AND backend_id != ''"
                ).unwrap();
                stmt.query_map(rusqlite::params![chunk_id, &state.node_id], |row| row.get(0))
                    .unwrap().filter_map(|r| r.ok()).collect()
            };

            // Find backends that are missing this chunk
            for (backend_id, backend_path) in &local_backends {
                if backends_with_chunk.contains(backend_id) {
                    continue; // Already has it
                }

                // Find a source backend that has this chunk
                let source = backends_with_chunk.first();
                let source_path = source.and_then(|sid| {
                    local_backends.iter().find(|(id, _)| id == sid).map(|(_, p)| p.as_str())
                });

                let source_path = match source_path {
                    Some(p) => p,
                    None => continue,
                };

                // Check dedup status for path resolution
                let dedup_sha: Option<String> = db.query_row(
                    "SELECT dedup_sha256 FROM file_chunks WHERE id = ?1",
                    rusqlite::params![chunk_id], |row| row.get(0),
                ).ok().flatten();

                // Copy chunk from source to missing backend
                let src = if let Some(ref dsha) = dedup_sha {
                    chunk::dedup_chunk_path(source_path, volume_id, dsha)
                } else {
                    chunk::chunk_path(source_path, volume_id, *file_id, *chunk_index)
                };
                let dst = if let Some(ref dsha) = dedup_sha {
                    chunk::dedup_chunk_path(backend_path, volume_id, dsha)
                } else {
                    chunk::chunk_path(backend_path, volume_id, *file_id, *chunk_index)
                };

                if !src.exists() {
                    continue;
                }

                if let Some(parent) = dst.parent() {
                    if std::fs::create_dir_all(parent).is_err() { continue; }
                }

                // Atomic copy: read → tmp → fsync → rename
                let data = match std::fs::read(&src) {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                let tmp = dst.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                if std::fs::write(&tmp, &data).is_err() { continue; }
                if let Ok(f) = std::fs::File::open(&tmp) {
                    let _ = f.sync_all();
                }
                if std::fs::rename(&tmp, &dst).is_err() {
                    let _ = std::fs::remove_file(&tmp);
                    continue;
                }

                // Register in DB
                log_err!(ChunkService::set_replica_synced(db, *chunk_id, backend_id, &state.node_id),
                    "rebalancer: mirror repair set_replica_synced");

                repaired += 1;
                tracing::info!("Mirror repair: copied chunk {}/idx{} → backend {}",
                    file_id, chunk_index, &backend_id[..8]);
            }
        }
    }

    if repaired > 0 {
        tracing::info!("Mirror repair: {} missing local copies restored", repaired);
    }
}
