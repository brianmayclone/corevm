//! ResourceGroupService — manages resource groups and permissions.

use rusqlite::Connection;

pub struct ResourceGroupService;

impl ResourceGroupService {
    /// List all resource groups with VM counts.
    pub fn list(db: &Connection) -> Result<Vec<serde_json::Value>, String> {
        let mut stmt = db.prepare(
            "SELECT rg.id, rg.name, rg.description, rg.is_default, rg.created_at,
                    (SELECT COUNT(*) FROM vms WHERE resource_group_id = rg.id) as vm_count
             FROM resource_groups rg ORDER BY rg.name"
        ).map_err(|e| e.to_string())?;

        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "name": row.get::<_, String>(1)?,
                "description": row.get::<_, String>(2)?,
                "is_default": row.get::<_, i32>(3)? != 0,
                "created_at": row.get::<_, String>(4)?,
                "vm_count": row.get::<_, i64>(5)?,
                "permissions": []
            }))
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}
