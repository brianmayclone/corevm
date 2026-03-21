//! Authentication service — login with local DB or LDAP fallback.

use rusqlite::Connection;
use crate::auth::jwt;
use crate::services::ldap::LdapService;

#[derive(Debug)]
pub struct AuthenticatedUser {
    pub id: i64,
    pub username: String,
    pub role: String,
}

pub struct AuthService;

impl AuthService {
    /// Authenticate user by username + password.
    /// First tries local DB, then falls back to active LDAP directories.
    pub fn login(
        db: &Connection,
        username: &str,
        password: &str,
        jwt_secret: &str,
        token_hours: u64,
    ) -> Result<(AuthenticatedUser, String), String> {
        // Step 1: Try local DB auth
        if let Ok(result) = Self::local_login(db, username, password, jwt_secret, token_hours) {
            return Ok(result);
        }

        // Step 2: Try LDAP auth (if any active configs exist)
        if let Ok(result) = Self::ldap_login(db, username, password, jwt_secret, token_hours) {
            return Ok(result);
        }

        Err("Invalid credentials".to_string())
    }

    fn local_login(
        db: &Connection, username: &str, password: &str, jwt_secret: &str, token_hours: u64,
    ) -> Result<(AuthenticatedUser, String), String> {
        let (user_id, password_hash, role) = db.query_row(
            "SELECT id, password_hash, role FROM users WHERE username = ?1",
            rusqlite::params![username],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        ).map_err(|_| "User not found locally".to_string())?;

        use argon2::{Argon2, PasswordHash, PasswordVerifier};
        let parsed_hash = PasswordHash::new(&password_hash)
            .map_err(|_| "Internal error".to_string())?;
        Argon2::default().verify_password(password.as_bytes(), &parsed_hash)
            .map_err(|_| "Invalid password".to_string())?;

        let token = jwt::create_access_token(user_id, username, &role, jwt_secret, token_hours)?;
        Ok((AuthenticatedUser { id: user_id, username: username.to_string(), role }, token))
    }

    fn ldap_login(
        db: &Connection, username: &str, password: &str, jwt_secret: &str, token_hours: u64,
    ) -> Result<(AuthenticatedUser, String), String> {
        let configs = LdapService::list(db)?;
        let active: Vec<_> = configs.iter().filter(|c| c.enabled).collect();

        if active.is_empty() {
            return Err("No active LDAP directories".into());
        }

        for config in &active {
            // Build user DN from filter template
            let user_filter = config.user_filter.replace("{username}", username);

            // Attempt LDAP simple bind to validate credentials
            // The user DN is constructed from the filter and base DN
            let user_dn = format!("{}={},{}", config.attr_username, username, config.user_search_dn);

            match Self::ldap_bind(&config.server_url, &user_dn, password) {
                Ok(()) => {
                    // Auth successful — determine role from mapping
                    let role = Self::ldap_resolve_role(config, username);

                    // Create or update local user for this LDAP user
                    let user_id = Self::ensure_local_user(db, username, &role)?;

                    let token = jwt::create_access_token(user_id, username, &role, jwt_secret, token_hours)?;
                    tracing::info!("LDAP auth succeeded for '{}' via '{}' (role: {})", username, config.name, role);
                    return Ok((AuthenticatedUser { id: user_id, username: username.to_string(), role }, token));
                }
                Err(e) => {
                    tracing::debug!("LDAP auth failed for '{}' via '{}': {}", username, config.name, e);
                    continue;
                }
            }
        }

        Err("LDAP authentication failed".into())
    }

    /// Simple LDAP bind via TCP to validate credentials.
    fn ldap_bind(server_url: &str, user_dn: &str, password: &str) -> Result<(), String> {
        // Parse server URL: ldap://host:port or ldaps://host:port
        let url = server_url.trim();
        let (host, port) = if let Some(rest) = url.strip_prefix("ldap://") {
            let parts: Vec<&str> = rest.split(':').collect();
            (parts[0], parts.get(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(389))
        } else if let Some(rest) = url.strip_prefix("ldaps://") {
            let parts: Vec<&str> = rest.split(':').collect();
            (parts[0], parts.get(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(636))
        } else {
            return Err("Invalid LDAP URL".into());
        };

        // Connect and attempt LDAP simple bind
        // This is a minimal LDAP bind implementation (BER encoding)
        let addr = format!("{}:{}", host, port);
        let mut stream = std::net::TcpStream::connect_timeout(
            &addr.parse().map_err(|_| format!("Cannot parse address: {}", addr))?,
            std::time::Duration::from_secs(5),
        ).map_err(|e| format!("Cannot connect to LDAP {}: {}", addr, e))?;

        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();

        // Send LDAP BindRequest (Simple Bind)
        let bind_request = build_ldap_bind_request(user_dn, password);
        use std::io::Write;
        stream.write_all(&bind_request).map_err(|e| format!("LDAP write error: {}", e))?;

        // Read response
        let mut buf = vec![0u8; 256];
        use std::io::Read;
        let n = stream.read(&mut buf).map_err(|e| format!("LDAP read error: {}", e))?;

        if n < 10 {
            return Err("LDAP response too short".into());
        }

        // Parse LDAP BindResponse — check result code
        // Result code is at a fixed offset in the BER structure
        // Success = 0, InvalidCredentials = 49
        let result_code = parse_ldap_bind_result(&buf[..n]);
        if result_code == 0 {
            Ok(())
        } else if result_code == 49 {
            Err("Invalid credentials".into())
        } else {
            Err(format!("LDAP bind failed with result code {}", result_code))
        }
    }

    fn ldap_resolve_role(config: &crate::services::ldap::LdapConfig, _username: &str) -> String {
        // Parse role_mapping JSON: { "AD-Group-Name": "admin", ... }
        let mapping: std::collections::HashMap<String, String> =
            serde_json::from_str(&config.role_mapping).unwrap_or_default();

        // Without actual group lookup (requires full LDAP search), default to "operator"
        // If role_mapping has a "*" key, use that as default
        mapping.get("*").cloned().unwrap_or_else(|| "operator".into())
    }

    /// Create or update a local user synced from LDAP.
    fn ensure_local_user(db: &Connection, username: &str, role: &str) -> Result<i64, String> {
        // Check if user already exists
        if let Ok(id) = db.query_row(
            "SELECT id FROM users WHERE username = ?1",
            rusqlite::params![username],
            |r| r.get::<_, i64>(0),
        ) {
            // Update role if changed
            let _ = db.execute(
                "UPDATE users SET role = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![role, id],
            );
            return Ok(id);
        }

        // Create new user with a random password hash (can't login locally, only via LDAP)
        use argon2::PasswordHasher;
        let salt = argon2::password_hash::SaltString::generate(&mut rand::rngs::OsRng);
        let random_pass = uuid::Uuid::new_v4().to_string();
        let hash = argon2::Argon2::default()
            .hash_password(random_pass.as_bytes(), &salt)
            .map_err(|e| e.to_string())?
            .to_string();

        db.execute(
            "INSERT INTO users (username, password_hash, role) VALUES (?1, ?2, ?3)",
            rusqlite::params![username, &hash, role],
        ).map_err(|e| e.to_string())?;

        Ok(db.last_insert_rowid())
    }
}

/// Build a minimal LDAP BindRequest message (BER encoded).
fn build_ldap_bind_request(dn: &str, password: &str) -> Vec<u8> {
    let dn_bytes = dn.as_bytes();
    let pass_bytes = password.as_bytes();

    // Simple authentication choice (context-specific, tag 0)
    let auth_tag = 0x80;

    // BindRequest = SEQUENCE { version INTEGER(3), name OCTET STRING, authentication CHOICE }
    let mut bind_req = Vec::new();
    // version = 3
    bind_req.extend_from_slice(&[0x02, 0x01, 0x03]);
    // name (DN)
    bind_req.push(0x04);
    ber_length(&mut bind_req, dn_bytes.len());
    bind_req.extend_from_slice(dn_bytes);
    // simple auth
    bind_req.push(auth_tag);
    ber_length(&mut bind_req, pass_bytes.len());
    bind_req.extend_from_slice(pass_bytes);

    // Wrap in APPLICATION 0 (BindRequest)
    let mut app = Vec::new();
    app.push(0x60); // APPLICATION 0, constructed
    ber_length(&mut app, bind_req.len());
    app.extend_from_slice(&bind_req);

    // Wrap in LDAPMessage = SEQUENCE { messageID INTEGER(1), protocolOp }
    let mut msg = Vec::new();
    // messageID = 1
    msg.extend_from_slice(&[0x02, 0x01, 0x01]);
    msg.extend_from_slice(&app);

    // Outer SEQUENCE
    let mut result = Vec::new();
    result.push(0x30); // SEQUENCE
    ber_length(&mut result, msg.len());
    result.extend_from_slice(&msg);

    result
}

fn ber_length(buf: &mut Vec<u8>, len: usize) {
    if len < 128 {
        buf.push(len as u8);
    } else if len < 256 {
        buf.push(0x81);
        buf.push(len as u8);
    } else {
        buf.push(0x82);
        buf.push((len >> 8) as u8);
        buf.push(len as u8);
    }
}

/// Parse LDAP BindResponse to extract the result code.
fn parse_ldap_bind_result(data: &[u8]) -> u8 {
    // Very simplified BER parser — look for the result code in the response
    // Structure: SEQUENCE { messageID, BindResponse(APPLICATION 1) { resultCode ENUM, ... } }
    if data.len() < 10 { return 255; }

    // Skip outer SEQUENCE tag + length
    let mut pos = 0;
    if data[pos] != 0x30 { return 255; }
    pos += 1;
    pos += skip_ber_length(data, pos);

    // Skip messageID (INTEGER)
    if pos >= data.len() || data[pos] != 0x02 { return 255; }
    pos += 1;
    let id_len = data.get(pos).copied().unwrap_or(0) as usize;
    pos += 1 + id_len;

    // BindResponse tag should be 0x61 (APPLICATION 1, constructed)
    if pos >= data.len() || data[pos] != 0x61 { return 255; }
    pos += 1;
    pos += skip_ber_length(data, pos);

    // Result code is an ENUMERATED (tag 0x0A)
    if pos >= data.len() || data[pos] != 0x0A { return 255; }
    pos += 1;
    let rc_len = data.get(pos).copied().unwrap_or(0) as usize;
    pos += 1;
    if rc_len == 1 && pos < data.len() {
        return data[pos];
    }
    255
}

fn skip_ber_length(data: &[u8], pos: usize) -> usize {
    if pos >= data.len() { return 1; }
    let b = data[pos];
    if b < 128 { 1 }
    else if b == 0x81 { 2 }
    else if b == 0x82 { 3 }
    else { 1 }
}
