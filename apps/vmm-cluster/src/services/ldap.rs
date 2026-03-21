//! LdapService — Active Directory / LDAP integration for cluster auth.

use rusqlite::Connection;
use serde::Serialize;

pub struct LdapService;

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
pub struct LdapConfig {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub server_url: String,
    pub bind_dn: String,
    #[serde(skip_serializing)]
    pub bind_password: String,
    pub base_dn: String,
    pub user_search_dn: String,
    pub user_filter: String,
    pub group_search_dn: String,
    pub group_filter: String,
    pub attr_username: String,
    pub attr_email: String,
    pub attr_display: String,
    pub role_mapping: String,
    pub use_tls: bool,
    pub skip_tls_verify: bool,
    pub created_at: String,
}

impl LdapService {
    pub fn list(db: &Connection) -> Result<Vec<LdapConfig>, String> {
        let mut stmt = db.prepare(
            "SELECT id, name, enabled, server_url, bind_dn, bind_password, base_dn, \
                    user_search_dn, user_filter, group_search_dn, group_filter, \
                    attr_username, attr_email, attr_display, role_mapping, \
                    use_tls, skip_tls_verify, created_at \
             FROM ldap_configs ORDER BY name"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(LdapConfig {
                id: row.get(0)?, name: row.get(1)?, enabled: row.get::<_, i32>(2)? != 0,
                server_url: row.get(3)?, bind_dn: row.get(4)?, bind_password: row.get(5)?,
                base_dn: row.get(6)?, user_search_dn: row.get(7)?, user_filter: row.get(8)?,
                group_search_dn: row.get(9)?, group_filter: row.get(10)?,
                attr_username: row.get(11)?, attr_email: row.get(12)?, attr_display: row.get(13)?,
                role_mapping: row.get(14)?,
                use_tls: row.get::<_, i32>(15)? != 0, skip_tls_verify: row.get::<_, i32>(16)? != 0,
                created_at: row.get(17)?,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn create(db: &Connection, name: &str, server_url: &str, base_dn: &str) -> Result<i64, String> {
        db.execute(
            "INSERT INTO ldap_configs (name, server_url, base_dn) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, server_url, base_dn],
        ).map_err(|e| e.to_string())?;
        Ok(db.last_insert_rowid())
    }

    pub fn update(db: &Connection, id: i64, updates: &serde_json::Value) -> Result<(), String> {
        let str_fields = [
            "name", "server_url", "bind_dn", "bind_password", "base_dn",
            "user_search_dn", "user_filter", "group_search_dn", "group_filter",
            "attr_username", "attr_email", "attr_display", "role_mapping",
        ];
        for field in &str_fields {
            if let Some(val) = updates.get(field).and_then(|v| v.as_str()) {
                db.execute(&format!("UPDATE ldap_configs SET {} = ?1 WHERE id = ?2", field),
                    rusqlite::params![val, id]).map_err(|e| e.to_string())?;
            }
        }
        for field in &["enabled", "use_tls", "skip_tls_verify"] {
            if let Some(val) = updates.get(field).and_then(|v| v.as_bool()) {
                db.execute(&format!("UPDATE ldap_configs SET {} = ?1 WHERE id = ?2", field),
                    rusqlite::params![val as i32, id]).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub fn delete(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("DELETE FROM ldap_configs WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn test_connection(db: &Connection, id: i64) -> Result<String, String> {
        let config = db.query_row(
            "SELECT server_url FROM ldap_configs WHERE id = ?1",
            rusqlite::params![id], |r| r.get::<_, String>(0),
        ).map_err(|_| "LDAP config not found".to_string())?;
        // TODO: actual LDAP connection test with ldap3 crate
        Ok(format!("Connection test to {} queued", config))
    }
}
