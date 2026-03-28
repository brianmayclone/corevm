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
    let db = state.db.lock().unwrap();

    // Find chunk_replicas on offline/degraded/draining backends that need relocation
    let chunks_to_move: Vec<(i64, i64, u32, String, String, String)> = {
        let mut stmt = db.prepare(
            "SELECT cr.chunk_id, fc.file_id, fc.chunk_index, cr.backend_id, b.path, fm.volume_id
             FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN file_map fm ON fm.id = fc.file_id
             JOIN backends b ON b.id = cr.backend_id
             WHERE cr.node_id = ?1 AND b.status IN ('offline', 'degraded', 'draining')
               AND cr.state = 'synced'
             LIMIT ?2"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![&state.node_id, MAX_CHUNKS_PER_CYCLE],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if chunks_to_move.is_empty() {
        return;
    }

    tracing::info!("Rebalancer: {} chunks to evacuate from degraded/draining backends", chunks_to_move.len());

    let mut moved = 0u32;
    let mut failed = 0u32;

    for (chunk_id, file_id, chunk_index, old_backend_id, old_backend_path, volume_id) in &chunks_to_move {
        // Find a healthy target backend on this node (backends are node-wide, not volume-specific)
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

        // Read source chunk
        let src = chunk::chunk_path(old_backend_path, volume_id, *file_id, *chunk_index);
        let src_data = match std::fs::read(&src) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Rebalancer: cannot read source chunk {}: {}", src.display(), e);
                // Mark source as error since it's unreadable
                db.execute(
                    "UPDATE chunk_replicas SET state = 'error' WHERE chunk_id = ?1 AND backend_id = ?2",
                    rusqlite::params![chunk_id, old_backend_id],
                ).ok();
                failed += 1;
                continue;
            }
        };

        let src_sha = format!("{:x}", Sha256::digest(&src_data));

        // Atomic write to target: tmp → fsync → rename
        let dst = chunk::chunk_path(&target_path, volume_id, *file_id, *chunk_index);
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

        // Copy verified — update DB atomically
        let now = chrono::Utc::now().to_rfc3339();
        db.execute("BEGIN IMMEDIATE", []).ok();

        db.execute(
            "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
             VALUES (?1, ?2, ?3, 'synced', ?4)",
            rusqlite::params![chunk_id, &target_id, &state.node_id, &now],
        ).ok();

        db.execute(
            "DELETE FROM chunk_replicas WHERE chunk_id = ?1 AND backend_id = ?2",
            rusqlite::params![chunk_id, old_backend_id],
        ).ok();

        db.execute("COMMIT", []).ok();

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
        db.execute(
            "UPDATE backends SET status = 'offline' WHERE id = ?1",
            rusqlite::params![&backend_id],
        ).ok();
    }
}
