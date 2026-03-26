//! Local rebalancer — redistributes chunks across local disks when backends change.
//!
//! Handles:
//! 1. Backend goes offline/degraded → evacuate chunks to other local backends
//! 2. Backend draining → move all chunks off before removal
//! 3. Manual rebalance trigger → even out usage across backends

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::storage::chunk;

const REBALANCE_INTERVAL_SECS: u64 = 30;

/// Spawn the rebalancer as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(REBALANCE_INTERVAL_SECS));
        loop {
            tick.tick().await;
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
             LIMIT 100"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![&state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if chunks_to_move.is_empty() {
        return;
    }

    tracing::info!("Rebalancer: {} chunks to evacuate from degraded/draining backends", chunks_to_move.len());

    for (chunk_id, file_id, chunk_index, old_backend_id, old_backend_path, volume_id) in chunks_to_move {
        // Find a healthy target backend on this node
        let target = db.query_row(
            "SELECT id, path FROM backends
             WHERE volume_id = ?1 AND node_id = ?2 AND status = 'online' AND id != ?3
             ORDER BY free_bytes DESC LIMIT 1",
            rusqlite::params![&volume_id, &state.node_id, &old_backend_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        );

        let (target_id, target_path) = match target {
            Ok(t) => t,
            Err(_) => {
                tracing::warn!("Rebalancer: no target backend for chunk {} in volume {}", chunk_id, volume_id);
                continue;
            }
        };

        // Copy chunk from old to new backend
        let src = chunk::chunk_path(&old_backend_path, &volume_id, file_id, chunk_index);
        let dst = chunk::chunk_path(&target_path, &volume_id, file_id, chunk_index);

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        match std::fs::copy(&src, &dst) {
            Ok(bytes) => {
                // Update chunk_replicas to point to new backend
                let now = chrono::Utc::now().to_rfc3339();
                db.execute(
                    "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
                     VALUES (?1, ?2, ?3, 'synced', ?4)",
                    rusqlite::params![chunk_id, &target_id, &state.node_id, &now],
                ).ok();

                // Remove old replica
                db.execute(
                    "DELETE FROM chunk_replicas WHERE chunk_id = ?1 AND backend_id = ?2",
                    rusqlite::params![chunk_id, &old_backend_id],
                ).ok();

                // Remove old chunk file
                std::fs::remove_file(&src).ok();

                tracing::debug!("Rebalancer: moved chunk {} ({} bytes) {} → {}",
                    chunk_id, bytes, old_backend_id, target_id);
            }
            Err(e) => {
                tracing::warn!("Rebalancer: failed to copy chunk {}: {}", chunk_id, e);
            }
        }
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
