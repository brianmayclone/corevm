//! Dedup engine — periodic post-process deduplication of chunk data.
//!
//! Scans dedup-enabled volumes for chunks with duplicate SHA256 hashes,
//! consolidates them into a content-addressed store (.dedup/<sha256>),
//! and removes the original positional chunk files.

use std::sync::Arc;
use sha2::{Sha256, Digest};
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::storage::chunk;

/// Spawn the dedup engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    if !state.config.dedup.enabled {
        tracing::info!("Dedup engine disabled");
        return;
    }

    let check_interval = state.config.dedup.interval_secs;
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(check_interval));
        loop {
            tick.tick().await;
            run_dedup_cycle(&state).await;
        }
    });
}

/// Run one dedup cycle across all dedup-enabled volumes.
async fn run_dedup_cycle(state: &CoreSanState) {
    let volumes: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name FROM volumes WHERE dedup = 1 AND status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    if volumes.is_empty() {
        return;
    }

    for (volume_id, volume_name) in &volumes {
        dedup_volume(state, volume_id, volume_name).await;
    }
}

/// Deduplicate a single volume: find duplicate SHA256 hashes and consolidate.
async fn dedup_volume(state: &CoreSanState, volume_id: &str, volume_name: &str) {
    // Find duplicate SHA256 hashes among non-deduplicated chunks.
    // Skip files with active write leases to avoid conflicts.
    let duplicates: Vec<(String, i64)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT fc.sha256, COUNT(*) as cnt
             FROM file_chunks fc
             JOIN file_map fm ON fm.id = fc.file_id
             WHERE fm.volume_id = ?1
               AND fc.dedup_sha256 IS NULL
               AND fc.sha256 != ''
               AND (fm.write_owner = '' OR fm.write_lease_until < datetime('now'))
             GROUP BY fc.sha256
             HAVING cnt > 1
             LIMIT 100"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![volume_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if duplicates.is_empty() {
        return;
    }

    tracing::info!("Dedup: volume '{}' has {} duplicate SHA256 groups to process", volume_name, duplicates.len());

    let mut consolidated = 0u64;
    let mut saved_bytes = 0u64;

    for (sha256, _dup_count) in &duplicates {
        match consolidate_sha256(state, volume_id, sha256).await {
            Ok(saved) => {
                consolidated += 1;
                saved_bytes += saved;
            }
            Err(e) => {
                tracing::warn!("Dedup: failed to consolidate sha256={} in volume {}: {}",
                    &sha256[..16.min(sha256.len())], volume_name, e);
            }
        }
    }

    // Cleanup: remove dedup_store entries with ref_count = 0
    cleanup_orphaned(state, volume_id);

    if consolidated > 0 {
        tracing::info!("Dedup: volume '{}' consolidated {} groups, saved {} bytes",
            volume_name, consolidated, saved_bytes);
    }
}

/// Consolidate all chunks with a given SHA256 into the content-addressed store.
/// Returns bytes saved.
async fn consolidate_sha256(state: &CoreSanState, volume_id: &str, sha256: &str) -> Result<u64, String> {
    // Find all chunks for this SHA256
    let chunks: Vec<(i64, i64, u32, u64)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT fc.id, fc.file_id, fc.chunk_index, fc.size_bytes
             FROM file_chunks fc
             JOIN file_map fm ON fm.id = fc.file_id
             WHERE fm.volume_id = ?1 AND fc.sha256 = ?2 AND fc.dedup_sha256 IS NULL"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![volume_id, sha256],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if chunks.len() < 2 {
        return Ok(0);
    }

    let chunk_size = chunks[0].3;

    // Get all backends that hold replicas of these chunks on this node
    let backend_ids: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let chunk_ids: Vec<i64> = chunks.iter().map(|(id, _, _, _)| *id).collect();
        let n = chunk_ids.len();
        let placeholders: String = (1..=n).map(|i| format!("?{}", i)).collect::<Vec<_>>().join(",");
        let query = format!(
            "SELECT DISTINCT cr.backend_id, b.path
             FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             WHERE cr.chunk_id IN ({}) AND cr.node_id = ?{} AND cr.backend_id != ''",
            placeholders, n + 1
        );
        let mut stmt = db.prepare(&query).unwrap();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = chunk_ids.iter()
            .map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        params.push(Box::new(state.node_id.clone()));
        stmt.query_map(rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    let mut saved = 0u64;

    // For each backend, ensure the .dedup/<sha256> file exists
    for (backend_id, backend_path) in &backend_ids {
        let dedup_path = chunk::dedup_chunk_path(backend_path, volume_id, sha256);

        if !dedup_path.exists() {
            // Find a source chunk on this backend
            let source_path = find_source_chunk(state, volume_id, &chunks, backend_id);
            if let Some(src) = source_path {
                let data = tokio::fs::read(&src).await
                    .map_err(|e| format!("Read source chunk: {}", e))?;

                // Verify SHA256
                let actual_sha = format!("{:x}", Sha256::digest(&data));
                if actual_sha != sha256 {
                    return Err(format!("SHA256 mismatch: expected {}, got {}", sha256, actual_sha));
                }

                if let Some(parent) = dedup_path.parent() {
                    tokio::fs::create_dir_all(parent).await
                        .map_err(|e| format!("Create .dedup dir: {}", e))?;
                }

                // Atomic write: tmp → fsync → rename
                let tmp = dedup_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                tokio::fs::write(&tmp, &data).await
                    .map_err(|e| format!("Write dedup tmp: {}", e))?;
                if let Ok(f) = tokio::fs::File::open(&tmp).await {
                    f.sync_all().await.ok();
                }
                tokio::fs::rename(&tmp, &dedup_path).await
                    .map_err(|e| format!("Rename dedup: {}", e))?;
            }
        }

        // Insert/update dedup_store
        {
            let db = state.db.lock().unwrap();
            let ref_count = chunks.len() as i64;
            db.execute(
                "INSERT INTO dedup_store (sha256, volume_id, backend_id, size_bytes, ref_count)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(sha256, volume_id, backend_id) DO UPDATE SET ref_count = ?5",
                rusqlite::params![sha256, volume_id, backend_id, chunk_size, ref_count],
            ).ok();
        }
    }

    // Update all file_chunks to point to the dedup store
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "UPDATE file_chunks SET dedup_sha256 = ?1
             WHERE sha256 = ?1 AND dedup_sha256 IS NULL
               AND file_id IN (SELECT id FROM file_map WHERE volume_id = ?2)",
            rusqlite::params![sha256, volume_id],
        ).ok();
    }

    // Delete old positional chunk files
    for (_chunk_id, file_id, chunk_index, size) in &chunks {
        for (_backend_id, backend_path) in &backend_ids {
            let old_path = chunk::chunk_path(backend_path, volume_id, *file_id, *chunk_index);
            if old_path.exists() {
                tokio::fs::remove_file(&old_path).await.ok();
                saved += size;
            }
        }
    }

    // We keep one copy in .dedup per backend, so subtract that from savings
    saved = saved.saturating_sub(chunk_size * backend_ids.len() as u64);

    Ok(saved)
}

/// Find a source chunk file on a specific backend.
fn find_source_chunk(
    state: &CoreSanState,
    volume_id: &str,
    chunks: &[(i64, i64, u32, u64)],
    backend_id: &str,
) -> Option<std::path::PathBuf> {
    let db = state.db.lock().unwrap();
    for (chunk_id, file_id, chunk_index, _) in chunks {
        let has_replica: bool = db.query_row(
            "SELECT COUNT(*) FROM chunk_replicas WHERE chunk_id = ?1 AND backend_id = ?2 AND state = 'synced'",
            rusqlite::params![chunk_id, backend_id],
            |row| row.get::<_, i64>(0),
        ).map(|c| c > 0).unwrap_or(false);

        if has_replica {
            let backend_path: String = db.query_row(
                "SELECT path FROM backends WHERE id = ?1",
                rusqlite::params![backend_id],
                |row| row.get(0),
            ).unwrap_or_default();

            let path = chunk::chunk_path(&backend_path, volume_id, *file_id, *chunk_index);
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

/// Remove dedup_store entries with ref_count = 0 and delete orphaned .dedup files.
fn cleanup_orphaned(state: &CoreSanState, volume_id: &str) {
    let orphans: Vec<(String, String, String)> = {
        let db = state.db.lock().unwrap();

        // Recount refs
        db.execute(
            "UPDATE dedup_store SET ref_count = (
                SELECT COUNT(*) FROM file_chunks fc
                JOIN file_map fm ON fm.id = fc.file_id
                WHERE fc.dedup_sha256 = dedup_store.sha256
                  AND fm.volume_id = dedup_store.volume_id
            ) WHERE volume_id = ?1",
            rusqlite::params![volume_id],
        ).ok();

        let mut stmt = db.prepare(
            "SELECT sha256, backend_id, (SELECT path FROM backends WHERE id = dedup_store.backend_id)
             FROM dedup_store WHERE volume_id = ?1 AND ref_count = 0"
        ).unwrap();
        stmt.query_map(rusqlite::params![volume_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2).unwrap_or_default())),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    for (sha256, _backend_id, backend_path) in &orphans {
        if !backend_path.is_empty() {
            let path = chunk::dedup_chunk_path(backend_path, volume_id, sha256);
            std::fs::remove_file(&path).ok();
        }
    }

    if !orphans.is_empty() {
        let db = state.db.lock().unwrap();
        db.execute(
            "DELETE FROM dedup_store WHERE volume_id = ?1 AND ref_count = 0",
            rusqlite::params![volume_id],
        ).ok();
        tracing::info!("Dedup cleanup: removed {} orphaned dedup entries for volume {}", orphans.len(), volume_id);
    }
}
