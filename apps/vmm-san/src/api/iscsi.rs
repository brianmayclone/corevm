//! iSCSI ACL management REST endpoints.
//! Used by vmm-ui and vmm-cluster to create/list/delete initiator ACLs.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::state::CoreSanState;

#[derive(Deserialize)]
pub struct CreateAclRequest {
    pub volume_id: String,
    pub initiator_iqn: String,
    #[serde(default)]
    pub comment: String,
}

#[derive(Serialize)]
pub struct AclResponse {
    pub id: String,
    pub volume_id: String,
    pub volume_name: String,
    pub initiator_iqn: String,
    pub comment: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct AclQuery {
    pub volume_id: Option<String>,
}

#[derive(Serialize)]
pub struct TargetResponse {
    pub volume_id: String,
    pub volume_name: String,
    pub iqn: String,
    pub portals: Vec<String>,
    pub alua_state: String,
    pub status: String,
}

/// GET /api/iscsi/acls?volume_id=X
pub async fn list_acls(
    State(state): State<Arc<CoreSanState>>,
    Query(query): Query<AclQuery>,
) -> Result<Json<Vec<AclResponse>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let acls: Vec<AclResponse> = if let Some(ref vid) = query.volume_id {
        let mut stmt = db.prepare(
            "SELECT a.id, a.volume_id, v.name, a.initiator_iqn, a.comment, a.created_at
             FROM iscsi_acls a JOIN volumes v ON a.volume_id = v.id
             WHERE a.volume_id = ?1 ORDER BY a.created_at"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        stmt.query_map(rusqlite::params![vid], |row| {
            Ok(AclResponse {
                id: row.get(0)?, volume_id: row.get(1)?, volume_name: row.get(2)?,
                initiator_iqn: row.get(3)?, comment: row.get(4)?, created_at: row.get(5)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    } else {
        let mut stmt = db.prepare(
            "SELECT a.id, a.volume_id, v.name, a.initiator_iqn, a.comment, a.created_at
             FROM iscsi_acls a JOIN volumes v ON a.volume_id = v.id ORDER BY a.created_at"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        stmt.query_map([], |row| {
            Ok(AclResponse {
                id: row.get(0)?, volume_id: row.get(1)?, volume_name: row.get(2)?,
                initiator_iqn: row.get(3)?, comment: row.get(4)?, created_at: row.get(5)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    };
    Ok(Json(acls))
}

/// POST /api/iscsi/acls
pub async fn create_acl(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<CreateAclRequest>,
) -> Result<(StatusCode, Json<AclResponse>), (StatusCode, String)> {
    if !body.initiator_iqn.starts_with("iqn.") {
        return Err((StatusCode::BAD_REQUEST, "initiator_iqn must start with 'iqn.'".into()));
    }

    let db = state.db.lock().unwrap();

    // Verify volume exists and has iscsi protocol
    let vol_name: String = db.query_row(
        "SELECT name FROM volumes WHERE id = ?1", rusqlite::params![&body.volume_id],
        |row| row.get(0),
    ).map_err(|_| (StatusCode::NOT_FOUND, format!("Volume '{}' not found", body.volume_id)))?;

    let protos: String = db.query_row(
        "SELECT access_protocols FROM volumes WHERE id = ?1", rusqlite::params![&body.volume_id],
        |row| row.get(0),
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !protos.contains("iscsi") {
        return Err((StatusCode::BAD_REQUEST, "Volume does not have iSCSI protocol enabled".into()));
    }

    let id = uuid::Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO iscsi_acls (id, volume_id, initiator_iqn, comment) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![&id, &body.volume_id, &body.initiator_iqn, &body.comment],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create ACL: {}", e)))?;

    tracing::info!("iSCSI ACL created: volume={} iqn={}", body.volume_id, body.initiator_iqn);

    Ok((StatusCode::CREATED, Json(AclResponse {
        id, volume_id: body.volume_id, volume_name: vol_name,
        initiator_iqn: body.initiator_iqn, comment: body.comment,
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    })))
}

/// DELETE /api/iscsi/acls/{id}
pub async fn delete_acl(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let deleted = db.execute("DELETE FROM iscsi_acls WHERE id = ?1", rusqlite::params![&id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if deleted == 0 {
        return Err((StatusCode::NOT_FOUND, "ACL not found".into()));
    }

    tracing::info!("iSCSI ACL deleted: id={}", id);
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/iscsi/targets
pub async fn list_targets(
    State(state): State<Arc<CoreSanState>>,
) -> Result<Json<Vec<TargetResponse>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, name, status FROM volumes WHERE access_protocols LIKE '%iscsi%' AND status != 'deleted'"
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let node_name = &state.hostname;

    let targets: Vec<TargetResponse> = stmt.query_map([], |row| {
        let vol_id: String = row.get(0)?;
        let vol_name: String = row.get(1)?;
        let status: String = row.get(2)?;
        Ok(TargetResponse {
            volume_id: vol_id,
            volume_name: vol_name.clone(),
            iqn: format!("iqn.2026-04.io.corevm:{}", vol_name),
            portals: vec![format!("{}:3260", node_name)],
            alua_state: "active_optimized".to_string(),
            status,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Ok(Json(targets))
}
