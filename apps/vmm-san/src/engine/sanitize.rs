//! Startup sanitize engine — verifies all local chunk data before the node becomes available.
//!
//! When CoreSAN starts, it enters a "Sanitizing" state. During this phase:
//! 1. Every local chunk replica is verified (file exists, SHA-256 matches)
//! 2. Missing or corrupt chunks are marked as 'error'
//! 3. Chunks are re-fetched from peers if possible
//! 4. Only after sanitize completes does the node become available for reads/writes
//!
//! This prevents serving stale or corrupt data after a crash or disk issue.

use std::sync::Arc;
use sha2::{Sha256, Digest};
use crate::state::CoreSanState;
use crate::storage::chunk;
use crate::peer::client::PeerClient;

/// Run the startup sanitize check. Blocks until complete.
/// Returns (passed, failed, repaired) counts.
pub async fn run_startup_sanitize(state: &Arc<CoreSanState>) -> (u64, u64, u64) {
    tracing::info!("Sanitize: starting startup integrity check...");

    // Phase 0: Clean up orphaned .tmp files from previous crashed writes
    cleanup_orphaned_tmp_files(state);


    // Get all local chunk replicas that should be synced
    let chunks: Vec<(i64, i64, String, u32, String, String, String, Option<String>)> = {
        let db = state.db.read();
        let mut stmt = db.prepare(
            "SELECT fc.id, fc.file_id, COALESCE(fc.sha256, ''), fc.chunk_index,
                    cr.backend_id, b.path, fm.volume_id, fc.dedup_sha256
             FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_map fm ON fm.id = fc.file_id
             WHERE cr.node_id = ?1 AND cr.state = 'synced'"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![&state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                       row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if chunks.is_empty() {
        tracing::info!("Sanitize: no local chunks to verify — clean start");
        return (0, 0, 0);
    }

    tracing::info!("Sanitize: verifying {} local chunk replicas...", chunks.len());

    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut repaired = 0u64;

    for (chunk_id, file_id, expected_sha256, chunk_index, backend_id, backend_path, volume_id, dedup_sha256) in &chunks {
        let path = if let Some(ref dsha) = dedup_sha256 {
            chunk::dedup_chunk_path(backend_path, volume_id, dsha)
        } else {
            chunk::chunk_path(backend_path, volume_id, *file_id, *chunk_index)
        };

        // Check 1: Does the file exist?
        let data = match tokio::fs::read(&path).await {
            Ok(d) => d,
            Err(_) => {
                tracing::warn!("Sanitize: chunk MISSING — {}/{}/idx{} on {}",
                    volume_id, file_id, chunk_index, backend_path);
                mark_error(state, *chunk_id, backend_id);
                failed += 1;
                // Try to repair from peer
                if repair_chunk_from_peer(state, *chunk_id, *file_id, *chunk_index, volume_id, backend_id, backend_path).await {
                    repaired += 1;
                    failed -= 1;
                }
                continue;
            }
        };

        // Check 2: Does the SHA-256 match? (skip if no expected hash stored)
        if !expected_sha256.is_empty() {
            let actual = format!("{:x}", Sha256::digest(&data));
            if actual != *expected_sha256 {
                tracing::warn!("Sanitize: chunk CORRUPT — {}/{}/idx{} expected={} actual={}",
                    volume_id, file_id, chunk_index,
                    &expected_sha256[..8.min(expected_sha256.len())],
                    &actual[..8.min(actual.len())]);
                mark_error(state, *chunk_id, backend_id);
                failed += 1;
                // Try to repair from peer
                if repair_chunk_from_peer(state, *chunk_id, *file_id, *chunk_index, volume_id, backend_id, backend_path).await {
                    repaired += 1;
                    failed -= 1;
                }
                continue;
            }
        }

        passed += 1;
    }

    tracing::info!("Sanitize complete: {} passed, {} failed, {} repaired (of {} total)",
        passed, failed, repaired, chunks.len());

    (passed, failed, repaired)
}

fn mark_error(state: &CoreSanState, chunk_id: i64, backend_id: &str) {
    let db = state.db.write();
    db.execute(
        "UPDATE chunk_replicas SET state = 'error' WHERE chunk_id = ?1 AND backend_id = ?2",
        rusqlite::params![chunk_id, backend_id],
    ).ok();
}

/// Try to repair a single chunk by pulling it from an online peer.
async fn repair_chunk_from_peer(
    state: &CoreSanState,
    chunk_id: i64,
    file_id: i64,
    chunk_index: u32,
    volume_id: &str,
    backend_id: &str,
    backend_path: &str,
) -> bool {
    // Find a peer that might have this chunk
    let online_peers: Vec<(String, String)> = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .map(|p| (p.node_id.clone(), p.address.clone()))
        .collect();

    if online_peers.is_empty() {
        return false;
    }

    let client = PeerClient::new(&state.config.peer.secret);

    for (peer_id, peer_addr) in &online_peers {
        match client.pull_chunk(peer_addr, volume_id, file_id, chunk_index).await {
            Ok(data) => {
                // Write the repaired chunk
                let path = chunk::chunk_path(backend_path, volume_id, file_id, chunk_index);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }

                let tmp = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                if std::fs::write(&tmp, &data).is_ok() {
                    if let Ok(f) = std::fs::File::open(&tmp) {
                        f.sync_all().ok();
                    }
                    if std::fs::rename(&tmp, &path).is_ok() {
                        // Update chunk_replicas back to synced
                        let db = state.db.write();
                        let now = chrono::Utc::now().to_rfc3339();
                        let sha256 = format!("{:x}", Sha256::digest(&data));
                        db.execute(
                            "UPDATE chunk_replicas SET state = 'synced', synced_at = ?1
                             WHERE chunk_id = ?2 AND backend_id = ?3",
                            rusqlite::params![&now, chunk_id, backend_id],
                        ).ok();
                        db.execute(
                            "UPDATE file_chunks SET sha256 = ?1 WHERE id = ?2",
                            rusqlite::params![&sha256, chunk_id],
                        ).ok();
                        tracing::info!("Sanitize: repaired chunk {}/{}/idx{} from peer {}",
                            volume_id, file_id, chunk_index, peer_id);
                        return true;
                    } else {
                        std::fs::remove_file(&tmp).ok();
                    }
                }
            }
            Err(_) => continue,
        }
    }

    tracing::warn!("Sanitize: could NOT repair chunk {}/{}/idx{} — no peer had it",
        volume_id, file_id, chunk_index);
    false
}

/// Clean up orphaned .tmp files left over from crashed writes.
/// Scans all backend .coresan directories for files matching *.tmp.*
fn cleanup_orphaned_tmp_files(state: &CoreSanState) {
    let backend_paths: Vec<String> = {
        let db = state.db.read();
        let mut stmt = db.prepare(
            "SELECT path FROM backends WHERE node_id = ?1"
        ).unwrap();
        stmt.query_map(rusqlite::params![&state.node_id], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    let mut cleaned = 0u64;
    for bp in &backend_paths {
        let coresan_dir = std::path::Path::new(bp).join(".coresan");
        if !coresan_dir.exists() {
            continue;
        }
        if let Ok(entries) = walkdir(coresan_dir) {
            for path in entries {
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if name.contains(".tmp.") {
                    if std::fs::remove_file(&path).is_ok() {
                        cleaned += 1;
                    }
                }
            }
        }
    }

    if cleaned > 0 {
        tracing::info!("Sanitize: cleaned up {} orphaned .tmp files", cleaned);
    }
}

/// Simple recursive directory walker — returns all file paths.
fn walkdir(dir: std::path::PathBuf) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&d) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    files.push(path);
                }
            }
        }
    }
    Ok(files)
}
