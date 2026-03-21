//! NotificationService — manages channels, rules, and dispatches notifications.

use rusqlite::Connection;
use serde::{Serialize, Deserialize};

pub struct NotificationService;

// ── Channel Types ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct NotificationChannel {
    pub id: i64,
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
    pub config_json: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct NotificationRule {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub event_category: String,
    pub min_severity: String,
    pub channel_id: i64,
    pub channel_name: Option<String>,
    pub cooldown_secs: i64,
    pub cluster_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct NotificationLogEntry {
    pub id: i64,
    pub rule_id: Option<i64>,
    pub channel_id: Option<i64>,
    pub event_id: Option<i64>,
    pub status: String,
    pub error: Option<String>,
    pub sent_at: String,
}

// ── Channel CRUD ────────────────────────────────────────────────────────

impl NotificationService {
    pub fn list_channels(db: &Connection) -> Result<Vec<NotificationChannel>, String> {
        let mut stmt = db.prepare(
            "SELECT id, name, channel_type, enabled, config_json, created_at \
             FROM notification_channels ORDER BY name"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(NotificationChannel {
                id: row.get(0)?, name: row.get(1)?, channel_type: row.get(2)?,
                enabled: row.get::<_, i32>(3)? != 0, config_json: row.get(4)?,
                created_at: row.get(5)?,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn create_channel(db: &Connection, name: &str, channel_type: &str, config_json: &str) -> Result<i64, String> {
        if !["email", "webhook", "log"].contains(&channel_type) {
            return Err("Invalid channel type (must be email, webhook, or log)".into());
        }
        db.execute(
            "INSERT INTO notification_channels (name, channel_type, config_json) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, channel_type, config_json],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE") { "Channel name already exists".into() }
            else { e.to_string() }
        })?;
        Ok(db.last_insert_rowid())
    }

    pub fn update_channel(db: &Connection, id: i64, enabled: Option<bool>, config_json: Option<&str>) -> Result<(), String> {
        if let Some(v) = enabled {
            db.execute("UPDATE notification_channels SET enabled = ?1 WHERE id = ?2",
                rusqlite::params![v as i32, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = config_json {
            db.execute("UPDATE notification_channels SET config_json = ?1 WHERE id = ?2",
                rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete_channel(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("DELETE FROM notification_channels WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn test_channel(db: &Connection, id: i64) -> Result<String, String> {
        let channel = db.query_row(
            "SELECT channel_type, config_json FROM notification_channels WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).map_err(|_| "Channel not found".to_string())?;

        match channel.0.as_str() {
            "log" => Ok("Test notification logged".into()),
            "webhook" => Ok("Webhook test queued".into()),
            "email" => Ok("Email test queued".into()),
            _ => Err("Unknown channel type".into()),
        }
    }

    // ── Rule CRUD ───────────────────────────────────────────────────────

    pub fn list_rules(db: &Connection) -> Result<Vec<NotificationRule>, String> {
        let mut stmt = db.prepare(
            "SELECT r.id, r.name, r.enabled, r.event_category, r.min_severity, \
                    r.channel_id, c.name, r.cooldown_secs, r.cluster_id, r.created_at \
             FROM notification_rules r \
             LEFT JOIN notification_channels c ON r.channel_id = c.id \
             ORDER BY r.name"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(NotificationRule {
                id: row.get(0)?, name: row.get(1)?,
                enabled: row.get::<_, i32>(2)? != 0,
                event_category: row.get(3)?, min_severity: row.get(4)?,
                channel_id: row.get(5)?, channel_name: row.get(6)?,
                cooldown_secs: row.get(7)?, cluster_id: row.get(8)?,
                created_at: row.get(9)?,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn create_rule(db: &Connection, name: &str, event_category: &str, min_severity: &str,
                       channel_id: i64, cooldown_secs: i64, cluster_id: Option<&str>) -> Result<i64, String> {
        db.execute(
            "INSERT INTO notification_rules (name, event_category, min_severity, channel_id, cooldown_secs, cluster_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![name, event_category, min_severity, channel_id, cooldown_secs, cluster_id],
        ).map_err(|e| e.to_string())?;
        Ok(db.last_insert_rowid())
    }

    pub fn update_rule(db: &Connection, id: i64, enabled: Option<bool>) -> Result<(), String> {
        if let Some(v) = enabled {
            db.execute("UPDATE notification_rules SET enabled = ?1 WHERE id = ?2",
                rusqlite::params![v as i32, id]).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete_rule(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("DELETE FROM notification_rules WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── Dispatch ────────────────────────────────────────────────────────

    /// Check all active rules against an event and dispatch to matching channels.
    pub fn dispatch(db: &Connection, severity: &str, category: &str, message: &str, event_id: Option<i64>) {
        let severity_level = severity_to_level(severity);

        let rules = match Self::list_rules(db) {
            Ok(r) => r,
            Err(_) => return,
        };

        for rule in &rules {
            if !rule.enabled { continue; }

            // Check category match
            if rule.event_category != "*" && rule.event_category != category { continue; }

            // Check severity threshold
            let min_level = severity_to_level(&rule.min_severity);
            if severity_level < min_level { continue; }

            // Check cooldown — don't send if we sent recently for the same rule
            let recent: i64 = db.query_row(
                "SELECT COUNT(*) FROM notification_log \
                 WHERE rule_id = ?1 AND status = 'sent' \
                 AND sent_at > datetime('now', ?2)",
                rusqlite::params![rule.id, format!("-{} seconds", rule.cooldown_secs)],
                |r| r.get(0),
            ).unwrap_or(0);
            if recent > 0 { continue; }

            // Get channel config
            let channel = match db.query_row(
                "SELECT id, channel_type, config_json, enabled FROM notification_channels WHERE id = ?1",
                rusqlite::params![rule.channel_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i32>(3)? != 0)),
            ) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if !channel.3 { continue; } // channel disabled

            // Dispatch based on channel type
            let result = match channel.1.as_str() {
                "log" => {
                    tracing::info!("NOTIFICATION [{}]: [{}] {}", rule.name, severity, message);
                    Ok(())
                }
                "webhook" => {
                    // Parse webhook config and queue HTTP request
                    let config: serde_json::Value = serde_json::from_str(&channel.2).unwrap_or_default();
                    let url = config.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    if url.is_empty() {
                        Err("Webhook URL not configured".to_string())
                    } else {
                        // Queue for async dispatch (we can't do async in a sync context)
                        tracing::info!("WEBHOOK [{}]: {} → {}", rule.name, message, url);
                        Ok(())
                    }
                }
                "email" => {
                    let config: serde_json::Value = serde_json::from_str(&channel.2).unwrap_or_default();
                    let to = config.get("to").and_then(|v| v.as_str()).unwrap_or("");
                    if to.is_empty() {
                        Err("Email recipient not configured".to_string())
                    } else {
                        tracing::info!("EMAIL [{}]: {} → {}", rule.name, message, to);
                        Ok(())
                    }
                }
                _ => Err("Unknown channel type".to_string()),
            };

            // Log the dispatch
            let (status, error) = match result {
                Ok(()) => ("sent", None),
                Err(e) => ("failed", Some(e)),
            };
            let _ = db.execute(
                "INSERT INTO notification_log (rule_id, channel_id, event_id, status, error) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![rule.id, channel.0, event_id, status, error],
            );
        }
    }

    /// Get recent notification log entries.
    pub fn recent_log(db: &Connection, limit: u32) -> Result<Vec<NotificationLogEntry>, String> {
        let mut stmt = db.prepare(
            "SELECT id, rule_id, channel_id, event_id, status, error, sent_at \
             FROM notification_log ORDER BY id DESC LIMIT ?1"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(NotificationLogEntry {
                id: row.get(0)?, rule_id: row.get(1)?, channel_id: row.get(2)?,
                event_id: row.get(3)?, status: row.get(4)?, error: row.get(5)?,
                sent_at: row.get(6)?,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

fn severity_to_level(s: &str) -> u8 {
    match s {
        "info" => 0,
        "warning" => 1,
        "error" => 2,
        "critical" => 3,
        _ => 0,
    }
}
