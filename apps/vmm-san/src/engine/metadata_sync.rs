//! Metadata synchronization engine — leader pushes file_map to all peers.
//!
//! The leader node is the metadata master. It periodically syncs file metadata
//! (file_map + file_chunks layout) to all online peers so they know about all
//! files in the cluster, even if they don't have local chunk replicas yet.
//!
//! Only syncs files that have changed since the last sync cycle (tracked via
//! updated_at timestamp) to avoid redundant traffic.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::peer::client::PeerClient;

/// Spawn the metadata sync engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(10));
        let client = PeerClient::new(&state.config.peer.secret);
        let mut last_sync_at = String::new(); // tracks last synced updated_at

        loop {
            tick.tick().await;

            let is_leader = state.is_leader.load(std::sync::atomic::Ordering::Relaxed);

            let quorum = *state.quorum_status.read().unwrap();
            if quorum == crate::state::QuorumStatus::Fenced
                || quorum == crate::state::QuorumStatus::Sanitizing {
                continue;
            }

            last_sync_at = sync_metadata_to_peers(&state, &client, is_leader, &last_sync_at).await;
        }
    });
}

/// Push changed file metadata to all online peers.
/// Returns the new last_sync_at watermark.
async fn sync_metadata_to_peers(
    state: &CoreSanState,
    client: &PeerClient,
    is_leader: bool,
    last_sync_at: &str,
) -> String {
    let files: Vec<serde_json::Value> = {
        let db = state.db.read();

        // Only sync files changed since last cycle (or all on first run)
        let time_filter = if last_sync_at.is_empty() {
            "1=1".to_string()
        } else {
            format!("fm.updated_at > '{}'", last_sync_at)
        };

        let query = if is_leader {
            format!(
                "SELECT fm.id, fm.volume_id, fm.rel_path, fm.size_bytes, fm.sha256,
                        fm.version, fm.chunk_count, v.chunk_size_bytes, v.ftt, v.local_raid
                 FROM file_map fm
                 JOIN volumes v ON v.id = fm.volume_id
                 WHERE ({}) AND (fm.size_bytes > 0 OR fm.chunk_count > 0)", time_filter)
        } else {
            format!(
                "SELECT fm.id, fm.volume_id, fm.rel_path, fm.size_bytes, fm.sha256,
                        fm.version, fm.chunk_count, v.chunk_size_bytes, v.ftt, v.local_raid
                 FROM file_map fm
                 JOIN volumes v ON v.id = fm.volume_id
                 WHERE fm.write_owner = ?1 AND ({}) AND (fm.size_bytes > 0 OR fm.chunk_count > 0)", time_filter)
        };

        let mut stmt = db.prepare(&query).unwrap();

        let mapper = |row: &rusqlite::Row| {
            Ok(serde_json::json!({
                "file_id": row.get::<_, i64>(0)?,
                "volume_id": row.get::<_, String>(1)?,
                "rel_path": row.get::<_, String>(2)?,
                "size_bytes": row.get::<_, u64>(3)?,
                "sha256": row.get::<_, String>(4)?,
                "version": row.get::<_, i64>(5)?,
                "chunk_count": row.get::<_, u32>(6)?,
                "chunk_size_bytes": row.get::<_, u64>(7)?,
                "ftt": row.get::<_, u32>(8)?,
                "local_raid": row.get::<_, String>(9)?,
            }))
        };

        if is_leader {
            stmt.query_map([], mapper).unwrap().filter_map(|r| r.ok()).collect()
        } else {
            stmt.query_map(rusqlite::params![&state.node_id], mapper)
                .unwrap().filter_map(|r| r.ok()).collect()
        }
    };

    // Remember current time as watermark for next cycle
    let new_watermark = chrono::Utc::now().to_rfc3339();

    if files.is_empty() {
        return new_watermark;
    }

    // Get online peers
    let peers: Vec<(String, String)> = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .map(|p| (p.node_id.clone(), p.address.clone()))
        .collect();

    if peers.is_empty() {
        return new_watermark;
    }

    let mut synced_count = 0;
    for (peer_node_id, peer_addr) in &peers {
        for meta in &files {
            match client.push_file_meta(peer_addr, meta).await {
                Ok(_) => synced_count += 1,
                Err(e) => {
                    tracing::warn!("Metadata sync to {} failed for {}/{}: {}",
                        peer_node_id,
                        meta["volume_id"].as_str().unwrap_or("?"),
                        meta["rel_path"].as_str().unwrap_or("?"),
                        e);
                }
            }
        }
    }

    tracing::info!("Metadata sync: {} file(s) synced to {} peer(s)",
        files.len(), peers.len());

    new_watermark
}
