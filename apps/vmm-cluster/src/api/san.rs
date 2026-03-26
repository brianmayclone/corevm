//! CoreSAN proxy endpoints — the cluster proxies all vSAN operations.
//!
//! The UI talks to vmm-cluster, which forwards requests to the appropriate
//! vmm-san host(s). Multi-host operations (disks, backends) are fanned out
//! and aggregated. All mutating operations are logged to the event log.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::auth::middleware::{AuthUser, AppError};
use crate::san_client::{SanClient, get_san_hosts, get_san_host_by_id};
use crate::services::event::EventService;
use crate::state::ClusterState;

/// Pick any online SAN host to forward a request to (volumes are synced across all).
fn any_san_client(state: &ClusterState) -> Result<(SanClient, String), AppError> {
    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let hosts = get_san_hosts(&db);
    let host = hosts.into_iter().next()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "No SAN hosts available".into()))?;
    let host_id = host.host_id.clone();
    Ok((SanClient::new(&host.san_address), host_id))
}

/// Route a request to a specific SAN host by cluster host_id.
fn san_client_for_host(state: &ClusterState, host_id: &str) -> Result<SanClient, AppError> {
    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let host = get_san_host_by_id(&db, host_id)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, format!("SAN host '{}' not found or not SAN-enabled", host_id)))?;
    Ok(SanClient::new(&host.san_address))
}

fn san_err(e: String) -> AppError {
    AppError(StatusCode::BAD_GATEWAY, e)
}

// ── Status ────────────────────────────────────────────────────────

/// GET /api/san/status — fan-out to ALL SAN hosts, return aggregated status.
pub async fn status(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let hosts = {
        let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        get_san_hosts(&db)
    };

    let mut results = Vec::new();
    let futures: Vec<_> = hosts.iter().map(|h| {
        let client = SanClient::new(&h.san_address);
        let host_id = h.host_id.clone();
        let hostname = h.hostname.clone();
        async move {
            match client.get_status().await {
                Ok(mut status) => {
                    if let Some(obj) = status.as_object_mut() {
                        obj.insert("_host_id".into(), Value::String(host_id));
                        obj.insert("_host_name".into(), Value::String(hostname));
                    }
                    Some(status)
                }
                Err(e) => {
                    tracing::warn!("SAN status failed for {} ({}): {}", hostname, host_id, e);
                    Some(serde_json::json!({
                        "_host_id": host_id,
                        "_host_name": hostname,
                        "running": false,
                        "error": e,
                    }))
                }
            }
        }
    }).collect();

    let statuses = futures::future::join_all(futures).await;
    for s in statuses.into_iter().flatten() {
        results.push(s);
    }

    Ok(Json(Value::Array(results)))
}

// ── Volumes ───────────────────────────────────────────────────────

/// GET /api/san/volumes — forward to any SAN host.
pub async fn list_volumes(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.list_volumes().await.map(Json).map_err(san_err)
}

/// POST /api/san/volumes — create volume, log event.
pub async fn create_volume(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let (client, host_id) = any_san_client(&state)?;
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");

    let result = client.create_volume(&body).await.map_err(san_err)?;

    let vol_id = result.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("Volume '{}' created (id={})", name, vol_id),
        Some("volume"), Some(vol_id), Some(&host_id));

    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/san/volumes/{id}
pub async fn get_volume(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.get_volume(&id).await.map(Json).map_err(san_err)
}

/// PUT /api/san/volumes/{id} — update volume policy, log event.
pub async fn update_volume(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let (client, host_id) = any_san_client(&state)?;
    let result = client.update_volume(&id, &body).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("Volume policy updated (id={})", id),
        Some("volume"), Some(&id), Some(&host_id));

    Ok(Json(result))
}

/// DELETE /api/san/volumes/{id} — delete volume, log event.
pub async fn delete_volume(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let (client, host_id) = any_san_client(&state)?;
    let result = client.delete_volume(&id).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("Volume deleted (id={})", id),
        Some("volume"), Some(&id), Some(&host_id));

    Ok(Json(result))
}

// ── Backends ──────────────────────────────────────────────────────

/// GET /api/san/volumes/{id}/backends — fan-out to ALL SAN hosts, merge.
pub async fn list_backends(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let hosts = {
        let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        get_san_hosts(&db)
    };

    let futures: Vec<_> = hosts.iter().map(|h| {
        let client = SanClient::new(&h.san_address);
        let host_id = h.host_id.clone();
        let hostname = h.hostname.clone();
        let vol_id = id.clone();
        async move {
            match client.list_backends(&vol_id).await {
                Ok(Value::Array(backends)) => {
                    backends.into_iter().map(|mut b| {
                        if let Some(obj) = b.as_object_mut() {
                            obj.insert("_host_id".into(), Value::String(host_id.clone()));
                            obj.insert("_host_name".into(), Value::String(hostname.clone()));
                        }
                        b
                    }).collect::<Vec<_>>()
                }
                _ => Vec::new(),
            }
        }
    }).collect();

    let all: Vec<Value> = futures::future::join_all(futures).await.into_iter().flatten().collect();
    Ok(Json(Value::Array(all)))
}

#[derive(Deserialize)]
pub struct AddBackendRequest {
    pub host_id: String,
    pub path: String,
}

/// POST /api/san/volumes/{id}/backends — route to specific host, log event.
pub async fn add_backend(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<AddBackendRequest>,
) -> Result<Json<Value>, AppError> {
    let client = san_client_for_host(&state, &body.host_id)?;
    let san_body = serde_json::json!({ "path": body.path });
    let result = client.add_backend(&id, &san_body).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("Backend added to volume {} on host {}", id, body.host_id),
        Some("backend"), None, Some(&body.host_id));

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct RemoveBackendPath {
    pub vid: String,
    pub bid: String,
}

#[derive(Deserialize)]
pub struct HostIdQuery {
    pub host_id: String,
}

/// DELETE /api/san/volumes/{vid}/backends/{bid} — route to specific host.
pub async fn remove_backend(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(path): Path<RemoveBackendPath>,
    Json(query): Json<HostIdQuery>,
) -> Result<Json<Value>, AppError> {
    let client = san_client_for_host(&state, &query.host_id)?;
    let result = client.remove_backend(&path.vid, &path.bid).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("Backend {} removed from volume {}", path.bid, path.vid),
        Some("backend"), Some(&path.bid), Some(&query.host_id));

    Ok(Json(result))
}

// ── Peers ─────────────────────────────────────────────────────────

/// GET /api/san/peers — forward to any SAN host.
pub async fn list_peers(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.list_peers().await.map(Json).map_err(san_err)
}

// ── Disks ─────────────────────────────────────────────────────────

/// GET /api/san/disks — fan-out to ALL SAN hosts, merge with host tags.
pub async fn list_disks(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let hosts = {
        let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        get_san_hosts(&db)
    };

    let futures: Vec<_> = hosts.iter().map(|h| {
        let client = SanClient::new(&h.san_address);
        let host_id = h.host_id.clone();
        let hostname = h.hostname.clone();
        async move {
            match client.list_disks().await {
                Ok(Value::Array(disks)) => {
                    disks.into_iter().map(|mut d| {
                        if let Some(obj) = d.as_object_mut() {
                            obj.insert("_host_id".into(), Value::String(host_id.clone()));
                            obj.insert("_host_name".into(), Value::String(hostname.clone()));
                        }
                        d
                    }).collect::<Vec<_>>()
                }
                Err(e) => {
                    tracing::warn!("SAN disk list failed for {}: {}", hostname, e);
                    Vec::new()
                }
                _ => Vec::new(),
            }
        }
    }).collect();

    let all: Vec<Value> = futures::future::join_all(futures).await.into_iter().flatten().collect();
    Ok(Json(Value::Array(all)))
}

#[derive(Deserialize)]
pub struct DiskActionRequest {
    pub host_id: String,
    pub device_path: String,
    #[serde(default)]
    pub confirm_format: bool,
}

/// POST /api/san/disks/claim — route to specific host, log event.
pub async fn claim_disk(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Json(body): Json<DiskActionRequest>,
) -> Result<Json<Value>, AppError> {
    let client = san_client_for_host(&state, &body.host_id)?;
    let san_body = serde_json::json!({
        "device_path": body.device_path,
        "confirm_format": body.confirm_format,
    });
    let result = client.claim_disk(&san_body).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("Disk {} claimed on host {}", body.device_path, body.host_id),
        Some("disk"), Some(&body.device_path), Some(&body.host_id));

    Ok(Json(result))
}

/// POST /api/san/disks/release — route to specific host, log event.
pub async fn release_disk(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Json(body): Json<DiskActionRequest>,
) -> Result<Json<Value>, AppError> {
    let client = san_client_for_host(&state, &body.host_id)?;
    let san_body = serde_json::json!({ "device_path": body.device_path });
    let result = client.release_disk(&san_body).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("Disk {} released on host {}", body.device_path, body.host_id),
        Some("disk"), Some(&body.device_path), Some(&body.host_id));

    Ok(Json(result))
}

/// POST /api/san/disks/reset — route to specific host, log event.
pub async fn reset_disk(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Json(body): Json<DiskActionRequest>,
) -> Result<Json<Value>, AppError> {
    let client = san_client_for_host(&state, &body.host_id)?;
    let san_body = serde_json::json!({ "device_path": body.device_path });
    let result = client.reset_disk(&san_body).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "warning", "san",
        &format!("Disk {} reset on host {}", body.device_path, body.host_id),
        Some("disk"), Some(&body.device_path), Some(&body.host_id));

    Ok(Json(result))
}

// ── Benchmark ─────────────────────────────────────────────────────

/// GET /api/san/benchmark — forward to any SAN host.
pub async fn benchmark_matrix(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.benchmark_matrix().await.map(Json).map_err(san_err)
}

/// POST /api/san/benchmark/run — trigger benchmark, log event.
pub async fn run_benchmark(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (client, host_id) = any_san_client(&state)?;
    let result = client.run_benchmark().await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        "Manual benchmark triggered",
        Some("benchmark"), None, Some(&host_id));

    Ok(Json(result))
}

// ── Volume File Operations ────────────────────────────────────────

#[derive(Deserialize)]
pub struct BrowsePath {
    pub id: String,
    pub path: String,
}

/// GET /api/san/volumes/{id}/browse/*path
pub async fn browse_volume(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path((id, path)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.browse_volume(&id, &path).await.map(Json).map_err(san_err)
}

/// GET /api/san/volumes/{id}/browse (root)
pub async fn browse_volume_root(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.browse_volume(&id, "").await.map(Json).map_err(san_err)
}

/// POST /api/san/volumes/{id}/mkdir
pub async fn mkdir_volume(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.mkdir_volume(&id, &body).await.map(Json).map_err(san_err)
}

/// DELETE /api/san/volumes/{id}/files/*path
pub async fn delete_file(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path((id, path)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.delete_file(&id, &path).await.map(Json).map_err(san_err)
}

/// PUT /api/san/volumes/{id}/files/*path
pub async fn upload_file(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path((id, path)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.upload_file(&id, &path, body.to_vec()).await.map(Json).map_err(san_err)
}

// ── Witness ──────────────────────────────────────────────────────

/// GET /api/san/witness/{node_id} — witness tie-breaker for SAN quorum.
/// No auth required — SAN nodes call this directly.
pub async fn witness(
    State(state): State<Arc<ClusterState>>,
    Path(requesting_node_id): Path<String>,
) -> Json<Value> {
    // Get ALL known SAN hosts
    let all_san_host_ids: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id FROM hosts WHERE san_enabled = 1 AND san_address != ''"
        ).unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    if all_san_host_ids.is_empty() {
        return Json(serde_json::json!({"allowed": false, "reason": "no SAN hosts known"}));
    }

    // Get SAN hosts the cluster considers ONLINE
    let reachable_ids: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id FROM hosts WHERE san_enabled = 1 AND san_address != '' AND status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    // Is the requesting node reachable from the cluster?
    if !reachable_ids.contains(&requesting_node_id) {
        return Json(serde_json::json!({
            "allowed": false,
            "reason": "requesting node not reachable from cluster"
        }));
    }

    let total = all_san_host_ids.len();
    let reachable = reachable_ids.len();
    let unreachable = total - reachable;

    if reachable > unreachable {
        return Json(serde_json::json!({"allowed": true}));
    }

    if reachable < unreachable {
        return Json(serde_json::json!({"allowed": false, "reason": "minority partition"}));
    }

    // Tie — partition with lowest host_id wins
    let lowest_overall = all_san_host_ids.iter().min().cloned().unwrap_or_default();
    let allowed = reachable_ids.contains(&lowest_overall);

    Json(serde_json::json!({
        "allowed": allowed,
        "reason": if allowed { "tie broken by lowest host_id" } else { "tie lost — lowest host_id in other partition" }
    }))
}

// ── Health ────────────────────────────────────────────────────────

/// GET /api/san/health — return latest health snapshot from the health engine.
pub async fn health(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let snapshot = state.san_health.read().unwrap();
    Ok(Json(snapshot.clone()))
}
