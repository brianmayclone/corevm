//! GroupService — user groups CRUD (Settings → Groups & Roles).

use rusqlite::Connection;

pub struct GroupService;

impl GroupService {
    pub fn list(db: &Connection) -> Result<Vec<serde_json::Value>, String> {
        let mut stmt = db.prepare(
            "SELECT g.id, g.name, g.role, g.description, \
                    (SELECT COUNT(*) FROM group_members gm WHERE gm.group_id = g.id) \
             FROM groups g ORDER BY g.name"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "name": row.get::<_, String>(1)?,
                "role": row.get::<_, String>(2)?,
                "description": row.get::<_, String>(3)?,
                "member_count": row.get::<_, i64>(4)?,
            }))
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn create(db: &Connection, name: &str, role: &str, description: &str) -> Result<i64, String> {
        if name.trim().is_empty() { return Err("Name required".into()); }
        db.execute("INSERT INTO groups (name, role, description) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, role, description])
            .map_err(|e| e.to_string())?;
        Ok(db.last_insert_rowid())
    }

    pub fn delete(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("DELETE FROM groups WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
