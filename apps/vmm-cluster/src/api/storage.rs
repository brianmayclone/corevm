//! Storage/Datastore API handlers — cluster-wide datastore management.
//!
//! Datastores are defined centrally and mounted on all hosts in the cluster.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_operator};
use crate::services::datastore::DatastoreService;
use crate::services::audit::AuditService;
use crate::services::event::EventService;
use crate::services::host::HostService;
use crate::node_client::NodeClient;

#[derive(Deserialize)]
pub struct CreateDatastoreRequest {
    pub name: String,
    pub store_type: String,
    pub mount_source: String,
    #[serde(default)]
    pub mount_opts: String,
    pub mount_path: String,
    pub cluster_id: String,
}

pub async fn list_datastores(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let datastores = DatastoreService::list(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(datastores).unwrap()))
}

pub async fn get_datastore(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let ds = DatastoreService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::to_value(ds).unwrap()))
}

pub async fn create_datastore(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateDatastoreRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    // Validate store type
    if !["nfs", "cephfs", "glusterfs"].contains(&body.store_type.as_str()) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid store type (must be nfs, cephfs, glusterfs)".into()));
    }

    let ds_id = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        let id = DatastoreService::create(&db, &body.name, &body.store_type, &body.mount_source,
            &body.mount_opts, &body.mount_path, &body.cluster_id)
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
        AuditService::log(&db, user.id, "datastore.create", "datastore", &id, Some(&body.name));
        id
    };

    // Mount on all hosts in the cluster (async, best-effort)
    let hosts = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        HostService::list(&db).unwrap_or_default()
    };

    let cluster_hosts: Vec<_> = hosts.iter().filter(|h| h.cluster_id == body.cluster_id && h.status == "online").collect();
    let mount_req = vmm_core::cluster::MountDatastoreRequest {
        datastore_id: ds_id.clone(),
        store_type: body.store_type.clone(),
        mount_source: body.mount_source.clone(),
        mount_opts: body.mount_opts.clone(),
        mount_path: body.mount_path.clone(),
    };

    for host in &cluster_hosts {
        // Register mount entry
        if let Ok(db) = state.db.lock() {
            DatastoreService::add_host_mount(&db, &ds_id, &host.id).ok();
        }

        // Send mount command to node
        if let Some(node) = state.nodes.get(&host.id) {
            if let Ok(client) = NodeClient::new(&node.address, &node.agent_token) {
                let req = mount_req.clone();
                let host_id = host.id.clone();
                let ds_id_clone = ds_id.clone();
                let state_clone = state.clone();
                tokio::spawn(async move {
                    match client.mount_datastore(&req).await {
                        Ok(resp) if resp.success => {
                            if let Ok(db) = state_clone.db.lock() {
                                DatastoreService::update_host_mount(&db, &ds_id_clone, &host_id, true, "mounted", 0, 0).ok();
                            }
                        }
                        Ok(resp) => {
                            tracing::warn!("Datastore mount failed on {}: {:?}", host_id, resp.error);
                            if let Ok(db) = state_clone.db.lock() {
                                DatastoreService::update_host_mount(&db, &ds_id_clone, &host_id, false, "error", 0, 0).ok();
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Datastore mount error on {}: {}", host_id, e);
                        }
                    }
                });
            }
        }
    }

    // Update status to online
    if let Ok(db) = state.db.lock() {
        DatastoreService::update_status(&db, &ds_id, "online").ok();
        EventService::log(&db, "info", "datastore", &format!("Datastore '{}' created", body.name),
            Some("datastore"), Some(&ds_id), None);
    }

    Ok(Json(serde_json::json!({"id": ds_id})))
}

pub async fn delete_datastore(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    // Unmount from all hosts
    let ds = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        DatastoreService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?
    };

    for mount in &ds.host_mounts {
        if let Some(node) = state.nodes.get(&mount.host_id) {
            if let Ok(client) = NodeClient::new(&node.address, &node.agent_token) {
                let req = vmm_core::cluster::UnmountDatastoreRequest {
                    datastore_id: id.clone(),
                    mount_path: ds.mount_path.clone(),
                };
                let _ = client.unmount_datastore(&req).await;
            }
        }
    }

    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    DatastoreService::delete(&db, &id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "datastore.delete", "datastore", &id, None);
    Ok(Json(serde_json::json!({"ok": true})))
}
