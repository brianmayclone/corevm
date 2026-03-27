//! Authentication helpers — login, token decode, status display.

use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub user: UserInfo,
}

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub role: String,
}

/// Decode JWT payload (without verification — server already verified it).
pub fn decode_jwt_payload(token: &str) -> Option<JwtClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 { return None; }

    use base64::Engine;
    let decoder = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let payload = decoder.decode(parts[1]).ok()?;
    serde_json::from_slice(&payload).ok()
}

#[derive(Debug, Deserialize)]
pub struct JwtClaims {
    pub sub: i64,
    pub username: String,
    pub role: String,
    pub exp: i64,
    pub iat: i64,
}
