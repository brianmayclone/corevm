//! Database mirror — replicates the SQLite metadata DB across all claimed disks.
//!
//! The CoreSAN metadata DB is critical — without it, chunk locations are lost.
//! This engine periodically copies the DB to every claimed disk as a backup.
//! On startup, if the primary DB is missing/corrupt, it can be restored from
//! any surviving disk.

use std::sync::Arc;
use std::path::Path;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;

const MIRROR_INTERVAL_SECS: u64 = 60;
const DB_BACKUP_NAME: &str = ".coresan-metadata.db";

/// Spawn the DB mirror engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        // Wait for initial startup
        tokio::time::sleep(Duration::from_secs(10)).await;

        let mut tick = interval(Duration::from_secs(MIRROR_INTERVAL_SECS));
        loop {
            tick.tick().await;
            mirror_db(&state);
        }
    });
}

/// Copy the SQLite DB to all claimed disk mount paths.
fn mirror_db(state: &CoreSanState) {
    let db_path = state.config.data.data_dir.join("vmm-san.db");
    if !db_path.exists() {
        return;
    }

    // Get all mounted claimed disk paths
    let mount_paths: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT mount_path FROM claimed_disks WHERE status = 'mounted'"
        ).unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    if mount_paths.is_empty() {
        return;
    }

    // Read the current DB file
    let db_data = match std::fs::read(&db_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("DB mirror: cannot read {}: {}", db_path.display(), e);
            return;
        }
    };

    let mut ok = 0;
    let mut fail = 0;

    for mp in &mount_paths {
        let dest = Path::new(mp).join(DB_BACKUP_NAME);
        match std::fs::write(&dest, &db_data) {
            Ok(_) => ok += 1,
            Err(e) => {
                tracing::warn!("DB mirror: failed to write to {}: {}", dest.display(), e);
                fail += 1;
            }
        }
    }

    if ok > 0 {
        tracing::debug!("DB mirror: replicated to {} disks ({} failed)", ok, fail);
    }
}

/// On startup: try to restore the DB from any claimed disk if the primary is missing.
pub fn try_restore_from_disk(data_dir: &std::path::Path) -> bool {
    let db_path = data_dir.join("vmm-san.db");

    // If primary DB exists and is non-empty, no restore needed
    if db_path.exists() {
        if let Ok(meta) = std::fs::metadata(&db_path) {
            if meta.len() > 0 {
                return false;
            }
        }
    }

    tracing::warn!("Primary DB missing or empty — scanning disks for backup...");

    // Scan common CoreSAN disk mount locations
    let san_disk_dir = std::path::Path::new("/vmm/san-disks");
    if !san_disk_dir.exists() {
        return false;
    }

    let entries = match std::fs::read_dir(san_disk_dir) {
        Ok(e) => e,
        Err(_) => return false,
    };

    let mut newest_backup: Option<(std::time::SystemTime, std::path::PathBuf)> = None;

    for entry in entries.flatten() {
        let backup = entry.path().join(DB_BACKUP_NAME);
        if backup.exists() {
            if let Ok(meta) = std::fs::metadata(&backup) {
                if meta.len() > 0 {
                    let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    if newest_backup.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                        newest_backup = Some((mtime, backup));
                    }
                }
            }
        }
    }

    if let Some((_, backup_path)) = newest_backup {
        tracing::info!("Restoring DB from: {}", backup_path.display());
        match std::fs::copy(&backup_path, &db_path) {
            Ok(_) => {
                tracing::info!("DB restored successfully");
                true
            }
            Err(e) => {
                tracing::error!("DB restore failed: {}", e);
                false
            }
        }
    } else {
        tracing::warn!("No DB backup found on any disk");
        false
    }
}
