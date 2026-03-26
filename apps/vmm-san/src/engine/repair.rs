//! Repair engine — detects and fixes under-replicated files.
//!
//! Runs periodically, completely independent of vmm-cluster.
//! When a peer goes offline, the repair engine creates new replicas on surviving peers.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::peer::client::PeerClient;
use crate::storage::file_map;
use crate::engine::placement;

/// Spawn the repair engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    let repair_interval = state.config.integrity.repair_interval_secs;
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(repair_interval));
        let client = PeerClient::new(&state.config.peer.secret);

        loop {
            tick.tick().await;
            run_repair(&state, &client).await;
        }
    });
}

/// Run one repair cycle: find under-replicated files and create new replicas.
async fn run_repair(state: &CoreSanState, client: &PeerClient) {
    let under_replicated = {
        let db = state.db.lock().unwrap();
        file_map::find_under_replicated(&db)
    };

    if under_replicated.is_empty() {
        return;
    }

    tracing::info!("Repair: found {} under-replicated files", under_replicated.len());

    for file in under_replicated {
        let needed = file.desired_replicas - file.current_synced;
        for _ in 0..needed {
            repair_single_file(state, client, &file).await;
        }
    }
}

async fn repair_single_file(
    state: &CoreSanState,
    client: &PeerClient,
    file: &file_map::UnderReplicatedFile,
) {
    let (target, source) = {
        let db = state.db.lock().unwrap();
        let target = placement::select_new_replica_target(&db, file.file_id, &file.volume_id);
        let source = file_map::find_any_replica(&db, &file.volume_id, &file.rel_path);
        (target, source)
    };

    let (target_backend_id, target_node_id, target_path) = match target {
        Some(t) => t,
        None => {
            tracing::warn!("Repair: no target backend for {}/{}", file.volume_id, file.rel_path);
            return;
        }
    };

    let (source_node_id, source_backend_path, _) = match source {
        Some(s) => s,
        None => {
            tracing::warn!("Repair: no source for {}/{}", file.volume_id, file.rel_path);
            return;
        }
    };

    // If both source and target are local, just copy the file
    if source_node_id == state.node_id && target_node_id == state.node_id {
        let src = std::path::Path::new(&source_backend_path).join(&file.rel_path);
        let dst = std::path::Path::new(&target_path).join(&file.rel_path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if std::fs::copy(&src, &dst).is_ok() {
            mark_replica_synced(state, file.file_id, &target_backend_id);
            tracing::info!("Repair: local copy {}/{}", file.volume_id, file.rel_path);
        }
        return;
    }

    // If source is local, push to remote target
    if source_node_id == state.node_id {
        let src = std::path::Path::new(&source_backend_path).join(&file.rel_path);
        let data = match std::fs::read(&src) {
            Ok(d) => d,
            Err(_) => return,
        };
        let peer_addr = match state.peers.get(&target_node_id) {
            Some(p) => p.address.clone(),
            None => return,
        };
        if client.push_file(&peer_addr, &file.volume_id, &file.rel_path, data).await.is_ok() {
            mark_replica_synced(state, file.file_id, &target_backend_id);
            tracing::info!("Repair: pushed {}/{} to {}", file.volume_id, file.rel_path, target_node_id);
        }
        return;
    }

    // If target is local, pull from remote source
    if target_node_id == state.node_id {
        let peer_addr = match state.peers.get(&source_node_id) {
            Some(p) => p.address.clone(),
            None => return,
        };
        match client.pull_file(&peer_addr, &file.volume_id, &file.rel_path).await {
            Ok(data) => {
                let dst = std::path::Path::new(&target_path).join(&file.rel_path);
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                if std::fs::write(&dst, &data).is_ok() {
                    mark_replica_synced(state, file.file_id, &target_backend_id);
                    tracing::info!("Repair: pulled {}/{} from {}", file.volume_id, file.rel_path, source_node_id);
                }
            }
            Err(e) => {
                tracing::warn!("Repair: pull failed for {}/{}: {}", file.volume_id, file.rel_path, e);
            }
        }
    }
}

fn mark_replica_synced(state: &CoreSanState, file_id: i64, backend_id: &str) {
    let db = state.db.lock().unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(
        "INSERT OR REPLACE INTO file_replicas (file_id, backend_id, state, synced_at)
         VALUES (?1, ?2, 'synced', ?3)",
        rusqlite::params![file_id, backend_id, &now],
    ).ok();
}
