//! Integrity engine — periodic checksum verification of chunk replicas.
//!
//! Verifies every chunk on every local backend by reading it and comparing SHA-256.
//! Corrupt chunks are marked as 'error' and the repair engine fixes them.

use std::sync::Arc;
use sha2::{Sha256, Digest};
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::storage::chunk;

/// Spawn the integrity engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    if !state.config.integrity.enabled {
        tracing::info!("Integrity engine disabled");
        return;
    }

    let check_interval = state.config.integrity.interval_secs;
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(check_interval));
        loop {
            tick.tick().await;
            run_integrity_check(&state).await;
        }
    });
}

/// Run one full integrity check cycle — verifies all local chunk replicas.
async fn run_integrity_check(state: &CoreSanState) {
    // Get all local synced chunk replicas with expected checksums
    let chunks: Vec<(i64, i64, String, u32, String, String, Option<String>)> = {
        let db = state.db.read();
        let mut stmt = db.prepare(
            "SELECT fc.id, fc.file_id, fc.sha256, fc.chunk_index, cr.backend_id, b.path, fc.dedup_sha256
             FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_map fm ON fm.id = fc.file_id
             WHERE cr.node_id = ?1 AND cr.state = 'synced' AND fc.sha256 != ''"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![&state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if chunks.is_empty() {
        return;
    }

    tracing::info!("Integrity check: verifying {} local chunk replicas", chunks.len());

    let mut passed = 0u64;
    let mut failed = 0u64;

    for (chunk_id, file_id, expected_sha256, chunk_index, backend_id, backend_path, dedup_sha256) in chunks {
        // Get volume_id for chunk path
        let volume_id: String = {
            let db = state.db.read();
            db.query_row(
                "SELECT volume_id FROM file_map WHERE id = ?1",
                rusqlite::params![file_id], |row| row.get(0),
            ).unwrap_or_default()
        };

        let path = if let Some(ref dsha) = dedup_sha256 {
            chunk::dedup_chunk_path(&backend_path, &volume_id, dsha)
        } else {
            chunk::chunk_path(&backend_path, &volume_id, file_id, chunk_index)
        };

        let actual_sha256 = match tokio::fs::read(&path).await {
            Ok(data) => format!("{:x}", Sha256::digest(&data)),
            Err(_) => {
                mark_chunk_error(state, chunk_id, &backend_id, &expected_sha256, "MISSING");
                failed += 1;
                continue;
            }
        };

        if actual_sha256 == expected_sha256 {
            passed += 1;
        } else {
            failed += 1;
            mark_chunk_error(state, chunk_id, &backend_id, &expected_sha256, &actual_sha256);
            tracing::warn!("Integrity FAIL: chunk {} (expected={}, actual={})",
                chunk_id, &expected_sha256[..8.min(expected_sha256.len())],
                &actual_sha256[..8.min(actual_sha256.len())]);
        }

        // Log result
        {
            let db = state.db.write();
            // Reuse integrity_log for chunk-level too (file_id for backwards compat)
            db.execute(
                "INSERT INTO integrity_log (file_id, backend_id, expected_sha256, actual_sha256, passed)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![file_id, &backend_id, &expected_sha256, &actual_sha256,
                    (actual_sha256 == expected_sha256) as i32],
            ).ok();
        }
    }

    tracing::info!("Integrity check complete: {} passed, {} failed", passed, failed);
}

fn mark_chunk_error(state: &CoreSanState, chunk_id: i64, backend_id: &str, _expected: &str, _actual: &str) {
    let db = state.db.write();
    db.execute(
        "UPDATE chunk_replicas SET state = 'error'
         WHERE chunk_id = ?1 AND backend_id = ?2",
        rusqlite::params![chunk_id, backend_id],
    ).ok();
}
