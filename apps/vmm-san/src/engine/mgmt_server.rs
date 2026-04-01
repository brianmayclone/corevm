//! Management protocol server — handles credential management, volume resolution,
//! and SigV4 auth validation via Unix Domain Socket.
//!
//! Single socket at /run/vmm-san/mgmt.sock. The S3 gateway connects here for
//! auth validation and volume lookups.

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use vmm_core::san_mgmt::*;
use crate::state::CoreSanState;
use sha2::{Sha256, Digest};

/// Spawn the management UDS listener.
pub fn spawn(state: Arc<CoreSanState>) {
    std::fs::create_dir_all("/run/vmm-san").ok();

    let sock_path = MGMT_SOCKET_PATH;

    // Remove stale socket
    std::fs::remove_file(sock_path).ok();

    tokio::spawn(async move {
        let listener = match UnixListener::bind(sock_path) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Mgmt server: cannot bind {}: {}", sock_path, e);
                return;
            }
        };

        // Make socket world-accessible
        std::fs::set_permissions(sock_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o666)).ok();

        tracing::info!("Mgmt server: listening on {}", sock_path);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let state = state.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(state, stream).await {
                            tracing::warn!("Mgmt server: connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Mgmt server: accept error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    });
}

use std::os::unix::fs::PermissionsExt;

async fn handle_connection(
    state: Arc<CoreSanState>,
    mut stream: UnixStream,
) -> Result<(), String> {
    let mut hdr_buf = [0u8; MgmtRequestHeader::SIZE];

    loop {
        if stream.read_exact(&mut hdr_buf).await.is_err() {
            break; // Connection closed
        }

        let hdr = MgmtRequestHeader::from_bytes(&hdr_buf);
        if hdr.magic != MGMT_REQUEST_MAGIC {
            send_response(&mut stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            break;
        }

        let cmd = match MgmtCommand::from_u32(hdr.cmd) {
            Some(c) => c,
            None => {
                drain(&mut stream, hdr.key_len as u64 + hdr.body_len).await;
                send_response(&mut stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
                continue;
            }
        };

        // Read key
        let mut key_buf = vec![0u8; hdr.key_len as usize];
        if hdr.key_len > 0 {
            if stream.read_exact(&mut key_buf).await.is_err() {
                break;
            }
        }
        let key = String::from_utf8_lossy(&key_buf).to_string();

        // Read body
        let mut body = vec![0u8; hdr.body_len as usize];
        if hdr.body_len > 0 {
            if stream.read_exact(&mut body).await.is_err() {
                break;
            }
        }

        match cmd {
            MgmtCommand::ListVolumes => handle_list_volumes(&state, &mut stream).await,
            MgmtCommand::ResolveVolume => handle_resolve_volume(&state, &key, &mut stream).await,
            MgmtCommand::CreateCredential => handle_create_credential(&state, &body, &mut stream).await,
            MgmtCommand::ValidateCredential => handle_validate_credential(&state, &body, &mut stream).await,
            MgmtCommand::ListCredentials => handle_list_credentials(&state, &mut stream).await,
            MgmtCommand::DeleteCredential => handle_delete_credential(&state, &key, &mut stream).await,
            MgmtCommand::CreateVolume | MgmtCommand::DeleteVolume => {
                // Not handled via mgmt socket — use REST API
                send_response(&mut stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            }
            MgmtCommand::ListIscsiVolumes => handle_list_iscsi_volumes(&state, &mut stream).await,
            MgmtCommand::ListIscsiAcls => handle_list_iscsi_acls(&state, &key, &mut stream).await,
            MgmtCommand::CreateIscsiAcl => handle_create_iscsi_acl(&state, &body, &mut stream).await,
            MgmtCommand::DeleteIscsiAcl => handle_delete_iscsi_acl(&state, &key, &mut stream).await,
            MgmtCommand::GetAluaState => handle_get_alua_state(&state, &key, &mut stream).await,
            MgmtCommand::GetTargetPortGroups => handle_get_target_port_groups(&state, &key, &mut stream).await,
        }
    }

    Ok(())
}

async fn drain(stream: &mut UnixStream, n: u64) {
    if n == 0 { return; }
    let mut remaining = n;
    let mut buf = vec![0u8; 8192];
    while remaining > 0 {
        let to_read = remaining.min(buf.len() as u64) as usize;
        if stream.read_exact(&mut buf[..to_read]).await.is_err() {
            break;
        }
        remaining -= to_read as u64;
    }
}

async fn send_response(stream: &mut UnixStream, header: MgmtResponseHeader, metadata: &[u8], body: &[u8]) {
    let _ = stream.write_all(&header.to_bytes()).await;
    if !metadata.is_empty() {
        let _ = stream.write_all(metadata).await;
    }
    if !body.is_empty() {
        let _ = stream.write_all(body).await;
    }
}

// ── ListVolumes ─────────────────────────────────────────────────

async fn handle_list_volumes(
    state: &CoreSanState,
    stream: &mut UnixStream,
) {
    let volumes: Vec<serde_json::Value> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name, status, access_protocols, max_size_bytes FROM volumes
             WHERE status = 'online' AND access_protocols LIKE '%s3%'"
        ).unwrap();
        stmt.query_map([], |row| {
            let protocols_json: String = row.get::<_, String>(3).unwrap_or_else(|_| "[\"fuse\"]".into());
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "access_protocols": serde_json::from_str::<Vec<String>>(&protocols_json).unwrap_or_default(),
                "max_size_bytes": row.get::<_, u64>(4)?,
            }))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    let body = serde_json::to_string(&volumes).unwrap_or_else(|_| "[]".into());
    let body_bytes = body.as_bytes();
    send_response(stream, MgmtResponseHeader::ok(body_bytes.len() as u64, 0), &[], body_bytes).await;
}

// ── ResolveVolume ───────────────────────────────────────────────

async fn handle_resolve_volume(
    state: &CoreSanState,
    volume_name: &str,
    stream: &mut UnixStream,
) {
    let vol_info = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT id, name, status FROM volumes WHERE name = ?1",
            rusqlite::params![volume_name],
            |row| Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
            })),
        ).ok()
    };

    match vol_info {
        Some(info) => {
            let meta = info.to_string();
            let meta_bytes = meta.as_bytes();
            send_response(stream, MgmtResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
        }
        None => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::NotFound), &[], &[]).await;
        }
    }
}

// ── CreateCredential ────────────────────────────────────────────

async fn handle_create_credential(
    state: &CoreSanState,
    body: &[u8],
    stream: &mut UnixStream,
) {
    #[derive(serde::Deserialize)]
    struct CreateCred {
        user_id: String,
        #[serde(default)]
        display_name: String,
    }

    let params: CreateCred = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            return;
        }
    };

    let cred_id = uuid::Uuid::new_v4().to_string();
    let access_key = generate_access_key();
    let secret_key = generate_secret_key();

    // Encrypt secret key with XOR using node_id hash
    let encrypted = xor_encrypt(&secret_key, &state.node_id);
    let encrypted_b64 = base64_encode(&encrypted);

    let db_result = {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO s3_credentials (id, access_key, secret_key_enc, user_id, display_name, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active')",
            rusqlite::params![&cred_id, &access_key, &encrypted_b64, &params.user_id, &params.display_name],
        )
    };

    if let Err(e) = db_result {
        tracing::error!("Mgmt server: create credential failed: {}", e);
        send_response(stream, MgmtResponseHeader::err(MgmtStatus::InternalError), &[], &[]).await;
        return;
    }

    let resp = serde_json::json!({
        "id": cred_id,
        "access_key": access_key,
        "secret_key": secret_key,
        "user_id": params.user_id,
        "display_name": params.display_name,
    });
    let body_str = resp.to_string();
    let body_bytes = body_str.as_bytes();
    send_response(stream, MgmtResponseHeader::ok(body_bytes.len() as u64, 0), &[], body_bytes).await;
}

// ── ValidateCredential ──────────────────────────────────────────

async fn handle_validate_credential(
    state: &CoreSanState,
    body: &[u8],
    stream: &mut UnixStream,
) {
    #[derive(serde::Deserialize)]
    struct ValidateCred {
        access_key: String,
        string_to_sign: String,
        signature: String,
        region: String,
        date: String,   // YYYYMMDD
    }

    let params: ValidateCred = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            return;
        }
    };

    // Look up credential
    let cred_info = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT secret_key_enc, user_id FROM s3_credentials WHERE access_key = ?1 AND status = 'active'",
            rusqlite::params![&params.access_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok()
    };

    let (encrypted_b64, _user_id) = match cred_info {
        Some(info) => info,
        None => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::AccessDenied), &[], &[]).await;
            return;
        }
    };

    // Decrypt secret key
    let encrypted = base64_decode(&encrypted_b64);
    let secret_key = xor_decrypt(&encrypted, &state.node_id);

    // AWS SigV4 signature computation
    // signing_key = HMAC-SHA256(HMAC-SHA256(HMAC-SHA256(HMAC-SHA256("AWS4" + secret_key, date), region), "s3"), "aws4_request")
    let date_key = hmac_sha256(format!("AWS4{}", secret_key).as_bytes(), params.date.as_bytes());
    let region_key = hmac_sha256(&date_key, params.region.as_bytes());
    let service_key = hmac_sha256(&region_key, b"s3");
    let signing_key = hmac_sha256(&service_key, b"aws4_request");

    let expected_sig = hex_encode(&hmac_sha256(&signing_key, params.string_to_sign.as_bytes()));

    if expected_sig == params.signature {
        send_response(stream, MgmtResponseHeader::ok(0, 0), &[], &[]).await;
    } else {
        send_response(stream, MgmtResponseHeader::err(MgmtStatus::AccessDenied), &[], &[]).await;
    }
}

// ── ListCredentials ─────────────────────────────────────────────

async fn handle_list_credentials(
    state: &CoreSanState,
    stream: &mut UnixStream,
) {
    let creds: Vec<serde_json::Value> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, access_key, user_id, display_name, status, created_at FROM s3_credentials ORDER BY created_at"
        ).unwrap();
        stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "access_key": row.get::<_, String>(1)?,
                "user_id": row.get::<_, String>(2)?,
                "display_name": row.get::<_, String>(3)?,
                "status": row.get::<_, String>(4)?,
                "created_at": row.get::<_, String>(5)?,
            }))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    let body = serde_json::to_string(&creds).unwrap_or_else(|_| "[]".into());
    let body_bytes = body.as_bytes();
    send_response(stream, MgmtResponseHeader::ok(body_bytes.len() as u64, 0), &[], body_bytes).await;
}

// ── DeleteCredential ────────────────────────────────────────────

async fn handle_delete_credential(
    state: &CoreSanState,
    credential_id: &str,
    stream: &mut UnixStream,
) {
    let deleted = {
        let db = state.db.lock().unwrap();
        db.execute(
            "DELETE FROM s3_credentials WHERE id = ?1",
            rusqlite::params![credential_id],
        ).unwrap_or(0)
    };

    if deleted > 0 {
        send_response(stream, MgmtResponseHeader::ok(0, 0), &[], &[]).await;
    } else {
        send_response(stream, MgmtResponseHeader::err(MgmtStatus::NotFound), &[], &[]).await;
    }
}

// ── ListIscsiVolumes ────────────────────────────────────────────

async fn handle_list_iscsi_volumes(
    state: &CoreSanState,
    stream: &mut UnixStream,
) {
    let volumes: Vec<serde_json::Value> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name, status, max_size_bytes, access_protocols FROM volumes \
             WHERE access_protocols LIKE '%iscsi%' AND status != 'deleted'"
        ).unwrap();
        stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "max_size_bytes": row.get::<_, u64>(3)?,
                "access_protocols": row.get::<_, String>(4)?,
            }))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    let body = serde_json::to_vec(&volumes).unwrap_or_default();
    send_response(stream, MgmtResponseHeader::ok(body.len() as u64, 0), &[], &body).await;
}

// ── ListIscsiAcls ───────────────────────────────────────────────

async fn handle_list_iscsi_acls(
    state: &CoreSanState,
    volume_id: &str,
    stream: &mut UnixStream,
) {
    let acls: Vec<serde_json::Value> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, volume_id, initiator_iqn, comment, created_at FROM iscsi_acls WHERE volume_id = ?1"
        ).unwrap();
        stmt.query_map(rusqlite::params![volume_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "volume_id": row.get::<_, String>(1)?,
                "initiator_iqn": row.get::<_, String>(2)?,
                "comment": row.get::<_, String>(3)?,
                "created_at": row.get::<_, String>(4)?,
            }))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    let body = serde_json::to_vec(&acls).unwrap_or_default();
    send_response(stream, MgmtResponseHeader::ok(body.len() as u64, 0), &[], &body).await;
}

// ── CreateIscsiAcl ──────────────────────────────────────────────

async fn handle_create_iscsi_acl(
    state: &CoreSanState,
    body: &[u8],
    stream: &mut UnixStream,
) {
    let body_json: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            return;
        }
    };
    let volume_id = body_json["volume_id"].as_str().unwrap_or("");
    let iqn = body_json["initiator_iqn"].as_str().unwrap_or("");
    let comment = body_json["comment"].as_str().unwrap_or("");
    let id = uuid::Uuid::new_v4().to_string();

    let db_result = {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO iscsi_acls (id, volume_id, initiator_iqn, comment) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![&id, volume_id, iqn, comment],
        )
    };

    match db_result {
        Ok(_) => {
            let resp = serde_json::json!({"id": id}).to_string();
            let resp_bytes = resp.as_bytes().to_vec();
            send_response(stream, MgmtResponseHeader::ok(resp_bytes.len() as u64, 0), &[], &resp_bytes).await;
        }
        Err(_) => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::AlreadyExists), &[], &[]).await;
        }
    }
}

// ── DeleteIscsiAcl ──────────────────────────────────────────────

async fn handle_delete_iscsi_acl(
    state: &CoreSanState,
    acl_id: &str,
    stream: &mut UnixStream,
) {
    let deleted = {
        let db = state.db.lock().unwrap();
        db.execute("DELETE FROM iscsi_acls WHERE id = ?1", rusqlite::params![acl_id]).unwrap_or(0)
    };

    if deleted > 0 {
        send_response(stream, MgmtResponseHeader::ok(0, 0), &[], &[]).await;
    } else {
        send_response(stream, MgmtResponseHeader::err(MgmtStatus::NotFound), &[], &[]).await;
    }
}

// ── GetAluaState ────────────────────────────────────────────────

async fn handle_get_alua_state(
    state: &CoreSanState,
    _volume_id: &str,
    stream: &mut UnixStream,
) {
    // For now, always report active_optimized (leader tracking TBD)
    let body = serde_json::json!({"state": "active_optimized", "tpg_id": &state.node_id}).to_string();
    let body_bytes = body.as_bytes().to_vec();
    send_response(stream, MgmtResponseHeader::ok(body_bytes.len() as u64, 0), &[], &body_bytes).await;
}

// ── GetTargetPortGroups ─────────────────────────────────────────

async fn handle_get_target_port_groups(
    state: &CoreSanState,
    _volume_id: &str,
    stream: &mut UnixStream,
) {
    // Report this node as a single TPG for now
    let tpgs = serde_json::json!([
        {"tpg_id": &state.node_id, "state": "active_optimized"}
    ]);
    let body = serde_json::to_vec(&tpgs).unwrap_or_default();
    send_response(stream, MgmtResponseHeader::ok(body.len() as u64, 0), &[], &body).await;
}

// ── Crypto Helpers ──────────────────────────────────────────────

/// Generate a 20-character access key (alphanumeric uppercase).
fn generate_access_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = (0..20)
        .map(|_| {
            let idx = rng.gen_range(0..36);
            if idx < 10 { (b'0' + idx) as char } else { (b'A' + idx - 10) as char }
        })
        .collect();
    chars.into_iter().collect()
}

/// Generate a 40-character secret key (alphanumeric mixed case + digits).
fn generate_secret_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let charset = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    (0..40)
        .map(|_| charset[rng.gen_range(0..charset.len())] as char)
        .collect()
}

/// XOR-encrypt data using a key derived from the node_id hash.
fn xor_encrypt(data: &str, node_id: &str) -> Vec<u8> {
    let key_hash = Sha256::digest(node_id.as_bytes());
    data.as_bytes()
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ key_hash[i % key_hash.len()])
        .collect()
}

/// XOR-decrypt data using a key derived from the node_id hash (same as encrypt).
fn xor_decrypt(data: &[u8], node_id: &str) -> String {
    let key_hash = Sha256::digest(node_id.as_bytes());
    let decrypted: Vec<u8> = data
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ key_hash[i % key_hash.len()])
        .collect();
    String::from_utf8_lossy(&decrypted).to_string()
}

/// HMAC-SHA256.
fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    // HMAC: H((K ^ opad) || H((K ^ ipad) || message))
    let block_size = 64;
    let mut k = key.to_vec();
    if k.len() > block_size {
        k = Sha256::digest(&k).to_vec();
    }
    while k.len() < block_size {
        k.push(0);
    }

    let mut ipad = vec![0x36u8; block_size];
    let mut opad = vec![0x5cu8; block_size];
    for i in 0..block_size {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    // Inner hash
    let mut inner = ipad;
    inner.extend_from_slice(data);
    let inner_hash = Sha256::digest(&inner);

    // Outer hash
    let mut outer = opad;
    outer.extend_from_slice(&inner_hash);
    Sha256::digest(&outer).to_vec()
}

/// Hex-encode bytes.
fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Base64 encode bytes. Public because the S3 credential REST API (Task 15) uses it.
pub fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let chunks = data.chunks(3);
    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let n = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((n >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((n >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARS[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Base64 decode string. Public because the S3 credential REST API (Task 15) uses it.
pub fn base64_decode(input: &str) -> Vec<u8> {
    fn char_val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    }

    let bytes: Vec<u8> = input.bytes().filter(|&b| b != b'=').collect();
    let mut result = Vec::new();

    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 { break; }
        let b0 = char_val(chunk[0]) as u32;
        let b1 = char_val(chunk[1]) as u32;
        let b2 = if chunk.len() > 2 { char_val(chunk[2]) as u32 } else { 0 };
        let b3 = if chunk.len() > 3 { char_val(chunk[3]) as u32 } else { 0 };

        let n = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;

        result.push(((n >> 16) & 0xFF) as u8);
        if chunk.len() > 2 {
            result.push(((n >> 8) & 0xFF) as u8);
        }
        if chunk.len() > 3 {
            result.push((n & 0xFF) as u8);
        }
    }
    result
}
