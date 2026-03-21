//! Audit log service — records user actions for the activity feed.

use rusqlite::Connection;
use serde::Serialize;

pub struct AuditService;

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub user_id: Option<i64>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub details: Option<String>,
    pub created_at: String,
}

impl AuditService {
    /// Log an action.
    pub fn log(db: &Connection, user_id: i64, action: &str, target_type: &str, target_id: &str, details: Option<&str>) {
        let _ = db.execute(
            "INSERT INTO audit_log (user_id, action, target_type, target_id, details) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![user_id, action, target_type, target_id, details],
        );
    }

    /// Get recent entries (newest first).
    pub fn recent(db: &Connection, limit: u32) -> Result<Vec<AuditEntry>, String> {
        let mut stmt = db.prepare(
            "SELECT id, user_id, action, target_type, target_id, details, created_at
             FROM audit_log ORDER BY id DESC LIMIT ?1"
        ).map_err(|e| e.to_string())?;
        let entries = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(AuditEntry {
                id: row.get(0)?, user_id: row.get(1)?,
                action: row.get(2)?, target_type: row.get(3)?,
                target_id: row.get(4)?, details: row.get(5)?,
                created_at: row.get(6)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(entries)
    }
}
