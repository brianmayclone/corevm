//! Metadata synchronization engine — leader pushes file_map to all peers.
//!
//! The leader node is the metadata master. It periodically syncs file metadata
//! (file_map + file_chunks layout) to all online peers so they know about all
//! files in the cluster, even if they don't have local chunk replicas yet.
//!
//! This solves the problem where Host B doesn't see files that Host A wrote,
//! because each node has its own independent SQLite DB.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::peer::client::PeerClient;

/// Spawn the metadata sync engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        // Sync every 10 seconds — fast enough for near-real-time visibility
        let mut tick = interval(Duration::from_secs(10));
        let client = PeerClient::new(&state.config.peer.secret);

        loop {
            tick.tick().await;

            // Leader does full metadata sync; non-leaders sync only files they own
            // (write_owner == our node_id). This ensures metadata reaches peers even
            // if the leader election is delayed.
            let is_leader = state.is_leader.load(std::sync::atomic::Ordering::Relaxed);

            let quorum = *state.quorum_status.read().unwrap();
            if quorum == crate::state::QuorumStatus::Fenced {
                continue;
            }

            sync_metadata_to_peers(&state, &client, is_leader).await;
        }
    });
}

/// Push file metadata to all online peers.
/// Leader syncs ALL files; non-leaders only sync files they currently own (write_owner).
async fn sync_metadata_to_peers(state: &CoreSanState, client: &PeerClient, is_leader: bool) {
    let files: Vec<serde_json::Value> = {
        let db = state.db.lock().unwrap();

        let query = if is_leader {
            // Leader: sync all files
            "SELECT fm.id, fm.volume_id, fm.rel_path, fm.size_bytes, fm.sha256,
                    fm.version, fm.chunk_count, v.chunk_size_bytes, v.ftt, v.local_raid
             FROM file_map fm
             JOIN volumes v ON v.id = fm.volume_id
             WHERE fm.size_bytes > 0 OR fm.chunk_count > 0"
        } else {
            // Non-leader: only sync files we own (recently wrote to)
            "SELECT fm.id, fm.volume_id, fm.rel_path, fm.size_bytes, fm.sha256,
                    fm.version, fm.chunk_count, v.chunk_size_bytes, v.ftt, v.local_raid
             FROM file_map fm
             JOIN volumes v ON v.id = fm.volume_id
             WHERE fm.write_owner = ?1 AND (fm.size_bytes > 0 OR fm.chunk_count > 0)"
        };

        let mut stmt = db.prepare(query).unwrap();

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

    if files.is_empty() {
        return;
    }

    // Get online peers
    let peers: Vec<(String, String)> = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .map(|p| (p.node_id.clone(), p.address.clone()))
        .collect();

    if peers.is_empty() {
        return;
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

    if synced_count > 0 {
        tracing::debug!("Leader metadata sync: {} file entries synced to {} peers",
            files.len(), peers.len());
    }
}
