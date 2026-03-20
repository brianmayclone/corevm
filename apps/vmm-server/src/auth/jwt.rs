//! JWT token creation and validation.

use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};
use serde::{Serialize, Deserialize};
use chrono::{Utc, Duration};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// User ID
    pub sub: i64,
    /// Username
    pub username: String,
    /// Role: "admin", "operator", "viewer"
    pub role: String,
    /// Expiration (UTC timestamp)
    pub exp: i64,
    /// Issued at (UTC timestamp)
    pub iat: i64,
}

/// Create an access token (short-lived).
pub fn create_access_token(
    user_id: i64,
    username: &str,
    role: &str,
    secret: &str,
    hours: u64,
) -> Result<String, String> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        role: role.to_string(),
        exp: (now + Duration::hours(hours as i64)).timestamp(),
        iat: now.timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    ).map_err(|e| format!("JWT encode error: {}", e))
}

/// Validate a token and return the claims.
pub fn validate_token(token: &str, secret: &str) -> Result<Claims, String> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    ).map_err(|e| format!("JWT validation error: {}", e))?;
    Ok(data.claims)
}
