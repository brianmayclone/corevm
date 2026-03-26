//! Local rebalancer — redistributes files across local disks when backends change.
//!
//! Handles three scenarios:
//! 1. Backend goes offline/degraded → evacuate files to other local backends
//! 2. New backend added → optionally redistribute files for even usage
//! 3. Backend draining → move all files off before removal

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;

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

    // Find files on offline/degraded/draining backends that need to be moved
    let files_to_move: Vec<(i64, String, String, String, String)> = {
        let mut stmt = db.prepare(
            "SELECT fr.file_id, fm.volume_id, fm.rel_path, b.id, b.path
             FROM file_replicas fr
             JOIN file_map fm ON fm.id = fr.file_id
             JOIN backends b ON b.id = fr.backend_id
             WHERE b.node_id = ?1 AND b.status IN ('offline', 'degraded', 'draining')
               AND fr.state = 'synced'"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![&state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if files_to_move.is_empty() {
        return;
    }

    tracing::info!("Rebalancer: {} files to evacuate from degraded/draining backends", files_to_move.len());

    for (file_id, volume_id, rel_path, old_backend_id, old_backend_path) in files_to_move {
        // Find a healthy target backend on this node
        let target = db.query_row(
            "SELECT id, path FROM backends
             WHERE volume_id = ?1 AND node_id = ?2 AND status = 'online'
               AND id != ?3
             ORDER BY free_bytes DESC LIMIT 1",
            rusqlite::params![&volume_id, &state.node_id, &old_backend_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        );

        let (target_id, target_path) = match target {
            Ok(t) => t,
            Err(_) => {
                tracing::warn!("Rebalancer: no target backend for {}/{}", volume_id, rel_path);
                continue;
            }
        };

        // Copy file from old to new backend
        let src = std::path::Path::new(&old_backend_path).join(&rel_path);
        let dst = std::path::Path::new(&target_path).join(&rel_path);

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        match std::fs::copy(&src, &dst) {
            Ok(bytes) => {
                // Update file_replicas to point to new backend
                let now = chrono::Utc::now().to_rfc3339();
                db.execute(
                    "INSERT OR REPLACE INTO file_replicas (file_id, backend_id, state, replica_version, synced_at)
                     VALUES (?1, ?2, 'synced', (SELECT version FROM file_map WHERE id = ?1), ?3)",
                    rusqlite::params![file_id, &target_id, &now],
                ).ok();

                // Remove old replica entry
                db.execute(
                    "DELETE FROM file_replicas WHERE file_id = ?1 AND backend_id = ?2",
                    rusqlite::params![file_id, &old_backend_id],
                ).ok();

                // Remove old file (best effort — disk might be degraded)
                std::fs::remove_file(&src).ok();

                tracing::debug!("Rebalancer: moved {}/{} ({} bytes) {} → {}",
                    volume_id, rel_path, bytes, old_backend_id, target_id);
            }
            Err(e) => {
                tracing::warn!("Rebalancer: failed to copy {}/{}: {}", volume_id, rel_path, e);
            }
        }
    }

    // Check if any draining backends are now empty and can be removed
    let drained: Vec<String> = {
        let mut stmt = db.prepare(
            "SELECT b.id FROM backends b
             WHERE b.node_id = ?1 AND b.status = 'draining'
               AND NOT EXISTS (SELECT 1 FROM file_replicas fr WHERE fr.backend_id = b.id)"
        ).unwrap();
        stmt.query_map(rusqlite::params![&state.node_id], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    for backend_id in drained {
        tracing::info!("Rebalancer: backend {} fully drained, marking offline", backend_id);
        db.execute(
            "UPDATE backends SET status = 'offline' WHERE id = ?1",
            rusqlite::params![&backend_id],
        ).ok();
    }
}
