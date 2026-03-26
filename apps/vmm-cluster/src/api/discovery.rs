//! Discovery API — exposes auto-discovered nodes from UDP broadcasts.

use axum::extract::State;
use axum::Json;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::engine::discovery::DiscoveredNode;

/// GET /api/discovery/nodes — all discovered nodes on the network.
pub async fn list_nodes(
    State(state): State<Arc<ClusterState>>,
) -> Json<Vec<DiscoveredNode>> {
    Json(state.discovery.list())
}

/// GET /api/discovery/servers — unmanaged vmm-server nodes (candidates for registration).
pub async fn unmanaged_servers(
    State(state): State<Arc<ClusterState>>,
) -> Json<Vec<DiscoveredNode>> {
    Json(state.discovery.unmanaged_servers())
}

/// GET /api/discovery/san — discovered CoreSAN instances.
pub async fn san_nodes(
    State(state): State<Arc<ClusterState>>,
) -> Json<Vec<DiscoveredNode>> {
    Json(state.discovery.san_nodes())
}
