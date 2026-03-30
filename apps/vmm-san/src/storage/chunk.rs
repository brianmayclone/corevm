//! Chunk manager — splits files into fixed-size chunks distributed across backends.
//!
//! This is the core of CoreSAN's software RAID. Files are split into 64MB chunks,
//! and each chunk is placed on a different local disk (stripe) or mirrored across
//! multiple local disks. Cross-node replication happens at the chunk level via FTT.

use std::io::{Read, Seek, SeekFrom, Write as IoWrite};
use std::path::{Path, PathBuf};
use rusqlite::Connection;
use sha2::{Sha256, Digest};

pub const DEFAULT_CHUNK_SIZE: u64 = 4 * 1024 * 1024; // 4 MB

/// Describes which part of a chunk is affected by a read/write operation.
#[derive(Debug, Clone)]
pub struct ChunkRange {
    pub chunk_index: u32,
    pub local_offset: u64,  // offset within the chunk
    pub size: u64,          // bytes affected in this chunk
}

/// Calculate which chunks are affected by a read/write at [offset..offset+size].
pub fn affected_chunks(offset: u64, size: u64, chunk_size: u64) -> Vec<ChunkRange> {
    if size == 0 || chunk_size == 0 {
        return Vec::new();
    }
    let first_chunk = (offset / chunk_size) as u32;
    let last_byte = offset + size - 1;
    let last_chunk = (last_byte / chunk_size) as u32;

    let mut ranges = Vec::new();
    for ci in first_chunk..=last_chunk {
        let chunk_start = ci as u64 * chunk_size;
        let range_start = offset.max(chunk_start);
        let range_end = (offset + size).min(chunk_start + chunk_size);
        ranges.push(ChunkRange {
            chunk_index: ci,
            local_offset: range_start - chunk_start,
            size: range_end - range_start,
        });
    }
    ranges
}

/// Build the filesystem path for a chunk file.
pub fn chunk_path(backend_path: &str, volume_id: &str, file_id: i64, chunk_index: u32) -> PathBuf {
    Path::new(backend_path)
        .join(".coresan")
        .join(volume_id)
        .join(file_id.to_string())
        .join(format!("chunk_{:06}", chunk_index))
}

/// Select backend(s) for a new chunk based on local RAID policy.
/// Returns Vec of (backend_id, backend_path) — 1 for stripe, N for mirror.
pub fn place_chunk(
    db: &Connection,
    _volume_id: &str,
    node_id: &str,
    chunk_index: u32,
    local_raid: &str,
) -> Vec<(String, String)> {
    // Backends are a node-wide pool shared by all volumes
    let backends: Vec<(String, String)> = {
        let mut stmt = db.prepare(
            "SELECT id, path FROM backends
             WHERE node_id = ?1 AND status = 'online'
             ORDER BY free_bytes DESC"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![node_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if backends.is_empty() {
        return Vec::new();
    }

    match local_raid {
        "stripe" => {
            // Round-robin: chunk_index % num_backends
            let idx = (chunk_index as usize) % backends.len();
            vec![backends[idx].clone()]
        }
        "mirror" => {
            // All local backends get a copy
            backends
        }
        "stripe_mirror" => {
            // Stripe across pairs: chunk on 2 backends
            if backends.len() < 2 {
                return backends;
            }
            let idx = (chunk_index as usize * 2) % backends.len();
            let idx2 = (idx + 1) % backends.len();
            vec![backends[idx].clone(), backends[idx2].clone()]
        }
        _ => {
            vec![backends[0].clone()]
        }
    }
}

/// Ensure chunk entries exist in the database for a file of given size.
/// Creates file_chunks rows and chunk_replicas for local placement.
pub fn ensure_chunks(
    db: &Connection,
    file_id: i64,
    file_size: u64,
    chunk_size: u64,
    volume_id: &str,
    node_id: &str,
    local_raid: &str,
) {
    let chunk_count = if file_size == 0 { 0 } else { ((file_size - 1) / chunk_size + 1) as u32 };

    let existing_count: u32 = db.query_row(
        "SELECT COUNT(*) FROM file_chunks WHERE file_id = ?1",
        rusqlite::params![file_id], |row| row.get(0),
    ).unwrap_or(0);

    // Create missing chunk entries
    for ci in existing_count..chunk_count {
        let offset = ci as u64 * chunk_size;
        let size = chunk_size.min(file_size.saturating_sub(offset));

        db.execute(
            "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![file_id, ci, offset, size],
        ).ok();

        // Get chunk_id
        if let Ok(chunk_id) = db.query_row(
            "SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2",
            rusqlite::params![file_id, ci], |row| row.get::<_, i64>(0),
        ) {
            // Place chunk on local backends
            let placements = place_chunk(db, volume_id, node_id, ci, local_raid);
            for (backend_id, _) in &placements {
                db.execute(
                    "INSERT OR IGNORE INTO chunk_replicas (chunk_id, backend_id, node_id, state)
                     VALUES (?1, ?2, ?3, 'syncing')",
                    rusqlite::params![chunk_id, backend_id, node_id],
                ).ok();
            }
        }
    }

    // Update file_map chunk_count
    db.execute(
        "UPDATE file_map SET chunk_count = ?1 WHERE id = ?2",
        rusqlite::params![chunk_count, file_id],
    ).ok();
}

/// Read bytes from a file by assembling data from multiple chunk backends.
pub fn read_chunk_data(
    db: &Connection,
    file_id: i64,
    offset: u64,
    size: u64,
    volume_id: &str,
    node_id: &str,
    chunk_size: u64,
) -> Result<Vec<u8>, String> {
    let ranges = affected_chunks(offset, size, chunk_size);
    let mut result = Vec::with_capacity(size as usize);

    for range in ranges {
        // Find ALL local replicas for this chunk with expected SHA256
        let replicas: Vec<(String, String, String)> = {
            let mut stmt = db.prepare(
                "SELECT cr.backend_id, b.path, COALESCE(fc.sha256, '') FROM chunk_replicas cr
                 JOIN backends b ON b.id = cr.backend_id
                 JOIN file_chunks fc ON fc.id = cr.chunk_id
                 WHERE fc.file_id = ?1 AND fc.chunk_index = ?2
                   AND cr.node_id = ?3 AND cr.state = 'synced'"
            ).unwrap();
            stmt.query_map(
                rusqlite::params![file_id, range.chunk_index, node_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).unwrap().filter_map(|r| r.ok()).collect()
        };

        let mut read_ok = false;

        // Try each local replica until one succeeds with valid data
        for (backend_id, bp, expected_sha) in &replicas {
            let path = chunk_path(bp, volume_id, file_id, range.chunk_index);

            // For full-chunk reads (offset=0, size=full), verify SHA256 before serving.
            // For partial reads within a chunk, skip verification (too expensive for FUSE seeks).
            let is_full_chunk_read = range.local_offset == 0;

            match std::fs::File::open(&path) {
                Ok(mut file) => {
                    // If we have an expected SHA and this is a full-chunk read, verify first
                    if is_full_chunk_read && !expected_sha.is_empty() {
                        // Read the whole chunk for verification
                        let full_data = match std::fs::read(&path) {
                            Ok(d) => d,
                            Err(_) => { continue; }
                        };
                        let actual_sha = format!("{:x}", Sha256::digest(&full_data));
                        if actual_sha != *expected_sha {
                            tracing::warn!("Chunk SHA256 MISMATCH on read: {}/{}/idx{} backend={} — trying next replica",
                                volume_id, file_id, range.chunk_index, backend_id);
                            db.execute(
                                "UPDATE chunk_replicas SET state = 'error'
                                 WHERE chunk_id = (SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2)
                                   AND backend_id = ?3",
                                rusqlite::params![file_id, range.chunk_index, backend_id],
                            ).ok();
                            continue; // Try next replica — this one is corrupt
                        }
                        // SHA verified — extract the requested range
                        let end = (range.local_offset as usize + range.size as usize).min(full_data.len());
                        let start = range.local_offset as usize;
                        if start < full_data.len() {
                            result.extend_from_slice(&full_data[start..end]);
                            if (end - start) < range.size as usize {
                                result.extend(std::iter::repeat(0u8).take(range.size as usize - (end - start)));
                            }
                        } else {
                            result.extend(std::iter::repeat(0u8).take(range.size as usize));
                        }
                        read_ok = true;
                        break;
                    }

                    // Partial read or no SHA available — read directly
                    if file.seek(SeekFrom::Start(range.local_offset)).is_err() {
                        continue;
                    }
                    let mut buf = vec![0u8; range.size as usize];
                    match file.read(&mut buf) {
                        Ok(n) => {
                            result.extend_from_slice(&buf[..n]);
                            if (n as u64) < range.size {
                                result.extend(std::iter::repeat(0u8).take((range.size as usize) - n));
                            }
                            read_ok = true;
                            break;
                        }
                        Err(e) => {
                            tracing::warn!("Chunk read error on backend {}: {} — trying fallback", backend_id, e);
                            db.execute(
                                "UPDATE chunk_replicas SET state = 'error'
                                 WHERE chunk_id = (SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2)
                                   AND backend_id = ?3",
                                rusqlite::params![file_id, range.chunk_index, backend_id],
                            ).ok();
                            continue;
                        }
                    }
                }
                Err(_) => {
                    db.execute(
                        "UPDATE chunk_replicas SET state = 'error'
                         WHERE chunk_id = (SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2)
                           AND backend_id = ?3",
                        rusqlite::params![file_id, range.chunk_index, backend_id],
                    ).ok();
                    continue;
                }
            }
        }

        if !read_ok {
            // No local replica could serve this chunk — fill with zeros
            // The repair engine will fetch the chunk from a peer later.
            // TODO: Could do a synchronous peer fetch here for immediate recovery.
            tracing::warn!("Chunk {}/{} idx {} unreadable on all local replicas",
                volume_id, file_id, range.chunk_index);
            result.extend(std::iter::repeat(0u8).take(range.size as usize));
        }
    }

    Ok(result)
}

/// Write bytes to a file by distributing across chunk backends.
/// Returns list of (chunk_index, sha256) for changed chunks.
pub fn write_chunk_data(
    db: &Connection,
    file_id: i64,
    offset: u64,
    data: &[u8],
    volume_id: &str,
    node_id: &str,
    chunk_size: u64,
    local_raid: &str,
) -> Result<Vec<(u32, String)>, String> {
    let new_end = offset + data.len() as u64;

    // Ensure chunks exist for the new file size
    let current_size: u64 = db.query_row(
        "SELECT size_bytes FROM file_map WHERE id = ?1",
        rusqlite::params![file_id], |row| row.get(0),
    ).unwrap_or(0);
    let new_size = current_size.max(new_end);
    ensure_chunks(db, file_id, new_size, chunk_size, volume_id, node_id, local_raid);

    let ranges = affected_chunks(offset, data.len() as u64, chunk_size);
    let mut changed = Vec::new();
    let mut data_offset = 0usize;

    for range in ranges {
        // Get all backends for this chunk (for mirror writes)
        let chunk_backends: Vec<(i64, String, String)> = {
            let mut stmt = db.prepare(
                "SELECT fc.id, cr.backend_id, b.path FROM chunk_replicas cr
                 JOIN backends b ON b.id = cr.backend_id
                 JOIN file_chunks fc ON fc.id = cr.chunk_id
                 WHERE fc.file_id = ?1 AND fc.chunk_index = ?2 AND cr.node_id = ?3"
            ).unwrap();
            stmt.query_map(
                rusqlite::params![file_id, range.chunk_index, node_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).unwrap().filter_map(|r| r.ok()).collect()
        };

        if chunk_backends.is_empty() {
            data_offset += range.size as usize;
            continue;
        }

        let chunk_data_slice = &data[data_offset..data_offset + range.size as usize];
        data_offset += range.size as usize;

        // Write to each backend replica (mirror writes).
        // If a backend fails, mark it as error and try to write to a fallback backend.
        let mut chunk_sha = String::new();
        let mut at_least_one_write_ok = false;

        for (chunk_id, backend_id, backend_path) in &chunk_backends {
            let path = chunk_path(backend_path, volume_id, file_id, range.chunk_index);

            if let Some(parent) = path.parent() {
                if std::fs::create_dir_all(parent).is_err() {
                    // Can't even create directory — backend is probably dead
                    tracing::warn!("Chunk write: cannot create dir on backend {}, marking error", backend_id);
                    db.execute(
                        "UPDATE chunk_replicas SET state = 'error' WHERE chunk_id = ?1 AND backend_id = ?2",
                        rusqlite::params![chunk_id, backend_id],
                    ).ok();
                    db.execute(
                        "UPDATE backends SET status = 'degraded' WHERE id = ?1",
                        rusqlite::params![backend_id],
                    ).ok();
                    continue;
                }
            }

            // Read-modify-write: read existing chunk, patch, write back
            let mut existing = std::fs::read(&path).unwrap_or_default();
            let end = range.local_offset as usize + chunk_data_slice.len();
            if existing.len() < end {
                existing.resize(end, 0);
            }
            existing[range.local_offset as usize..end].copy_from_slice(chunk_data_slice);

            // Atomic write: temp + fsync + rename
            let tmp = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
            match std::fs::write(&tmp, &existing) {
                Ok(_) => {
                    if let Ok(f) = std::fs::File::open(&tmp) {
                        f.sync_all().ok();
                    }
                    match std::fs::rename(&tmp, &path) {
                        Ok(_) => {
                            let sha = format!("{:x}", Sha256::digest(&existing));
                            chunk_sha = sha.clone();

                            let now = chrono::Utc::now().to_rfc3339();
                            db.execute(
                                "UPDATE chunk_replicas SET state = 'synced', synced_at = ?1
                                 WHERE chunk_id = ?2 AND backend_id = ?3",
                                rusqlite::params![&now, chunk_id, backend_id],
                            ).ok();
                            at_least_one_write_ok = true;
                        }
                        Err(e) => {
                            std::fs::remove_file(&tmp).ok();
                            tracing::warn!("Chunk write rename failed on backend {}: {}", backend_id, e);
                            db.execute(
                                "UPDATE chunk_replicas SET state = 'error' WHERE chunk_id = ?1 AND backend_id = ?2",
                                rusqlite::params![chunk_id, backend_id],
                            ).ok();
                            db.execute(
                                "UPDATE backends SET status = 'degraded' WHERE id = ?1",
                                rusqlite::params![backend_id],
                            ).ok();
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Chunk write failed on backend {}: {} — marking degraded", backend_id, e);
                    std::fs::remove_file(&tmp).ok();
                    db.execute(
                        "UPDATE chunk_replicas SET state = 'error' WHERE chunk_id = ?1 AND backend_id = ?2",
                        rusqlite::params![chunk_id, backend_id],
                    ).ok();
                    db.execute(
                        "UPDATE backends SET status = 'degraded' WHERE id = ?1",
                        rusqlite::params![backend_id],
                    ).ok();
                }
            }
        }

        // If ALL assigned backends failed, try writing to ANY other healthy local backend
        if !at_least_one_write_ok {
            let assigned_ids: Vec<&str> = chunk_backends.iter().map(|(_, id, _)| id.as_str()).collect();
            let fallback = db.query_row(
                "SELECT id, path FROM backends WHERE node_id = ?1 AND status = 'online' ORDER BY free_bytes DESC LIMIT 1",
                rusqlite::params![node_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            ).ok();

            if let Some((fb_id, fb_path)) = fallback {
                if !assigned_ids.contains(&fb_id.as_str()) {
                    let path = chunk_path(&fb_path, volume_id, file_id, range.chunk_index);
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    let mut existing = Vec::new();
                    let end = range.local_offset as usize + chunk_data_slice.len();
                    existing.resize(end, 0);
                    existing[range.local_offset as usize..end].copy_from_slice(chunk_data_slice);

                    if std::fs::write(&path, &existing).is_ok() {
                        let sha = format!("{:x}", Sha256::digest(&existing));
                        chunk_sha = sha;
                        let now = chrono::Utc::now().to_rfc3339();
                        let chunk_id = chunk_backends.first().map(|(id, _, _)| *id).unwrap_or(0);
                        db.execute(
                            "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
                             VALUES (?1, ?2, ?3, 'synced', ?4)",
                            rusqlite::params![chunk_id, &fb_id, node_id, &now],
                        ).ok();
                        at_least_one_write_ok = true;
                        tracing::info!("Chunk write: fallback to backend {} succeeded", fb_id);
                    }
                }
            }

            if !at_least_one_write_ok {
                return Err(format!("All backends failed for chunk {} of file {}", range.chunk_index, file_id));
            }
        }

        // Update chunk SHA256
        if !chunk_sha.is_empty() {
            db.execute(
                "UPDATE file_chunks SET sha256 = ?1
                 WHERE file_id = ?2 AND chunk_index = ?3",
                rusqlite::params![&chunk_sha, file_id, range.chunk_index],
            ).ok();
        }

        // Mark chunk replicas on OTHER nodes as stale
        if let Some((chunk_id, _, _)) = chunk_backends.first() {
            db.execute(
                "UPDATE chunk_replicas SET state = 'stale'
                 WHERE chunk_id = ?1 AND node_id != ?2",
                rusqlite::params![chunk_id, node_id],
            ).ok();
        }

        changed.push((range.chunk_index, chunk_sha));
    }

    // Update file size — use MAX to never shrink (writes only grow the logical size)
    let old_size: u64 = db.query_row(
        "SELECT size_bytes FROM file_map WHERE id = ?1",
        rusqlite::params![file_id], |row| row.get(0),
    ).unwrap_or(0);
    if new_size > old_size {
        tracing::info!("write_chunk_data: file {} size {} -> {} (grew)", file_id, old_size, new_size);
    }
    log_err!(db.execute(
        "UPDATE file_map SET size_bytes = MAX(size_bytes, ?1), updated_at = datetime('now')
         WHERE id = ?2",
        rusqlite::params![new_size as i64, file_id],
    ), "write_chunk_data: UPDATE file size");

    Ok(changed)
}

/// Compute protection status for a file based on FTT.
pub fn compute_protection_status(db: &Connection, file_id: i64, ftt: u32) -> &'static str {
    if ftt == 0 {
        return "unprotected";
    }

    let required_copies = ftt + 1;

    // Find chunks with fewer than required distinct node replicas.
    // Exclude thin-provisioned chunks (zero synced replicas = never written, intentionally empty).
    let degraded_count: u64 = db.query_row(
        "SELECT COUNT(*) FROM file_chunks fc
         WHERE fc.file_id = ?1 AND (
             SELECT COUNT(DISTINCT cr.node_id) FROM chunk_replicas cr
             WHERE cr.chunk_id = fc.id AND cr.state = 'synced'
         ) < ?2
         AND EXISTS (
             SELECT 1 FROM chunk_replicas cr2
             WHERE cr2.chunk_id = fc.id AND cr2.state = 'synced'
         )",
        rusqlite::params![file_id, required_copies],
        |row| row.get(0),
    ).unwrap_or(0);

    if degraded_count == 0 {
        "protected"
    } else {
        "degraded"
    }
}
