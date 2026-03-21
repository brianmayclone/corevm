//! Resource group service — CRUD + permission management.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub struct ResourceGroupService;

#[derive(Debug, Serialize, Clone)]
pub struct ResourceGroup {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub is_default: bool,
    pub vm_count: i64,
    pub created_at: String,
    pub permissions: Vec<GroupPermission>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroupPermission {
    pub id: i64,
    pub group_id: i64,
    pub group_name: String,
    pub permissions: Vec<String>,
}

/// All available granular permissions.
pub const ALL_PERMISSIONS: &[&str] = &[
    "vm.create",
    "vm.edit",
    "vm.delete",
    "vm.start_stop",
    "vm.console",
    "network.edit",
    "storage.edit",
    "snapshots.manage",
];

impl ResourceGroupService {
    pub fn list(db: &Connection) -> Result<Vec<ResourceGroup>, String> {
        let mut stmt = db.prepare(
            "SELECT rg.id, rg.name, rg.description, rg.is_default, rg.created_at, \
             (SELECT COUNT(*) FROM vms WHERE resource_group_id = rg.id) as vm_count \
             FROM resource_groups rg ORDER BY rg.is_default DESC, rg.name"
        ).map_err(|e| e.to_string())?;

        let groups: Vec<(i64, String, String, bool, String, i64)> = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, i64>(3)? != 0, row.get(4)?, row.get(5)?))
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();

        let mut result = Vec::new();
        for (id, name, description, is_default, created_at, vm_count) in groups {
            let permissions = Self::get_permissions(db, id)?;
            result.push(ResourceGroup { id, name, description, is_default, vm_count, created_at, permissions });
        }
        Ok(result)
    }

    pub fn get(db: &Connection, rg_id: i64) -> Result<ResourceGroup, String> {
        let (id, name, description, is_default, created_at, vm_count) = db.query_row(
            "SELECT rg.id, rg.name, rg.description, rg.is_default, rg.created_at, \
             (SELECT COUNT(*) FROM vms WHERE resource_group_id = rg.id) \
             FROM resource_groups rg WHERE rg.id = ?1",
            rusqlite::params![rg_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, i64>(3)? != 0, row.get(4)?, row.get(5)?))
        ).map_err(|_| "Resource group not found".to_string())?;
        let permissions = Self::get_permissions(db, id)?;
        Ok(ResourceGroup { id, name, description, is_default, vm_count, created_at, permissions })
    }

    pub fn create(db: &Connection, name: &str, description: &str) -> Result<i64, String> {
        db.execute(
            "INSERT INTO resource_groups (name, description) VALUES (?1, ?2)",
            rusqlite::params![name, description],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE") { "Resource group name already exists".into() }
            else { e.to_string() }
        })?;
        Ok(db.last_insert_rowid())
    }

    pub fn update(db: &Connection, rg_id: i64, name: &str, description: &str) -> Result<(), String> {
        // Don't allow renaming the default group
        let is_default: bool = db.query_row(
            "SELECT is_default FROM resource_groups WHERE id = ?1",
            rusqlite::params![rg_id], |r| r.get::<_, i64>(0),
        ).map(|v| v != 0).map_err(|_| "Not found".to_string())?;

        if is_default && name != "All Machines" {
            return Err("Cannot rename the default resource group".into());
        }

        db.execute(
            "UPDATE resource_groups SET name = ?1, description = ?2 WHERE id = ?3",
            rusqlite::params![name, description, rg_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete(db: &Connection, rg_id: i64) -> Result<(), String> {
        let is_default: bool = db.query_row(
            "SELECT is_default FROM resource_groups WHERE id = ?1",
            rusqlite::params![rg_id], |r| r.get::<_, i64>(0),
        ).map(|v| v != 0).map_err(|_| "Not found".to_string())?;

        if is_default {
            return Err("Cannot delete the default resource group".into());
        }

        // Move all VMs to default group before deleting
        let default_id: i64 = db.query_row(
            "SELECT id FROM resource_groups WHERE is_default = 1", [], |r| r.get(0),
        ).map_err(|e| e.to_string())?;

        db.execute(
            "UPDATE vms SET resource_group_id = ?1 WHERE resource_group_id = ?2",
            rusqlite::params![default_id, rg_id],
        ).map_err(|e| e.to_string())?;

        db.execute("DELETE FROM resource_groups WHERE id = ?1", rusqlite::params![rg_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Set permissions for a user-group on a resource-group.
    pub fn set_permissions(db: &Connection, rg_id: i64, group_id: i64, permissions: &[String]) -> Result<(), String> {
        // Validate permissions
        for p in permissions {
            if !ALL_PERMISSIONS.contains(&p.as_str()) {
                return Err(format!("Unknown permission: {}", p));
            }
        }
        let perms_str = permissions.join(",");
        db.execute(
            "INSERT INTO resource_group_permissions (resource_group_id, group_id, permissions) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(resource_group_id, group_id) DO UPDATE SET permissions = ?3",
            rusqlite::params![rg_id, group_id, perms_str],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Remove all permissions for a user-group on a resource-group.
    pub fn remove_permissions(db: &Connection, rg_id: i64, group_id: i64) -> Result<(), String> {
        db.execute(
            "DELETE FROM resource_group_permissions WHERE resource_group_id = ?1 AND group_id = ?2",
            rusqlite::params![rg_id, group_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Get all permissions for a resource group.
    fn get_permissions(db: &Connection, rg_id: i64) -> Result<Vec<GroupPermission>, String> {
        let mut stmt = db.prepare(
            "SELECT rgp.id, rgp.group_id, g.name, rgp.permissions \
             FROM resource_group_permissions rgp \
             JOIN groups g ON rgp.group_id = g.id \
             WHERE rgp.resource_group_id = ?1"
        ).map_err(|e| e.to_string())?;

        let perms = stmt.query_map(rusqlite::params![rg_id], |row| {
            let perms_str: String = row.get(3)?;
            Ok(GroupPermission {
                id: row.get(0)?,
                group_id: row.get(1)?,
                group_name: row.get(2)?,
                permissions: perms_str.split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(perms)
    }

    /// Check if a user has a specific permission on a resource group.
    pub fn user_has_permission(db: &Connection, user_id: i64, user_role: &str, rg_id: i64, permission: &str) -> bool {
        // Admins always have full access
        if user_role == "admin" { return true; }

        // Check through group memberships
        let result: Result<i64, _> = db.query_row(
            "SELECT COUNT(*) FROM resource_group_permissions rgp \
             JOIN group_members gm ON rgp.group_id = gm.group_id \
             WHERE rgp.resource_group_id = ?1 AND gm.user_id = ?2 \
             AND (',' || rgp.permissions || ',') LIKE '%,' || ?3 || ',%'",
            rusqlite::params![rg_id, user_id, permission],
            |r| r.get(0),
        );
        result.map(|c| c > 0).unwrap_or(false)
    }

    /// Assign a VM to a resource group.
    pub fn assign_vm(db: &Connection, vm_id: &str, rg_id: i64) -> Result<(), String> {
        db.execute(
            "UPDATE vms SET resource_group_id = ?1 WHERE id = ?2",
            rusqlite::params![rg_id, vm_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }
}
