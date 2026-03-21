//! User management service — CRUD operations on the users table.

use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub created_at: String,
}

pub struct UserService;

impl UserService {
    pub fn list(db: &Connection) -> Result<Vec<User>, String> {
        let mut stmt = db.prepare("SELECT id, username, role, created_at FROM users ORDER BY id")
            .map_err(|e| e.to_string())?;
        let users = stmt.query_map([], |row| {
            Ok(User {
                id: row.get(0)?, username: row.get(1)?,
                role: row.get(2)?, created_at: row.get(3)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(users)
    }

    pub fn create(db: &Connection, username: &str, password: &str, role: &str) -> Result<i64, String> {
        if !["admin", "operator", "viewer"].contains(&role) {
            return Err("Invalid role".into());
        }
        let hash = Self::hash_password(password)?;
        db.execute(
            "INSERT INTO users (username, password_hash, role) VALUES (?1, ?2, ?3)",
            rusqlite::params![username, &hash, role],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE") { "Username already exists".into() }
            else { e.to_string() }
        })?;
        Ok(db.last_insert_rowid())
    }

    pub fn update(db: &Connection, user_id: i64, username: Option<&str>, role: Option<&str>) -> Result<(), String> {
        if let Some(name) = username {
            db.execute("UPDATE users SET username = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![name, user_id]).map_err(|e| e.to_string())?;
        }
        if let Some(r) = role {
            if !["admin", "operator", "viewer"].contains(&r) {
                return Err("Invalid role".into());
            }
            db.execute("UPDATE users SET role = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![r, user_id]).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete(db: &Connection, user_id: i64) -> Result<(), String> {
        let affected = db.execute("DELETE FROM users WHERE id = ?1", rusqlite::params![user_id])
            .map_err(|e| e.to_string())?;
        if affected == 0 { Err("User not found".into()) } else { Ok(()) }
    }

    pub fn change_password(db: &Connection, user_id: i64, new_password: &str) -> Result<(), String> {
        let hash = Self::hash_password(new_password)?;
        let affected = db.execute(
            "UPDATE users SET password_hash = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![&hash, user_id],
        ).map_err(|e| e.to_string())?;
        if affected == 0 { Err("User not found".into()) } else { Ok(()) }
    }

    fn hash_password(password: &str) -> Result<String, String> {
        use argon2::PasswordHasher;
        let salt = argon2::password_hash::SaltString::generate(&mut rand::rngs::OsRng);
        argon2::Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| format!("Password hash failed: {}", e))
    }
}
