//! DRS Exclusion Service — manages VMs/resource groups excluded from DRS.

use rusqlite::Connection;

pub struct DrsExclusionService;

impl DrsExclusionService {
    pub fn list(db: &Connection) -> Result<Vec<serde_json::Value>, String> {
        let mut stmt = db.prepare(
            "SELECT id, cluster_id, exclusion_type, target_id, reason, created_at \
             FROM drs_exclusions ORDER BY created_at DESC"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "cluster_id": row.get::<_, String>(1)?,
                "exclusion_type": row.get::<_, String>(2)?,
                "target_id": row.get::<_, String>(3)?,
                "reason": row.get::<_, String>(4)?,
                "created_at": row.get::<_, String>(5)?,
            }))
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn create(db: &Connection, cluster_id: &str, exclusion_type: &str, target_id: &str, reason: &str) -> Result<i64, String> {
        if !["vm", "resource_group"].contains(&exclusion_type) {
            return Err("exclusion_type must be 'vm' or 'resource_group'".into());
        }
        db.execute(
            "INSERT INTO drs_exclusions (cluster_id, exclusion_type, target_id, reason) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![cluster_id, exclusion_type, target_id, reason],
        ).map_err(|e| e.to_string())?;
        Ok(db.last_insert_rowid())
    }

    pub fn delete(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("DELETE FROM drs_exclusions WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
