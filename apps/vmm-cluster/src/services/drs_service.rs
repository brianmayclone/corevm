//! DRS Service — manages DRS recommendations and rules.

use rusqlite::Connection;
use serde::Serialize;

pub struct DrsService;

#[derive(Debug, Serialize)]
pub struct DrsRecommendation {
    pub id: i64,
    pub cluster_id: String,
    pub vm_id: String,
    pub vm_name: String,
    pub source_host_id: String,
    pub source_host_name: String,
    pub target_host_id: String,
    pub target_host_name: String,
    pub reason: String,
    pub priority: String,
    pub status: String,
    pub created_at: String,
}

impl DrsService {
    /// List pending DRS recommendations.
    pub fn list_pending(db: &Connection) -> Result<Vec<DrsRecommendation>, String> {
        let mut stmt = db.prepare(
            "SELECT r.id, r.cluster_id, r.vm_id, v.name, r.source_host_id, sh.hostname, \
                    r.target_host_id, th.hostname, r.reason, r.priority, r.status, r.created_at \
             FROM drs_recommendations r \
             JOIN vms v ON r.vm_id = v.id \
             JOIN hosts sh ON r.source_host_id = sh.id \
             JOIN hosts th ON r.target_host_id = th.id \
             WHERE r.status = 'pending' \
             ORDER BY r.created_at DESC"
        ).map_err(|e| e.to_string())?;

        let recs = stmt.query_map([], |row| {
            Ok(DrsRecommendation {
                id: row.get(0)?, cluster_id: row.get(1)?, vm_id: row.get(2)?,
                vm_name: row.get(3)?, source_host_id: row.get(4)?,
                source_host_name: row.get(5)?, target_host_id: row.get(6)?,
                target_host_name: row.get(7)?, reason: row.get(8)?,
                priority: row.get(9)?, status: row.get(10)?, created_at: row.get(11)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(recs)
    }

    /// Get a recommendation's VM and target for applying.
    pub fn get_apply_target(db: &Connection, id: i64) -> Result<(String, String), String> {
        db.query_row(
            "SELECT vm_id, target_host_id FROM drs_recommendations WHERE id = ?1 AND status = 'pending'",
            rusqlite::params![id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).map_err(|_| "Recommendation not found or already applied".to_string())
    }

    /// Mark a recommendation as applied.
    pub fn mark_applied(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("UPDATE drs_recommendations SET status = 'applied' WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Dismiss a recommendation.
    pub fn dismiss(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("UPDATE drs_recommendations SET status = 'dismissed' WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
