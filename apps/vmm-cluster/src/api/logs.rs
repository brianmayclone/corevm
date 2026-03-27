//! Host log retrieval — proxies log requests to vmm-server agents.

use axum::{Json, extract::{State, Path, Query}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError};
use crate::node_client::NodeClient;

#[derive(Deserialize)]
pub struct LogQuery {
    pub service: Option<String>,
    pub lines: Option<usize>,
}

/// GET /api/hosts/{id}/logs — fetch service logs from a specific host.
///
/// The cluster proxies this request to the host's agent API.
pub async fn host_logs(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(host_id): Path<String>,
    Query(q): Query<LogQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let node = state.nodes.get(&host_id)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Host not found".into()))?;

    let client = NodeClient::new(&node.address, &node.agent_token)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let result = client.get_logs(q.service.as_deref(), q.lines).await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Failed to fetch logs from host: {}", e)))?;

    Ok(Json(result))
}

/// GET /api/logs — fetch logs from ALL online hosts (aggregated).
pub async fn all_logs(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Query(q): Query<LogQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let nodes: Vec<(String, String, String)> = state.nodes.iter()
        .filter(|n| matches!(n.status, crate::state::NodeStatus::Online))
        .map(|n| (n.node_id.clone(), n.address.clone(), n.agent_token.clone()))
        .collect();

    let mut results = Vec::new();

    for (node_id, address, token) in &nodes {
        let client = match NodeClient::new(address, token) {
            Ok(c) => c,
            Err(_) => continue,
        };

        match client.get_logs(q.service.as_deref(), q.lines).await {
            Ok(mut resp) => {
                // Inject host_id into the response
                if let Some(obj) = resp.as_object_mut() {
                    obj.insert("host_id".to_string(), serde_json::Value::String(node_id.clone()));
                }
                results.push(resp);
            }
            Err(_) => continue,
        }
    }

    Ok(Json(serde_json::json!({ "hosts": results })))
}
