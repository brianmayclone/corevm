//! Authentication service — login, token creation, password verification.

use rusqlite::Connection;
use crate::auth::jwt;

#[derive(Debug)]
pub struct AuthenticatedUser {
    pub id: i64,
    pub username: String,
    pub role: String,
}

pub struct AuthService;

impl AuthService {
    /// Authenticate user by username + password. Returns user info + JWT token.
    pub fn login(
        db: &Connection,
        username: &str,
        password: &str,
        jwt_secret: &str,
        token_hours: u64,
    ) -> Result<(AuthenticatedUser, String), String> {
        let (user_id, password_hash, role) = db.query_row(
            "SELECT id, password_hash, role FROM users WHERE username = ?1",
            rusqlite::params![username],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        ).map_err(|_| "Invalid credentials".to_string())?;

        use argon2::{Argon2, PasswordHash, PasswordVerifier};
        let parsed_hash = PasswordHash::new(&password_hash)
            .map_err(|_| "Internal error".to_string())?;
        Argon2::default().verify_password(password.as_bytes(), &parsed_hash)
            .map_err(|_| "Invalid credentials".to_string())?;

        let token = jwt::create_access_token(user_id, username, &role, jwt_secret, token_hours)?;

        Ok((AuthenticatedUser { id: user_id, username: username.to_string(), role }, token))
    }
}
