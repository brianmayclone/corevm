//! Event logging service — cluster-wide event log (like vSphere Events).

use rusqlite::Connection;
use serde::Serialize;

pub struct EventService;

#[derive(Debug, Serialize)]
pub struct Event {
    pub id: i64,
    pub severity: String,
    pub category: String,
    pub message: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub host_id: Option<String>,
    pub created_at: String,
}

impl EventService {
    /// Log a cluster event and dispatch notifications to matching channels.
    pub fn log(
        db: &Connection,
        severity: &str,
        category: &str,
        message: &str,
        target_type: Option<&str>,
        target_id: Option<&str>,
        host_id: Option<&str>,
    ) {
        let _ = db.execute(
            "INSERT INTO events (severity, category, message, target_type, target_id, host_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![severity, category, message, target_type, target_id, host_id],
        );

        // Get the inserted event ID for notification log
        let event_id = db.last_insert_rowid();

        // Dispatch to notification channels
        crate::services::notification::NotificationService::dispatch(db, severity, category, message, Some(event_id));
    }

    /// Get recent events (newest first), optionally filtered by category.
    pub fn recent(db: &Connection, limit: u32, category: Option<&str>) -> Result<Vec<Event>, String> {
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(cat) = category {
            (
                "SELECT id, severity, category, message, target_type, target_id, host_id, created_at \
                 FROM events WHERE category = ?1 ORDER BY id DESC LIMIT ?2".into(),
                vec![Box::new(cat.to_string()), Box::new(limit)],
            )
        } else {
            (
                "SELECT id, severity, category, message, target_type, target_id, host_id, created_at \
                 FROM events ORDER BY id DESC LIMIT ?1".into(),
                vec![Box::new(limit)],
            )
        };

        let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let events = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(Event {
                id: row.get(0)?, severity: row.get(1)?,
                category: row.get(2)?, message: row.get(3)?,
                target_type: row.get(4)?, target_id: row.get(5)?,
                host_id: row.get(6)?, created_at: row.get(7)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(events)
    }
}
