//! Alarm service — evaluates alarm conditions and manages alarm CRUD.

use rusqlite::Connection;
use serde::Serialize;

pub struct AlarmService;

#[derive(Debug, Serialize)]
pub struct AlarmInfo {
    pub id: i64,
    pub name: String,
    pub target_type: String,
    pub target_id: String,
    pub condition_type: String,
    pub threshold: Option<f64>,
    pub severity: String,
    pub triggered: bool,
    pub acknowledged: bool,
    pub created_at: String,
    pub triggered_at: Option<String>,
}

impl AlarmService {
    /// List all alarms.
    pub fn list(db: &Connection) -> Result<Vec<AlarmInfo>, String> {
        let mut stmt = db.prepare(
            "SELECT id, name, target_type, target_id, condition_type, threshold, severity, \
                    triggered, acknowledged, created_at, triggered_at \
             FROM alarms ORDER BY triggered DESC, created_at DESC"
        ).map_err(|e| e.to_string())?;

        let alarms = stmt.query_map([], |row| {
            Ok(AlarmInfo {
                id: row.get(0)?, name: row.get(1)?, target_type: row.get(2)?,
                target_id: row.get(3)?, condition_type: row.get(4)?,
                threshold: row.get(5)?, severity: row.get(6)?,
                triggered: row.get::<_, i32>(7)? != 0,
                acknowledged: row.get::<_, i32>(8)? != 0,
                created_at: row.get(9)?, triggered_at: row.get(10)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(alarms)
    }

    /// Acknowledge an alarm.
    pub fn acknowledge(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("UPDATE alarms SET acknowledged = 1 WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Check all alarms and trigger/clear as needed.
    /// Called periodically by the heartbeat engine after state updates.
    pub fn evaluate(db: &Connection) {
        let mut stmt = match db.prepare(
            "SELECT a.id, a.condition_type, a.threshold, a.target_type, a.target_id, a.triggered \
             FROM alarms a WHERE a.acknowledged = 0"
        ) {
            Ok(s) => s,
            Err(_) => return,
        };

        struct AlarmRow { id: i64, condition: String, threshold: Option<f64>, target_type: String, target_id: String, triggered: bool }
        let alarms: Vec<AlarmRow> = match stmt.query_map([], |row| {
            Ok(AlarmRow {
                id: row.get(0)?, condition: row.get(1)?, threshold: row.get(2)?,
                target_type: row.get(3)?, target_id: row.get(4)?,
                triggered: row.get::<_, i32>(5)? != 0,
            })
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(_) => return,
        };

        for alarm in alarms {
            let threshold = match alarm.threshold {
                Some(t) => t,
                None => continue,
            };

            let current_value = match (alarm.condition.as_str(), alarm.target_type.as_str()) {
                ("cpu_usage", "host") => {
                    db.query_row("SELECT cpu_usage_pct FROM hosts WHERE id = ?1",
                        rusqlite::params![&alarm.target_id], |r| r.get::<_, f64>(0)).ok()
                }
                ("ram_usage", "host") => {
                    db.query_row(
                        "SELECT CASE WHEN total_ram_mb > 0 THEN (1.0 - CAST(free_ram_mb AS REAL)/total_ram_mb) * 100 ELSE 0 END \
                         FROM hosts WHERE id = ?1",
                        rusqlite::params![&alarm.target_id], |r| r.get::<_, f64>(0)).ok()
                }
                ("disk_usage", "datastore") => {
                    db.query_row(
                        "SELECT CASE WHEN total_bytes > 0 THEN (1.0 - CAST(free_bytes AS REAL)/total_bytes) * 100 ELSE 0 END \
                         FROM datastores WHERE id = ?1",
                        rusqlite::params![&alarm.target_id], |r| r.get::<_, f64>(0)).ok()
                }
                _ => None,
            };

            if let Some(value) = current_value {
                let should_trigger = value >= threshold;
                if should_trigger && !alarm.triggered {
                    let _ = db.execute(
                        "UPDATE alarms SET triggered = 1, triggered_at = datetime('now') WHERE id = ?1",
                        rusqlite::params![alarm.id],
                    );
                } else if !should_trigger && alarm.triggered {
                    let _ = db.execute(
                        "UPDATE alarms SET triggered = 0, triggered_at = NULL WHERE id = ?1",
                        rusqlite::params![alarm.id],
                    );
                }
            }
        }
    }
}
