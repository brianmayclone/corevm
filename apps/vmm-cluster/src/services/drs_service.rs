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

    // ── DRS Rules ───────────────────────────────────────────────────

    /// List all DRS rules for a cluster (or all if cluster_id is None).
    pub fn list_rules(db: &Connection, cluster_id: Option<&str>) -> Result<Vec<DrsRule>, String> {
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(cid) = cluster_id {
            ("SELECT id, cluster_id, name, enabled, metric, threshold, action, cooldown_secs, priority, created_at \
              FROM drs_rules WHERE cluster_id = ?1 ORDER BY name".into(),
             vec![Box::new(cid.to_string())])
        } else {
            ("SELECT id, cluster_id, name, enabled, metric, threshold, action, cooldown_secs, priority, created_at \
              FROM drs_rules ORDER BY name".into(),
             vec![])
        };
        let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rules = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(DrsRule {
                id: row.get(0)?, cluster_id: row.get(1)?, name: row.get(2)?,
                enabled: row.get::<_, i32>(3)? != 0, metric: row.get(4)?,
                threshold: row.get(5)?, action: row.get(6)?,
                cooldown_secs: row.get(7)?, priority: row.get(8)?,
                created_at: row.get(9)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(rules)
    }

    /// Create a DRS rule.
    pub fn create_rule(db: &Connection, cluster_id: &str, name: &str, metric: &str,
                       threshold: f64, action: &str, cooldown_secs: i64, priority: &str) -> Result<i64, String> {
        db.execute(
            "INSERT INTO drs_rules (cluster_id, name, metric, threshold, action, cooldown_secs, priority) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![cluster_id, name, metric, threshold, action, cooldown_secs, priority],
        ).map_err(|e| e.to_string())?;
        Ok(db.last_insert_rowid())
    }

    /// Update a DRS rule.
    pub fn update_rule(db: &Connection, id: i64, enabled: Option<bool>, threshold: Option<f64>,
                       action: Option<&str>, priority: Option<&str>) -> Result<(), String> {
        if let Some(v) = enabled {
            db.execute("UPDATE drs_rules SET enabled = ?1 WHERE id = ?2",
                rusqlite::params![v as i32, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = threshold {
            db.execute("UPDATE drs_rules SET threshold = ?1 WHERE id = ?2",
                rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = action {
            db.execute("UPDATE drs_rules SET action = ?1 WHERE id = ?2",
                rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = priority {
            db.execute("UPDATE drs_rules SET priority = ?1 WHERE id = ?2",
                rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Delete a DRS rule.
    pub fn delete_rule(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("DELETE FROM drs_rules WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Get active rules for a cluster (used by DRS engine).
    pub fn active_rules(db: &Connection, cluster_id: &str) -> Result<Vec<DrsRule>, String> {
        let all = Self::list_rules(db, Some(cluster_id))?;
        Ok(all.into_iter().filter(|r| r.enabled).collect())
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct DrsRule {
    pub id: i64,
    pub cluster_id: String,
    pub name: String,
    pub enabled: bool,
    pub metric: String,
    pub threshold: f64,
    pub action: String,
    pub cooldown_secs: i64,
    pub priority: String,
    pub created_at: String,
}
