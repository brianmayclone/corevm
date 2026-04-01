//! Disk monitor — watches for hot-add and hot-remove of physical disks.
//!
//! Polls /sys/block/ every few seconds to detect disk changes.
//! On hot-remove: immediately marks the backend offline and triggers rebalancing.
//! On hot-add: logs the new disk as available for claiming.

use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;

const POLL_INTERVAL_SECS: u64 = 5;

/// Spawn the disk monitor as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        // Initial snapshot of known disks
        let mut known_disks = scan_block_devices();
        tracing::info!("Disk monitor started ({} devices, polling every {}s)",
            known_disks.len(), POLL_INTERVAL_SECS);

        let mut tick = interval(Duration::from_secs(POLL_INTERVAL_SECS));
        loop {
            tick.tick().await;

            let current_disks = scan_block_devices();

            // Detect hot-add
            for disk in current_disks.difference(&known_disks) {
                tracing::info!("Hot-add detected: /dev/{}", disk);
                // No auto-claim — just log it. The UI will show it as "available".
            }

            // Detect hot-remove
            for disk in known_disks.difference(&current_disks) {
                tracing::warn!("Hot-remove detected: /dev/{}", disk);
                handle_disk_removed(&state, disk);
            }

            known_disks = current_disks;
        }
    });
}

/// Handle a disk that was removed while running.
fn handle_disk_removed(state: &CoreSanState, disk_name: &str) {
    let device_path = format!("/dev/{}", disk_name);
    let db = state.db.write();

    // Find the claimed disk entry
    let claimed = db.query_row(
        "SELECT id, mount_path, backend_id FROM claimed_disks WHERE device_path = ?1 AND status = 'mounted'",
        rusqlite::params![&device_path],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
    );

    if let Ok((disk_id, mount_path, backend_id)) = claimed {
        tracing::warn!("Claimed disk {} removed! Marking backend {} offline, triggering self-heal",
            device_path, backend_id);

        // Mark backend as offline immediately
        db.execute(
            "UPDATE backends SET status = 'offline' WHERE id = ?1",
            rusqlite::params![&backend_id],
        ).ok();

        // Mark claimed disk as error
        db.execute(
            "UPDATE claimed_disks SET status = 'error' WHERE id = ?1",
            rusqlite::params![&disk_id],
        ).ok();

        // Mark all chunk_replicas on this backend as error
        let affected_chunks = db.execute(
            "UPDATE chunk_replicas SET state = 'error' WHERE backend_id = ?1 AND state = 'synced'",
            rusqlite::params![&backend_id],
        ).unwrap_or(0);

        tracing::warn!("Hot-remove: {} chunk replicas marked as error on backend {}",
            affected_chunks, backend_id);

        // Try to unmount the FUSE path (it's probably already dead)
        std::process::Command::new("umount")
            .args(["-l", &mount_path])
            .output().ok();

        // The rebalancer engine will pick up the offline backend and redistribute chunks
        // to other local backends. The repair engine will create new cross-node replicas
        // to maintain FTT.

    } else {
        tracing::info!("Removed disk {} was not claimed by CoreSAN, ignoring", device_path);
    }
}

/// Scan /sys/block/ for all block devices (excluding loop, ram, sr).
fn scan_block_devices() -> HashSet<String> {
    let sys_block = std::path::Path::new("/sys/block");
    let entries = match std::fs::read_dir(sys_block) {
        Ok(e) => e,
        Err(_) => return HashSet::new(),
    };

    entries.filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| {
            // Only real disks: sd*, nvme*, vd*, hd*, xvd*
            // Exclude: loop*, ram*, sr*, dm-*, md*
            !name.starts_with("loop")
                && !name.starts_with("ram")
                && !name.starts_with("sr")
                && !name.starts_with("dm-")
                && !name.starts_with("md")
        })
        .collect()
}
