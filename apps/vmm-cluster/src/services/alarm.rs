//! Alarm service — evaluates alarm conditions against current state.

use rusqlite::Connection;

pub struct AlarmService;

impl AlarmService {
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

        let alarms: Vec<(i64, String, Option<f64>, String, String, bool)> = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get::<_, i32>(5)? != 0))
        }).unwrap_or_else(|_| Box::new(std::iter::empty()))
        .filter_map(|r| r.ok())
        .collect();

        for (id, condition, threshold, target_type, target_id, was_triggered) in alarms {
            let threshold = match threshold {
                Some(t) => t,
                None => continue,
            };

            let current_value = match (condition.as_str(), target_type.as_str()) {
                ("cpu_usage", "host") => {
                    db.query_row("SELECT cpu_usage_pct FROM hosts WHERE id = ?1",
                        rusqlite::params![&target_id], |r| r.get::<_, f64>(0)).ok()
                }
                ("ram_usage", "host") => {
                    db.query_row(
                        "SELECT CASE WHEN total_ram_mb > 0 THEN (1.0 - CAST(free_ram_mb AS REAL)/total_ram_mb) * 100 ELSE 0 END \
                         FROM hosts WHERE id = ?1",
                        rusqlite::params![&target_id], |r| r.get::<_, f64>(0)).ok()
                }
                ("disk_usage", "datastore") => {
                    db.query_row(
                        "SELECT CASE WHEN total_bytes > 0 THEN (1.0 - CAST(free_bytes AS REAL)/total_bytes) * 100 ELSE 0 END \
                         FROM datastores WHERE id = ?1",
                        rusqlite::params![&target_id], |r| r.get::<_, f64>(0)).ok()
                }
                _ => None,
            };

            if let Some(value) = current_value {
                let should_trigger = value >= threshold;
                if should_trigger && !was_triggered {
                    let _ = db.execute(
                        "UPDATE alarms SET triggered = 1, triggered_at = datetime('now') WHERE id = ?1",
                        rusqlite::params![id],
                    );
                } else if !should_trigger && was_triggered {
                    let _ = db.execute(
                        "UPDATE alarms SET triggered = 0, triggered_at = NULL WHERE id = ?1",
                        rusqlite::params![id],
                    );
                }
            }
        }
    }
}
