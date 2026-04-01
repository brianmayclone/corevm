//! iSCSI block I/O server — serves block reads/writes via Unix Domain Socket.
//!
//! One UDS listener per iSCSI-enabled volume. vmm-iscsi connects to these sockets
//! to translate iSCSI SCSI commands into block I/O on the CoreSAN volume.
//! Uses a virtual file `.iscsi-block` per volume for chunk storage.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use vmm_core::san_iscsi::*;
use crate::state::CoreSanState;

const BLOCK_SIZE: u64 = 512;
const MAX_IO_SIZE: u32 = 4 * 1024 * 1024; // 4 MB max per I/O
const MAX_CHUNK_CACHE: usize = 128;

/// Cached chunk in RAM.
struct ChunkBuf {
    data: Vec<u8>,
    dirty: bool,
    first_dirty: std::time::Instant,
    last_write: std::time::Instant,
}

/// Per-connection state for iSCSI block I/O.
struct BlockSession {
    volume_id: String,
    file_id: i64,
    max_size_bytes: u64,
    chunk_size: u64,
    local_raid: String,
    cache: HashMap<u32, ChunkBuf>,
}

/// Spawn UDS listeners for all iSCSI-enabled online volumes.
pub fn spawn_all(state: Arc<CoreSanState>) {
    let volumes: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name FROM volumes WHERE status = 'online' AND access_protocols LIKE '%iscsi%'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    std::fs::create_dir_all("/run/vmm-san").ok();

    for (vol_id, vol_name) in volumes {
        spawn_volume_listener(state.clone(), vol_id, vol_name);
    }
}

/// Spawn a single UDS listener for a volume's iSCSI block I/O.
pub fn spawn_volume_listener(state: Arc<CoreSanState>, volume_id: String, volume_name: String) {
    std::fs::create_dir_all("/run/vmm-san").ok();
    let sock_path = block_socket_path(&volume_id);
    std::fs::remove_file(&sock_path).ok();

    tokio::spawn(async move {
        let listener = match UnixListener::bind(&sock_path) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("iSCSI block server: cannot bind {}: {}", sock_path, e);
                return;
            }
        };

        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sock_path,
            std::fs::Permissions::from_mode(0o666)).ok();

        tracing::info!("iSCSI block server: listening on {} (volume '{}')", sock_path, volume_name);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let st = state.clone();
                    let vid = volume_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(st, vid, stream).await {
                            tracing::debug!("iSCSI block session ended: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("iSCSI block server accept error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    });
}

async fn handle_connection(
    state: Arc<CoreSanState>,
    volume_id: String,
    mut stream: UnixStream,
) -> Result<(), String> {
    // Load volume metadata and set up block session
    let (max_size_bytes, chunk_size, local_raid) = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT max_size_bytes, chunk_size_bytes, local_raid FROM volumes WHERE id = ?1",
            rusqlite::params![&volume_id],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?, row.get::<_, String>(2)?)),
        ).map_err(|e| format!("volume lookup: {}", e))?
    };

    // Use a virtual file ".iscsi-block" for the block device
    let rel_path = ".iscsi-block";
    let file_id = crate::storage::chunk::deterministic_file_id(&volume_id, rel_path);

    // Ensure file_map entry exists for the virtual block file
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT OR IGNORE INTO file_map (id, volume_id, rel_path, size_bytes, sha256, chunk_count)
             VALUES (?1, ?2, ?3, ?4, '', 0)",
            rusqlite::params![file_id, &volume_id, rel_path, max_size_bytes as i64],
        ).ok();
    }

    let mut session = BlockSession {
        volume_id: volume_id.clone(),
        file_id,
        max_size_bytes,
        chunk_size,
        local_raid,
        cache: HashMap::new(),
    };

    let mut hdr_buf = [0u8; IscsiRequestHeader::SIZE];

    loop {
        if stream.read_exact(&mut hdr_buf).await.is_err() {
            flush_all(&state, &mut session);
            break;
        }

        let hdr = IscsiRequestHeader::from_bytes(&hdr_buf);
        if hdr.magic != ISCSI_REQUEST_MAGIC {
            flush_all(&state, &mut session);
            return Err("invalid magic".into());
        }

        let cmd = match IscsiCommand::from_u32(hdr.cmd) {
            Some(c) => c,
            None => {
                send_response(&mut stream, IscsiResponseHeader::err(IscsiStatus::ProtocolError), &[]).await;
                continue;
            }
        };

        match cmd {
            IscsiCommand::ReadBlocks => {
                let byte_offset = hdr.lba * BLOCK_SIZE;
                let length = hdr.length;
                if length > MAX_IO_SIZE || byte_offset + length as u64 > max_size_bytes {
                    send_response(&mut stream, IscsiResponseHeader::err(IscsiStatus::OutOfRange), &[]).await;
                    continue;
                }

                let data = read_blocks(&state, &mut session, byte_offset, length as u64);
                send_response(&mut stream, IscsiResponseHeader::ok(data.len() as u32), &data).await;
            }

            IscsiCommand::WriteBlocks => {
                let byte_offset = hdr.lba * BLOCK_SIZE;
                let length = hdr.length;
                if length > MAX_IO_SIZE || byte_offset + length as u64 > max_size_bytes {
                    // Drain payload
                    let mut discard = vec![0u8; length as usize];
                    stream.read_exact(&mut discard).await.ok();
                    send_response(&mut stream, IscsiResponseHeader::err(IscsiStatus::OutOfRange), &[]).await;
                    continue;
                }

                let mut data = vec![0u8; length as usize];
                if stream.read_exact(&mut data).await.is_err() {
                    flush_all(&state, &mut session);
                    break;
                }

                write_blocks(&state, &mut session, byte_offset, &data);

                // Flush stale and evict if needed
                flush_stale(&state, &mut session);
                if session.cache.len() > MAX_CHUNK_CACHE {
                    evict_oldest(&state, &mut session);
                }

                send_response(&mut stream, IscsiResponseHeader::ok(0), &[]).await;
            }

            IscsiCommand::Flush => {
                flush_all(&state, &mut session);
                send_response(&mut stream, IscsiResponseHeader::ok(0), &[]).await;
            }

            IscsiCommand::GetCapacity => {
                let body = serde_json::json!({
                    "size_bytes": max_size_bytes,
                    "block_size": BLOCK_SIZE,
                }).to_string();
                send_response(&mut stream, IscsiResponseHeader::ok(body.len() as u32), body.as_bytes()).await;
            }

            IscsiCommand::GetAluaState => {
                // leader_node_id is not yet tracked in the volumes table;
                // default to active_optimized on every node for now.
                let alua_state = "active_optimized";
                let body = serde_json::json!({
                    "state": alua_state,
                    "tpg_id": &state.node_id,
                }).to_string();
                send_response(&mut stream, IscsiResponseHeader::ok(body.len() as u32), body.as_bytes()).await;
            }
        }
    }

    Ok(())
}

fn read_blocks(state: &CoreSanState, sess: &mut BlockSession, offset: u64, size: u64) -> Vec<u8> {
    let chunk_idx = (offset / sess.chunk_size) as u32;
    let local_offset = (offset % sess.chunk_size) as usize;
    let read_size = size as usize;

    // Simple case: single chunk read
    if local_offset + read_size <= sess.chunk_size as usize {
        if let Some(buf) = sess.cache.get(&chunk_idx) {
            let end = (local_offset + read_size).min(buf.data.len());
            if local_offset < buf.data.len() {
                return buf.data[local_offset..end].to_vec();
            }
            return vec![0u8; read_size];
        }

        // Cache miss — read from chunk system
        let db = state.db.lock().unwrap();
        crate::storage::chunk::read_chunk_data(
            &db, sess.file_id, offset, size,
            &sess.volume_id, &state.node_id, sess.chunk_size,
        ).unwrap_or_else(|_| vec![0u8; size as usize])
    } else {
        // Cross-chunk read — delegate to chunk system (handles spanning)
        let db = state.db.lock().unwrap();
        crate::storage::chunk::read_chunk_data(
            &db, sess.file_id, offset, size,
            &sess.volume_id, &state.node_id, sess.chunk_size,
        ).unwrap_or_else(|_| vec![0u8; size as usize])
    }
}

fn write_blocks(state: &CoreSanState, sess: &mut BlockSession, offset: u64, data: &[u8]) {
    let chunk_idx = (offset / sess.chunk_size) as u32;
    let local_offset = (offset % sess.chunk_size) as usize;

    // Simple case: single chunk write
    if local_offset + data.len() <= sess.chunk_size as usize {
        let buf = sess.cache.entry(chunk_idx).or_insert_with(|| {
            let has_data = {
                let db = state.db.lock().unwrap();
                db.query_row(
                    "SELECT COUNT(*) FROM chunk_replicas cr
                     JOIN file_chunks fc ON fc.id = cr.chunk_id
                     WHERE fc.file_id = ?1 AND fc.chunk_index = ?2
                       AND cr.node_id = ?3 AND cr.state = 'synced'",
                    rusqlite::params![sess.file_id, chunk_idx, &state.node_id],
                    |row| row.get::<_, i64>(0),
                ).unwrap_or(0) > 0
            };

            let chunk_data = if has_data {
                let db = state.db.lock().unwrap();
                crate::storage::chunk::read_chunk_data(
                    &db, sess.file_id, chunk_idx as u64 * sess.chunk_size,
                    sess.chunk_size, &sess.volume_id, &state.node_id, sess.chunk_size,
                ).unwrap_or_else(|_| vec![0u8; sess.chunk_size as usize])
            } else {
                vec![0u8; sess.chunk_size as usize]
            };

            let now = std::time::Instant::now();
            ChunkBuf { data: chunk_data, dirty: false, first_dirty: now, last_write: now }
        });

        let end = local_offset + data.len();
        if buf.data.len() < end {
            buf.data.resize(end, 0);
        }
        buf.data[local_offset..end].copy_from_slice(data);
        if !buf.dirty {
            buf.first_dirty = std::time::Instant::now();
        }
        buf.dirty = true;
        buf.last_write = std::time::Instant::now();
    } else {
        // Cross-chunk write — write directly without cache
        let db = state.db.lock().unwrap();
        let _ = crate::storage::chunk::write_chunk_data(
            &db, sess.file_id, offset, data,
            &sess.volume_id, &state.node_id, sess.chunk_size, &sess.local_raid,
        );
    }
}

async fn send_response(stream: &mut UnixStream, header: IscsiResponseHeader, data: &[u8]) {
    let _ = stream.write_all(&header.to_bytes()).await;
    if !data.is_empty() {
        let _ = stream.write_all(data).await;
    }
}

/// Flush stale cached chunks to disk (>5s idle or >10s dirty).
fn flush_stale(state: &CoreSanState, sess: &mut BlockSession) {
    let now = std::time::Instant::now();
    let idle = std::time::Duration::from_secs(5);
    let max_dirty = std::time::Duration::from_secs(10);

    let stale: Vec<u32> = sess.cache.iter()
        .filter(|(_, b)| b.dirty && (
            now.duration_since(b.last_write) > idle ||
            now.duration_since(b.first_dirty) > max_dirty
        ))
        .map(|(k, _)| *k)
        .collect();

    for idx in stale {
        if let Some(buf) = sess.cache.remove(&idx) {
            if buf.dirty {
                write_chunk(state, sess, idx, &buf.data);
            }
        }
    }
}

/// Flush ALL dirty chunks to disk.
fn flush_all(state: &CoreSanState, sess: &mut BlockSession) {
    let dirty: Vec<(u32, Vec<u8>)> = sess.cache.drain()
        .filter(|(_, b)| b.dirty)
        .map(|(k, b)| (k, b.data))
        .collect();

    for (idx, data) in &dirty {
        write_chunk(state, sess, *idx, data);
    }

    if !dirty.is_empty() {
        tracing::info!("iSCSI block server: flushed {} chunks for volume {}", dirty.len(), sess.volume_id);
    }
}

/// Evict the oldest cached chunk.
fn evict_oldest(state: &CoreSanState, sess: &mut BlockSession) {
    let oldest = sess.cache.iter()
        .min_by_key(|(_, b)| b.last_write)
        .map(|(k, _)| *k);

    if let Some(idx) = oldest {
        if let Some(buf) = sess.cache.remove(&idx) {
            if buf.dirty {
                write_chunk(state, sess, idx, &buf.data);
            }
        }
    }
}

/// Write a chunk to disk via the chunk system.
fn write_chunk(state: &CoreSanState, sess: &BlockSession, chunk_index: u32, data: &[u8]) {
    let offset = chunk_index as u64 * sess.chunk_size;
    let db = state.db.lock().unwrap();
    if let Err(e) = crate::storage::chunk::write_chunk_data(
        &db, sess.file_id, offset, data,
        &sess.volume_id, &state.node_id, sess.chunk_size, &sess.local_raid,
    ) {
        tracing::error!("iSCSI block server: write_chunk FAILED for volume {}: {}", sess.volume_id, e);
    }
}
