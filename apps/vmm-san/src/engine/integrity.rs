//! Integrity engine — periodic checksum verification of all file replicas.
//!
//! Detects silent corruption by reading every file and comparing SHA-256.
//! Corrupt replicas are marked as 'error' and the repair engine fixes them.

use std::sync::Arc;
use sha2::{Sha256, Digest};
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;

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

/// Run one full integrity check cycle.
async fn run_integrity_check(state: &CoreSanState) {
    let replicas: Vec<(i64, String, String, String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT fr.file_id, fr.backend_id, fm.sha256, b.path, fm.rel_path
             FROM file_replicas fr
             JOIN file_map fm ON fm.id = fr.file_id
             JOIN backends b ON b.id = fr.backend_id
             WHERE b.node_id = ?1 AND fr.state = 'synced' AND fm.sha256 != ''"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![&state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if replicas.is_empty() {
        return;
    }

    tracing::info!("Integrity check: verifying {} local replicas", replicas.len());

    let mut passed = 0u64;
    let mut failed = 0u64;

    for (file_id, backend_id, expected_sha256, backend_path, rel_path) in replicas {
        let full_path = std::path::Path::new(&backend_path).join(&rel_path);

        let actual_sha256 = match tokio::fs::read(&full_path).await {
            Ok(data) => format!("{:x}", Sha256::digest(&data)),
            Err(_) => {
                // File missing — mark as error
                mark_integrity_result(state, file_id, &backend_id, &expected_sha256, "MISSING", false);
                failed += 1;
                continue;
            }
        };

        let ok = actual_sha256 == expected_sha256;
        mark_integrity_result(state, file_id, &backend_id, &expected_sha256, &actual_sha256, ok);

        if ok {
            passed += 1;
        } else {
            failed += 1;
            // Mark replica as error — repair engine will fix it
            let db = state.db.lock().unwrap();
            db.execute(
                "UPDATE file_replicas SET state = 'error'
                 WHERE file_id = ?1 AND backend_id = ?2",
                rusqlite::params![file_id, &backend_id],
            ).ok();
            tracing::warn!("Integrity FAIL: {}/{} (expected={}, actual={})",
                backend_path, rel_path, &expected_sha256[..8], &actual_sha256[..8]);
        }
    }

    tracing::info!("Integrity check complete: {} passed, {} failed", passed, failed);
}

fn mark_integrity_result(
    state: &CoreSanState,
    file_id: i64,
    backend_id: &str,
    expected: &str,
    actual: &str,
    passed: bool,
) {
    let db = state.db.lock().unwrap();
    db.execute(
        "INSERT INTO integrity_log (file_id, backend_id, expected_sha256, actual_sha256, passed)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![file_id, backend_id, expected, actual, passed as i32],
    ).ok();
}
