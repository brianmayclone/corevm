//! Object storage I/O server — serves S3 object operations via Unix Domain Socket.
//!
//! One UDS listener per S3-enabled volume. The S3 gateway (vmm-s3gw) connects
//! to these sockets for low-latency object CRUD without going through HTTP.
//! Uses the same chunk system as FUSE and disk_server.

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use vmm_core::san_object::*;
use crate::state::CoreSanState;
use sha2::{Sha256, Digest};

/// Spawn UDS listeners for all volumes with "s3" in access_protocols.
pub fn spawn_all(state: Arc<CoreSanState>) {
    let volumes: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name FROM volumes WHERE status = 'online' AND access_protocols LIKE '%s3%'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    // Ensure socket directory exists
    std::fs::create_dir_all("/run/vmm-san").ok();

    for (vol_id, vol_name) in volumes {
        spawn_volume_listener(state.clone(), vol_id, vol_name);
    }
}

/// Spawn a single UDS listener for a volume's object protocol.
pub fn spawn_volume_listener(state: Arc<CoreSanState>, volume_id: String, volume_name: String) {
    std::fs::create_dir_all("/run/vmm-san").ok();

    let sock_path = object_socket_path(&volume_id);

    // Remove stale socket
    std::fs::remove_file(&sock_path).ok();

    tokio::spawn(async move {
        let listener = match UnixListener::bind(&sock_path) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Object server: cannot bind {}: {}", sock_path, e);
                return;
            }
        };

        // Make socket world-accessible
        std::fs::set_permissions(&sock_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o666)).ok();

        tracing::info!("Object server: listening on {} (volume '{}')", sock_path, volume_name);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let state = state.clone();
                    let vid = volume_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(state, vid, stream).await {
                            tracing::warn!("Object server: connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Object server: accept error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    });
}

use std::os::unix::fs::PermissionsExt;

async fn handle_connection(
    state: Arc<CoreSanState>,
    volume_id: String,
    mut stream: UnixStream,
) -> Result<(), String> {
    let mut hdr_buf = [0u8; ObjectRequestHeader::SIZE];

    loop {
        // Read request header
        if stream.read_exact(&mut hdr_buf).await.is_err() {
            break; // Connection closed
        }

        let hdr = ObjectRequestHeader::from_bytes(&hdr_buf);
        if hdr.magic != OBJ_REQUEST_MAGIC {
            send_response(&mut stream, ObjectResponseHeader::err(ObjectStatus::ProtocolError), &[], &[]).await;
            break;
        }

        let cmd = match ObjectCommand::from_u32(hdr.cmd) {
            Some(c) => c,
            None => {
                // Drain key + body
                drain(&mut stream, hdr.key_len as u64 + hdr.body_len).await;
                send_response(&mut stream, ObjectResponseHeader::err(ObjectStatus::ProtocolError), &[], &[]).await;
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
            ObjectCommand::Put => handle_put(&state, &volume_id, &key, &body, &mut stream).await,
            ObjectCommand::Get => handle_get(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::Head => handle_head(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::Delete => handle_delete(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::List => handle_list(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::Copy => handle_copy(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::InitMultipart => handle_init_multipart(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::UploadPart => handle_upload_part(&state, &volume_id, &key, &body, &mut stream).await,
            ObjectCommand::CompleteMultipart => handle_complete_multipart(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::AbortMultipart => handle_abort_multipart(&state, &volume_id, &key, &mut stream).await,
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

async fn send_response(stream: &mut UnixStream, header: ObjectResponseHeader, metadata: &[u8], body: &[u8]) {
    let _ = stream.write_all(&header.to_bytes()).await;
    if !metadata.is_empty() {
        let _ = stream.write_all(metadata).await;
    }
    if !body.is_empty() {
        let _ = stream.write_all(body).await;
    }
}

/// Get volume config (chunk_size, local_raid) from DB.
fn get_volume_config(state: &CoreSanState, volume_id: &str) -> Option<(u64, String)> {
    let db = state.db.lock().unwrap();
    db.query_row(
        "SELECT chunk_size_bytes, local_raid FROM volumes WHERE id = ?1",
        rusqlite::params![volume_id],
        |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
    ).ok()
}

// ── PUT ─────────────────────────────────────────────────────────

async fn handle_put(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    data: &[u8],
    stream: &mut UnixStream,
) {
    let (chunk_size, local_raid) = match get_volume_config(state, volume_id) {
        Some(c) => c,
        None => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
            return;
        }
    };

    let file_id = crate::storage::chunk::deterministic_file_id(volume_id, key);
    let sha = format!("{:x}", Sha256::digest(data));
    let now = chrono::Utc::now().to_rfc3339();

    // Upsert file_map entry
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO file_map (id, volume_id, rel_path, size_bytes, sha256, version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET size_bytes = ?4, sha256 = ?5,
                version = version + 1, updated_at = ?6",
            rusqlite::params![file_id, volume_id, key, data.len() as i64, &sha, &now],
        ).ok();
    }

    // Write chunk data
    let write_result = {
        let db = state.db.lock().unwrap();
        crate::storage::chunk::write_chunk_data(
            &db, file_id, 0, data, volume_id, &state.node_id, chunk_size, &local_raid,
        )
    };

    if let Err(e) = write_result {
        tracing::error!("Object server PUT failed for {}: {}", key, e);
        send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
        return;
    }

    // Trigger push replication
    let version = {
        let db = state.db.lock().unwrap();
        db.query_row("SELECT version FROM file_map WHERE id = ?1",
            rusqlite::params![file_id], |row| row.get::<_, i64>(0)).unwrap_or(0)
    };
    let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
        volume_id: volume_id.to_string(),
        rel_path: key.to_string(),
        file_id,
        version,
        writer_node_id: state.node_id.clone(),
    });

    // Return ETag in metadata
    let meta = serde_json::json!({ "etag": sha }).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── GET ─────────────────────────────────────────────────────────

async fn handle_get(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    let file_id = crate::storage::chunk::deterministic_file_id(volume_id, key);

    let file_info = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT size_bytes, sha256, updated_at FROM file_map WHERE id = ?1 AND volume_id = ?2",
            rusqlite::params![file_id, volume_id],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        ).ok()
    };

    let (size, sha, updated_at) = match file_info {
        Some(info) => info,
        None => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
            return;
        }
    };

    let (chunk_size, _local_raid) = match get_volume_config(state, volume_id) {
        Some(c) => c,
        None => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
            return;
        }
    };

    // Read all chunk data
    let data = {
        let db = state.db.lock().unwrap();
        crate::storage::chunk::read_chunk_data(
            &db, file_id, 0, size, volume_id, &state.node_id, chunk_size,
        ).unwrap_or_default()
    };

    let meta = serde_json::json!({
        "etag": sha,
        "content_length": size,
        "last_modified": updated_at,
    }).to_string();
    let meta_bytes = meta.as_bytes();

    send_response(stream, ObjectResponseHeader::ok(data.len() as u64, meta_bytes.len() as u32), meta_bytes, &data).await;
}

// ── HEAD ────────────────────────────────────────────────────────

async fn handle_head(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    let file_id = crate::storage::chunk::deterministic_file_id(volume_id, key);

    let file_info = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT size_bytes, sha256, updated_at FROM file_map WHERE id = ?1 AND volume_id = ?2",
            rusqlite::params![file_id, volume_id],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        ).ok()
    };

    let (size, sha, updated_at) = match file_info {
        Some(info) => info,
        None => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
            return;
        }
    };

    let meta = serde_json::json!({
        "etag": sha,
        "content_length": size,
        "last_modified": updated_at,
    }).to_string();
    let meta_bytes = meta.as_bytes();

    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── DELETE ──────────────────────────────────────────────────────

async fn handle_delete(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    let file_id = crate::storage::chunk::deterministic_file_id(volume_id, key);

    {
        let db = state.db.lock().unwrap();

        // Delete chunk_replicas
        db.execute(
            "DELETE FROM chunk_replicas WHERE chunk_id IN (
                SELECT id FROM file_chunks WHERE file_id = ?1
            )",
            rusqlite::params![file_id],
        ).ok();

        // Delete file_chunks
        db.execute(
            "DELETE FROM file_chunks WHERE file_id = ?1",
            rusqlite::params![file_id],
        ).ok();

        // Delete file_map entry
        db.execute(
            "DELETE FROM file_map WHERE id = ?1 AND volume_id = ?2",
            rusqlite::params![file_id, volume_id],
        ).ok();
    }

    // Delete chunk files on disk
    {
        let db = state.db.lock().unwrap();
        let backend_paths: Vec<String> = {
            let mut stmt = db.prepare(
                "SELECT path FROM backends WHERE status = 'online' AND claimed_disk_id != ''"
            ).unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap().filter_map(|r| r.ok()).collect()
        };
        for bp in backend_paths {
            let dir = std::path::Path::new(&bp)
                .join(".coresan")
                .join(volume_id)
                .join(file_id.to_string());
            if dir.exists() {
                std::fs::remove_dir_all(&dir).ok();
            }
        }
    }

    send_response(stream, ObjectResponseHeader::ok(0, 0), &[], &[]).await;
}

// ── LIST ────────────────────────────────────────────────────────

async fn handle_list(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    // key is JSON: {prefix, delimiter, marker, max_keys}
    #[derive(serde::Deserialize)]
    struct ListParams {
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        delimiter: String,
        #[serde(default)]
        marker: String,
        #[serde(default = "default_max_keys")]
        max_keys: u32,
    }
    fn default_max_keys() -> u32 { 1000 }

    let params: ListParams = serde_json::from_str(key).unwrap_or(ListParams {
        prefix: String::new(),
        delimiter: String::new(),
        marker: String::new(),
        max_keys: 1000,
    });

    let like_pattern = format!("{}%", params.prefix);

    let entries: Vec<(String, u64, String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT rel_path, size_bytes, sha256, updated_at FROM file_map
             WHERE volume_id = ?1 AND rel_path LIKE ?2 AND rel_path > ?3
             ORDER BY rel_path LIMIT ?4"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![volume_id, &like_pattern, &params.marker, params.max_keys],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    // Handle delimiter for common_prefixes
    let mut objects = Vec::new();
    let mut common_prefixes = std::collections::BTreeSet::new();

    for (path, size, etag, last_modified) in &entries {
        if !params.delimiter.is_empty() {
            let suffix = &path[params.prefix.len()..];
            if let Some(pos) = suffix.find(&params.delimiter) {
                let cp = format!("{}{}", params.prefix, &suffix[..=pos + params.delimiter.len() - 1]);
                common_prefixes.insert(cp);
                continue;
            }
        }
        objects.push(serde_json::json!({
            "key": path,
            "size": size,
            "etag": etag,
            "last_modified": last_modified,
        }));
    }

    let is_truncated = entries.len() as u32 >= params.max_keys;
    let result = serde_json::json!({
        "objects": objects,
        "common_prefixes": common_prefixes.into_iter().collect::<Vec<_>>(),
        "is_truncated": is_truncated,
    });

    let body = result.to_string();
    let body_bytes = body.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(body_bytes.len() as u64, 0), &[], body_bytes).await;
}

// ── COPY ────────────────────────────────────────────────────────

async fn handle_copy(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    // key is JSON: {src_key, dst_key}
    #[derive(serde::Deserialize)]
    struct CopyParams {
        src_key: String,
        dst_key: String,
    }

    let params: CopyParams = match serde_json::from_str(key) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::InvalidKey), &[], &[]).await;
            return;
        }
    };

    let src_file_id = crate::storage::chunk::deterministic_file_id(volume_id, &params.src_key);

    let src_info = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT size_bytes, sha256 FROM file_map WHERE id = ?1 AND volume_id = ?2",
            rusqlite::params![src_file_id, volume_id],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
        ).ok()
    };

    let (src_size, src_sha) = match src_info {
        Some(info) => info,
        None => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
            return;
        }
    };

    let (chunk_size, local_raid) = match get_volume_config(state, volume_id) {
        Some(c) => c,
        None => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
            return;
        }
    };

    // Read source data
    let data = {
        let db = state.db.lock().unwrap();
        crate::storage::chunk::read_chunk_data(
            &db, src_file_id, 0, src_size, volume_id, &state.node_id, chunk_size,
        ).unwrap_or_default()
    };

    // Write to destination
    let dst_file_id = crate::storage::chunk::deterministic_file_id(volume_id, &params.dst_key);
    let now = chrono::Utc::now().to_rfc3339();

    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO file_map (id, volume_id, rel_path, size_bytes, sha256, version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET size_bytes = ?4, sha256 = ?5,
                version = version + 1, updated_at = ?6",
            rusqlite::params![dst_file_id, volume_id, &params.dst_key, data.len() as i64, &src_sha, &now],
        ).ok();
    }

    let write_result = {
        let db = state.db.lock().unwrap();
        crate::storage::chunk::write_chunk_data(
            &db, dst_file_id, 0, &data, volume_id, &state.node_id, chunk_size, &local_raid,
        )
    };

    if let Err(e) = write_result {
        tracing::error!("Object server COPY failed {} -> {}: {}", params.src_key, params.dst_key, e);
        send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
        return;
    }

    // Trigger replication for destination
    let version = {
        let db = state.db.lock().unwrap();
        db.query_row("SELECT version FROM file_map WHERE id = ?1",
            rusqlite::params![dst_file_id], |row| row.get::<_, i64>(0)).unwrap_or(0)
    };
    let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
        volume_id: volume_id.to_string(),
        rel_path: params.dst_key,
        file_id: dst_file_id,
        version,
        writer_node_id: state.node_id.clone(),
    });

    let meta = serde_json::json!({ "etag": src_sha }).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── MULTIPART: Init ─────────────────────────────────────────────

async fn handle_init_multipart(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    let upload_id = uuid::Uuid::new_v4().to_string();

    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO multipart_uploads (upload_id, volume_id, object_key, created_by, status)
             VALUES (?1, ?2, ?3, 'object_server', 'active')",
            rusqlite::params![&upload_id, volume_id, key],
        ).ok();
    }

    // Ensure temp directory exists
    std::fs::create_dir_all("/var/lib/vmm-san/multipart").ok();

    let meta = serde_json::json!({ "upload_id": upload_id }).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── MULTIPART: Upload Part ──────────────────────────────────────

async fn handle_upload_part(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    data: &[u8],
    stream: &mut UnixStream,
) {
    // key is JSON: {upload_id, part_number}
    #[derive(serde::Deserialize)]
    struct PartParams {
        upload_id: String,
        part_number: u32,
    }

    let params: PartParams = match serde_json::from_str(key) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::InvalidKey), &[], &[]).await;
            return;
        }
    };

    // Verify upload exists and is active
    let upload_ok = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT COUNT(*) FROM multipart_uploads WHERE upload_id = ?1 AND volume_id = ?2 AND status = 'active'",
            rusqlite::params![&params.upload_id, volume_id],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) > 0
    };

    if !upload_ok {
        send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
        return;
    }

    let etag = format!("{:x}", Sha256::digest(data));

    // Write part to temp file
    let part_path = format!("/var/lib/vmm-san/multipart/{}-{}", params.upload_id, params.part_number);
    if let Err(e) = std::fs::write(&part_path, data) {
        tracing::error!("Object server: write multipart part failed: {}", e);
        send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
        return;
    }

    // Record part in DB
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO multipart_parts (upload_id, part_number, size_bytes, etag, backend_path)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![&params.upload_id, params.part_number, data.len() as i64, &etag, &part_path],
        ).ok();
    }

    let meta = serde_json::json!({ "etag": etag, "part_number": params.part_number }).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── MULTIPART: Complete ─────────────────────────────────────────

async fn handle_complete_multipart(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    // key is JSON: {upload_id, parts: [{part_number, etag}]}
    #[derive(serde::Deserialize)]
    struct CompletePart {
        part_number: u32,
        #[allow(dead_code)]
        etag: String,
    }
    #[derive(serde::Deserialize)]
    struct CompleteParams {
        upload_id: String,
        #[allow(dead_code)]
        parts: Vec<CompletePart>,
    }

    let params: CompleteParams = match serde_json::from_str(key) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::InvalidKey), &[], &[]).await;
            return;
        }
    };

    // Get upload info
    let object_key = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT object_key FROM multipart_uploads WHERE upload_id = ?1 AND volume_id = ?2 AND status = 'active'",
            rusqlite::params![&params.upload_id, volume_id],
            |row| row.get::<_, String>(0),
        ).ok()
    };

    let object_key = match object_key {
        Some(k) => k,
        None => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
            return;
        }
    };

    // Read all parts in order and assemble
    let parts: Vec<(u32, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT part_number, backend_path FROM multipart_parts
             WHERE upload_id = ?1 ORDER BY part_number"
        ).unwrap();
        stmt.query_map(rusqlite::params![&params.upload_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    let mut assembled = Vec::new();
    for (_part_num, path) in &parts {
        match std::fs::read(path) {
            Ok(data) => assembled.extend_from_slice(&data),
            Err(e) => {
                tracing::error!("Object server: read multipart part {} failed: {}", path, e);
                send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
                return;
            }
        }
    }

    let (chunk_size, local_raid) = match get_volume_config(state, volume_id) {
        Some(c) => c,
        None => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
            return;
        }
    };

    // Write assembled object
    let file_id = crate::storage::chunk::deterministic_file_id(volume_id, &object_key);
    let sha = format!("{:x}", Sha256::digest(&assembled));
    let now = chrono::Utc::now().to_rfc3339();

    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO file_map (id, volume_id, rel_path, size_bytes, sha256, version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET size_bytes = ?4, sha256 = ?5,
                version = version + 1, updated_at = ?6",
            rusqlite::params![file_id, volume_id, &object_key, assembled.len() as i64, &sha, &now],
        ).ok();
    }

    let write_result = {
        let db = state.db.lock().unwrap();
        crate::storage::chunk::write_chunk_data(
            &db, file_id, 0, &assembled, volume_id, &state.node_id, chunk_size, &local_raid,
        )
    };

    if let Err(e) = write_result {
        tracing::error!("Object server: complete multipart write failed: {}", e);
        send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
        return;
    }

    // Trigger push replication
    let version = {
        let db = state.db.lock().unwrap();
        db.query_row("SELECT version FROM file_map WHERE id = ?1",
            rusqlite::params![file_id], |row| row.get::<_, i64>(0)).unwrap_or(0)
    };
    let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
        volume_id: volume_id.to_string(),
        rel_path: object_key,
        file_id,
        version,
        writer_node_id: state.node_id.clone(),
    });

    // Mark upload as completed
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "UPDATE multipart_uploads SET status = 'completed' WHERE upload_id = ?1",
            rusqlite::params![&params.upload_id],
        ).ok();
    }

    // Clean up temp files
    for (_part_num, path) in &parts {
        std::fs::remove_file(path).ok();
    }

    let meta = serde_json::json!({ "etag": sha }).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── MULTIPART: Abort ────────────────────────────────────────────

async fn handle_abort_multipart(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    // key is JSON: {upload_id}
    #[derive(serde::Deserialize)]
    struct AbortParams {
        upload_id: String,
    }

    let params: AbortParams = match serde_json::from_str(key) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::InvalidKey), &[], &[]).await;
            return;
        }
    };

    // Get part paths for cleanup
    let part_paths: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT backend_path FROM multipart_parts WHERE upload_id = ?1"
        ).unwrap();
        stmt.query_map(rusqlite::params![&params.upload_id], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    // Clean up temp files
    for path in &part_paths {
        std::fs::remove_file(path).ok();
    }

    // Delete parts and mark upload as aborted
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "DELETE FROM multipart_parts WHERE upload_id = ?1",
            rusqlite::params![&params.upload_id],
        ).ok();
        db.execute(
            "UPDATE multipart_uploads SET status = 'aborted' WHERE upload_id = ?1 AND volume_id = ?2",
            rusqlite::params![&params.upload_id, volume_id],
        ).ok();
    }

    send_response(stream, ObjectResponseHeader::ok(0, 0), &[], &[]).await;
}
