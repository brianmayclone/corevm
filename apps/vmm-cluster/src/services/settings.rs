//! ClusterSettingsService — key-value settings store for global cluster config.
//! Includes SMTP config, general settings, etc.

use rusqlite::Connection;
use serde::Serialize;

pub struct ClusterSettingsService;

#[derive(Debug, Serialize)]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
    pub category: String,
}

impl ClusterSettingsService {
    /// Get a single setting value.
    pub fn get(db: &Connection, key: &str) -> Option<String> {
        db.query_row("SELECT value FROM cluster_settings WHERE key = ?1",
            rusqlite::params![key], |r| r.get(0)).ok()
    }

    /// Set a setting (upsert).
    pub fn set(db: &Connection, key: &str, value: &str, category: &str) -> Result<(), String> {
        db.execute(
            "INSERT INTO cluster_settings (key, value, category) VALUES (?1, ?2, ?3) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value, category],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Get all settings in a category.
    pub fn list_category(db: &Connection, category: &str) -> Result<Vec<SettingEntry>, String> {
        let mut stmt = db.prepare("SELECT key, value, category FROM cluster_settings WHERE category = ?1 ORDER BY key")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![category], |row| {
            Ok(SettingEntry { key: row.get(0)?, value: row.get(1)?, category: row.get(2)? })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get all settings as a flat map.
    pub fn all(db: &Connection) -> Result<Vec<SettingEntry>, String> {
        let mut stmt = db.prepare("SELECT key, value, category FROM cluster_settings ORDER BY category, key")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(SettingEntry { key: row.get(0)?, value: row.get(1)?, category: row.get(2)? })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── SMTP convenience methods ────────────────────────────────────

    pub fn get_smtp_config(db: &Connection) -> SmtpConfig {
        SmtpConfig {
            host: Self::get(db, "smtp.host").unwrap_or_default(),
            port: Self::get(db, "smtp.port").and_then(|v| v.parse().ok()).unwrap_or(587),
            username: Self::get(db, "smtp.username").unwrap_or_default(),
            password: Self::get(db, "smtp.password").unwrap_or_default(),
            from_address: Self::get(db, "smtp.from_address").unwrap_or_default(),
            use_tls: Self::get(db, "smtp.use_tls").map(|v| v == "true").unwrap_or(true),
        }
    }

    pub fn set_smtp_config(db: &Connection, config: &SmtpConfig) -> Result<(), String> {
        Self::set(db, "smtp.host", &config.host, "smtp")?;
        Self::set(db, "smtp.port", &config.port.to_string(), "smtp")?;
        Self::set(db, "smtp.username", &config.username, "smtp")?;
        Self::set(db, "smtp.password", &config.password, "smtp")?;
        Self::set(db, "smtp.from_address", &config.from_address, "smtp")?;
        Self::set(db, "smtp.use_tls", &config.use_tls.to_string(), "smtp")?;
        Ok(())
    }
}

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_address: String,
    pub use_tls: bool,
}
