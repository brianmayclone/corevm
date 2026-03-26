//! Write lease management — ensures only one node writes to a file at a time.
//!
//! ## How it works:
//!
//! Before writing to a file, a node must acquire a **write lease**. The lease is
//! stored in `file_map.write_owner` + `file_map.write_lease_until`. A lease is
//! valid for a short duration (default 30 seconds) and auto-renewed on each write.
//!
//! If another node wants to write, it must first check:
//! 1. Is there a current owner? If not, acquire freely.
//! 2. Is the lease expired? If yes, steal the lease.
//! 3. Is the owner still online? If not, steal the lease.
//! 4. Otherwise, reject the write (the file is owned by another node).
//!
//! This is a **cooperative** lease — nodes are expected to release leases when
//! done writing (e.g., when a VM shuts down). The lease timeout is a safety net.
//!
//! ## Why not distributed locks?
//!
//! CoreSAN runs on each node independently. There is no central coordinator.
//! A distributed lock (like a Raft-based lock) would add latency and complexity.
//! Instead, we use a lease with timeout — simple, fast, and partition-tolerant.
//! In the rare case of a split-brain, the lease timeout prevents permanent deadlock.

use rusqlite::Connection;

/// Default lease duration in seconds.
const LEASE_DURATION_SECS: i64 = 30;

/// Result of a lease acquisition attempt.
#[derive(Debug)]
pub enum LeaseResult {
    /// Lease acquired — this node is the write owner.
    Acquired { version: i64 },
    /// Already owned by this node — lease renewed.
    Renewed { version: i64 },
    /// Owned by another node — write denied.
    Denied { owner_node_id: String, until: String },
}

/// Try to acquire or renew a write lease for a file.
pub fn acquire_lease(
    db: &Connection,
    volume_id: &str,
    rel_path: &str,
    node_id: &str,
    quorum: crate::state::QuorumStatus,
) -> LeaseResult {
    // Fenced nodes cannot acquire or renew leases
    if quorum == crate::state::QuorumStatus::Fenced {
        return LeaseResult::Denied {
            owner_node_id: String::new(),
            until: "node is fenced (no quorum)".into(),
        };
    }

    let now = chrono::Utc::now();
    let until = (now + chrono::Duration::seconds(LEASE_DURATION_SECS))
        .to_rfc3339();
    let now_str = now.to_rfc3339();

    // Check current lease state
    let current = db.query_row(
        "SELECT write_owner, write_lease_until, version FROM file_map
         WHERE volume_id = ?1 AND rel_path = ?2",
        rusqlite::params![volume_id, rel_path],
        |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        )),
    );

    match current {
        Ok((owner, lease_until, version)) => {
            if owner.is_empty() || owner == node_id {
                // No owner or we already own it — acquire/renew
                db.execute(
                    "UPDATE file_map SET write_owner = ?1, write_lease_until = ?2
                     WHERE volume_id = ?3 AND rel_path = ?4",
                    rusqlite::params![node_id, &until, volume_id, rel_path],
                ).ok();

                if owner == node_id {
                    LeaseResult::Renewed { version }
                } else {
                    // Ownership changed — increment epoch
                    db.execute(
                        "UPDATE file_map SET ownership_epoch = ownership_epoch + 1
                         WHERE volume_id = ?1 AND rel_path = ?2",
                        rusqlite::params![volume_id, rel_path],
                    ).ok();
                    LeaseResult::Acquired { version }
                }
            } else if lease_until < now_str {
                // Lease expired — steal it
                tracing::info!("Write lease expired for {}/{} (was owned by {}), stealing",
                    volume_id, rel_path, owner);
                db.execute(
                    "UPDATE file_map SET write_owner = ?1, write_lease_until = ?2
                     WHERE volume_id = ?3 AND rel_path = ?4",
                    rusqlite::params![node_id, &until, volume_id, rel_path],
                ).ok();
                // Ownership changed — increment epoch
                db.execute(
                    "UPDATE file_map SET ownership_epoch = ownership_epoch + 1
                     WHERE volume_id = ?1 AND rel_path = ?2",
                    rusqlite::params![volume_id, rel_path],
                ).ok();
                LeaseResult::Acquired { version }
            } else {
                // Owned by someone else, lease still valid
                LeaseResult::Denied { owner_node_id: owner, until: lease_until }
            }
        }
        Err(_) => {
            // File doesn't exist in file_map yet — no lease needed,
            // it will be created on first write
            LeaseResult::Acquired { version: 0 }
        }
    }
}

/// Release a write lease (e.g., when a VM shuts down or file handle is closed).
pub fn release_lease(
    db: &Connection,
    volume_id: &str,
    rel_path: &str,
    node_id: &str,
) {
    db.execute(
        "UPDATE file_map SET write_owner = '', write_lease_until = ''
         WHERE volume_id = ?1 AND rel_path = ?2 AND write_owner = ?3",
        rusqlite::params![volume_id, rel_path, node_id],
    ).ok();
}

/// Release all leases held by a specific node (e.g., when node goes offline).
pub fn release_all_leases_for_node(db: &Connection, node_id: &str) {
    let count = db.execute(
        "UPDATE file_map SET write_owner = '', write_lease_until = ''
         WHERE write_owner = ?1",
        rusqlite::params![node_id],
    ).unwrap_or(0);
    if count > 0 {
        tracing::info!("Released {} write leases for offline node {}", count, node_id);
    }
}

/// Expire all stale leases (called periodically).
pub fn expire_stale_leases(db: &Connection) {
    let now = chrono::Utc::now().to_rfc3339();
    let count = db.execute(
        "UPDATE file_map SET write_owner = '', write_lease_until = ''
         WHERE write_owner != '' AND write_lease_until < ?1",
        rusqlite::params![&now],
    ).unwrap_or(0);
    if count > 0 {
        tracing::debug!("Expired {} stale write leases", count);
    }
}

/// Perform an atomic write: acquire lease, write file, update metadata, log to write_log.
/// Returns the new version number, or an error string.
pub fn atomic_write(
    db: &Connection,
    volume_id: &str,
    rel_path: &str,
    node_id: &str,
    backend_id: &str,
    backend_path: &str,
    data: &[u8],
    offset: Option<i64>,
    quorum: crate::state::QuorumStatus,
) -> Result<i64, String> {
    // 1. Acquire/renew lease
    let version = match acquire_lease(db, volume_id, rel_path, node_id, quorum) {
        LeaseResult::Acquired { version } | LeaseResult::Renewed { version } => version,
        LeaseResult::Denied { owner_node_id, .. } => {
            return Err(format!("File is owned by node {}", owner_node_id));
        }
    };
    let new_version = version + 1;

    // 2. Build full path
    let full_path = std::path::Path::new(backend_path).join(rel_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir: {}", e))?;
    }

    // 3. Atomic write: write to temp file, fsync, rename
    let tmp_path = full_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));

    let content = if let Some(off) = offset {
        // Offset-based write (FUSE): read existing, apply patch
        let mut existing = std::fs::read(&full_path).unwrap_or_default();
        let end = off as usize + data.len();
        if existing.len() < end {
            existing.resize(end, 0);
        }
        existing[off as usize..end].copy_from_slice(data);
        existing
    } else {
        // Full file write (API): replace entire content
        data.to_vec()
    };

    // Write to temp file
    std::fs::write(&tmp_path, &content)
        .map_err(|e| format!("write tmp: {}", e))?;

    // fsync the temp file for durability
    if let Ok(f) = std::fs::File::open(&tmp_path) {
        f.sync_all().ok();
    }

    // Atomic rename
    std::fs::rename(&tmp_path, &full_path)
        .map_err(|e| {
            std::fs::remove_file(&tmp_path).ok();
            format!("rename: {}", e)
        })?;

    // 4. Compute checksum
    use sha2::{Sha256, Digest};
    let sha256 = format!("{:x}", Sha256::digest(&content));
    let size = content.len() as u64;
    let now = chrono::Utc::now().to_rfc3339();

    // 5. Update file_map with new version + ownership tick
    db.execute(
        "INSERT INTO file_map (volume_id, rel_path, size_bytes, sha256, version, write_owner, write_lease_until, created_at, updated_at, ownership_tick)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1)
         ON CONFLICT(volume_id, rel_path) DO UPDATE SET
            size_bytes = excluded.size_bytes, sha256 = excluded.sha256,
            version = excluded.version, write_owner = excluded.write_owner,
            write_lease_until = excluded.write_lease_until,
            updated_at = excluded.updated_at,
            ownership_tick = ownership_tick + 1",
        rusqlite::params![volume_id, rel_path, size, &sha256, new_version, node_id,
                          &(chrono::Utc::now() + chrono::Duration::seconds(LEASE_DURATION_SECS)).to_rfc3339(),
                          &now],
    ).map_err(|e| format!("db file_map: {}", e))?;

    // 6. Get file_id
    let file_id: i64 = db.query_row(
        "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
        rusqlite::params![volume_id, rel_path],
        |row| row.get(0),
    ).map_err(|e| format!("db get file_id: {}", e))?;

    // 7. Mark local replica as synced with this version
    db.execute(
        "INSERT INTO file_replicas (file_id, backend_id, state, replica_version, synced_at)
         VALUES (?1, ?2, 'synced', ?3, ?4)
         ON CONFLICT(file_id, backend_id) DO UPDATE SET
            state = 'synced', replica_version = excluded.replica_version,
            synced_at = excluded.synced_at",
        rusqlite::params![file_id, backend_id, new_version, &now],
    ).ok();

    // 8. Mark other replicas as stale (version mismatch)
    db.execute(
        "UPDATE file_replicas SET state = 'stale'
         WHERE file_id = ?1 AND backend_id != ?2",
        rusqlite::params![file_id, backend_id],
    ).ok();

    // 9. Append to write_log for push replication (with ownership epoch/tick)
    let (epoch, tick): (i64, i64) = db.query_row(
        "SELECT ownership_epoch, ownership_tick FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
        rusqlite::params![volume_id, rel_path],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap_or((0, 0));

    db.execute(
        "INSERT INTO write_log (file_id, volume_id, rel_path, version, writer_node_id, size_bytes, sha256, ownership_epoch, ownership_tick)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![file_id, volume_id, rel_path, new_version, node_id, size, &sha256, epoch, tick],
    ).ok();

    Ok(new_version)
}
