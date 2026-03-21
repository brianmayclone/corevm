//! Resource group API — CRUD + permission assignment.

use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::middleware::{AuthUser, AppError, require_admin, require_operator};
use crate::services::resource_groups::ResourceGroupService;
use crate::services::audit::AuditService;

/// GET /api/resource-groups
pub async fn list(auth: AuthUser, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let groups = ResourceGroupService::list(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(groups).unwrap()))
}

/// GET /api/resource-groups/:id
pub async fn get(auth: AuthUser, State(state): State<Arc<AppState>>, Path(id): Path<i64>) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let rg = ResourceGroupService::get(&db, id)
        .map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::to_value(rg).unwrap()))
}

#[derive(Deserialize)]
pub struct CreateResourceGroup {
    pub name: String,
    pub description: Option<String>,
}

/// POST /api/resource-groups
pub async fn create(auth: AuthUser, State(state): State<Arc<AppState>>, Json(req): Json<CreateResourceGroup>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    let id = ResourceGroupService::create(&db, &req.name, req.description.as_deref().unwrap_or(""))
        .map_err(|e| AppError(StatusCode::CONFLICT, e))?;
    AuditService::log(&db, auth.id, "resource_group.created", "resource_group", &id.to_string(), Some(&req.name));
    Ok(Json(serde_json::json!({"id": id, "name": req.name})))
}

#[derive(Deserialize)]
pub struct UpdateResourceGroup {
    pub name: String,
    pub description: Option<String>,
}

/// PUT /api/resource-groups/:id
pub async fn update(auth: AuthUser, State(state): State<Arc<AppState>>, Path(id): Path<i64>, Json(req): Json<UpdateResourceGroup>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    ResourceGroupService::update(&db, id, &req.name, req.description.as_deref().unwrap_or(""))
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /api/resource-groups/:id
pub async fn delete(auth: AuthUser, State(state): State<Arc<AppState>>, Path(id): Path<i64>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    ResourceGroupService::delete(&db, id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, auth.id, "resource_group.deleted", "resource_group", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct SetPermissions {
    pub group_id: i64,
    pub permissions: Vec<String>,
}

/// POST /api/resource-groups/:id/permissions
pub async fn set_permissions(auth: AuthUser, State(state): State<Arc<AppState>>, Path(id): Path<i64>, Json(req): Json<SetPermissions>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    ResourceGroupService::set_permissions(&db, id, req.group_id, &req.permissions)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct RemovePermissions {
    pub group_id: i64,
}

/// DELETE /api/resource-groups/:id/permissions
pub async fn remove_permissions(auth: AuthUser, State(state): State<Arc<AppState>>, Path(id): Path<i64>, Json(req): Json<RemovePermissions>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    ResourceGroupService::remove_permissions(&db, id, req.group_id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct AssignVm {
    pub vm_id: String,
}

/// POST /api/resource-groups/:id/assign-vm
pub async fn assign_vm(auth: AuthUser, State(state): State<Arc<AppState>>, Path(id): Path<i64>, Json(req): Json<AssignVm>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let db = state.db.lock().unwrap();
    ResourceGroupService::assign_vm(&db, &req.vm_id, id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// GET /api/resource-groups/permissions-list — returns all available permissions.
pub async fn permissions_list(_auth: AuthUser) -> Json<serde_json::Value> {
    use crate::services::resource_groups::ALL_PERMISSIONS;
    Json(serde_json::json!({
        "permissions": ALL_PERMISSIONS,
        "categories": {
            "Virtual Machines": ["vm.create", "vm.edit", "vm.delete", "vm.start_stop", "vm.console"],
            "Infrastructure": ["network.edit", "storage.edit", "snapshots.manage"],
        }
    }))
}
