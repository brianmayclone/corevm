//! Direct disk I/O server — serves VM disk reads/writes via Unix Domain Socket.
//!
//! Bypasses FUSE entirely. One UDS listener per volume, one tokio task per VM connection.
//! Uses the same chunk_cache + write_chunk_data as FUSE, but without single-thread bottleneck.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use vmm_core::san_disk::*;
use crate::state::CoreSanState;

const MAX_CHUNK_CACHE: usize = 128;

/// Cached chunk in RAM (same concept as FUSE ChunkBuffer).
struct ChunkBuf {
    data: Vec<u8>,
    dirty: bool,
    first_dirty: std::time::Instant,
    last_write: std::time::Instant,
}

/// Per-connection state.
struct DiskSession {
    volume_id: String,
    file_id: i64,
    rel_path: String,
    chunk_size: u64,
    local_raid: String,
    cache: HashMap<u32, ChunkBuf>, // chunk_index -> buffer
}

/// Spawn UDS listeners for all online volumes.
pub fn spawn_all(state: Arc<CoreSanState>) {
    let volumes: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name FROM volumes WHERE status = 'online'"
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

/// Spawn a single UDS listener for a volume.
pub fn spawn_volume_listener(state: Arc<CoreSanState>, volume_id: String, volume_name: String) {
    let sock_path = socket_path(&volume_id);

    // Remove stale socket
    std::fs::remove_file(&sock_path).ok();

    tokio::spawn(async move {
        let listener = match UnixListener::bind(&sock_path) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Disk server: cannot bind {}: {}", sock_path, e);
                return;
            }
        };

        // Make socket world-accessible (vmm-server runs as root too)
        std::fs::set_permissions(&sock_path, std::os::unix::fs::PermissionsExt::from_mode(0o666)).ok();

        tracing::info!("Disk server: listening on {} (volume '{}')", sock_path, volume_name);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let state = state.clone();
                    let vid = volume_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(state, vid, stream).await {
                            tracing::warn!("Disk server: connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Disk server: accept error: {}", e);
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
    let mut session: Option<DiskSession> = None;
    let mut req_buf = [0u8; SanRequestHeader::SIZE];

    loop {
        // Read request header
        if stream.read_exact(&mut req_buf).await.is_err() {
            // Connection closed
            break;
        }

        let req = SanRequestHeader::from_bytes(&req_buf);
        if req.magic != REQUEST_MAGIC {
            send_response(&mut stream, SanResponseHeader::err(SanStatus::ErrProtocol), &[]).await;
            break;
        }

        let cmd = match SanCommand::from_u32(req.cmd) {
            Some(c) => c,
            None => {
                send_response(&mut stream, SanResponseHeader::err(SanStatus::ErrProtocol), &[]).await;
                continue;
            }
        };

        match cmd {
            SanCommand::Open => {
                // Read rel_path from payload
                let mut path_buf = vec![0u8; req.size as usize];
                if stream.read_exact(&mut path_buf).await.is_err() {
                    break;
                }
                let rel_path = String::from_utf8_lossy(&path_buf).to_string();

                let result = {
                    let db = state.db.lock().unwrap();

                    // Get file_id
                    let file_id = db.query_row(
                        "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                        rusqlite::params![&volume_id, &rel_path], |row| row.get::<_, i64>(0),
                    ).ok();

                    // Get volume config
                    let vol_config = db.query_row(
                        "SELECT chunk_size_bytes, local_raid FROM volumes WHERE id = ?1",
                        rusqlite::params![&volume_id],
                        |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
                    ).ok();

                    match (file_id, vol_config) {
                        (Some(fid), Some((cs, lr))) => Ok((fid, cs, lr)),
                        _ => Err(SanStatus::ErrNotFound),
                    }
                };

                match result {
                    Ok((file_id, chunk_size, local_raid)) => {
                        // Acquire write lease
                        let lease_ok = {
                            let db = state.db.lock().unwrap();
                            let quorum = *state.quorum_status.read().unwrap();
                            crate::engine::write_lease::acquire_lease(
                                &db, &volume_id, &rel_path, &state.node_id, quorum,
                            )
                        };

                        match lease_ok {
                            crate::engine::write_lease::LeaseResult::Acquired { .. } |
                            crate::engine::write_lease::LeaseResult::Renewed { .. } => {
                                tracing::info!("Disk server: opened {}/{} (file_id={})", volume_id, rel_path, file_id);
                                session = Some(DiskSession {
                                    volume_id: volume_id.clone(),
                                    file_id,
                                    rel_path,
                                    chunk_size,
                                    local_raid,
                                    cache: HashMap::new(),
                                });
                                // Return file_id as data
                                let fid_bytes = file_id.to_le_bytes();
                                send_response(&mut stream, SanResponseHeader::ok(8), &fid_bytes).await;
                            }
                            crate::engine::write_lease::LeaseResult::Denied { .. } => {
                                send_response(&mut stream, SanResponseHeader::err(SanStatus::ErrLeaseDenied), &[]).await;
                            }
                        }
                    }
                    Err(status) => {
                        send_response(&mut stream, SanResponseHeader::err(status), &[]).await;
                    }
                }
            }

            SanCommand::Read => {
                let sess = match session.as_mut() {
                    Some(s) => s,
                    None => {
                        send_response(&mut stream, SanResponseHeader::err(SanStatus::ErrNotFound), &[]).await;
                        continue;
                    }
                };

                let offset = req.offset;
                let size = req.size as u64;

                // Check cache first
                let chunk_idx = (offset / sess.chunk_size) as u32;
                let local_offset = (offset % sess.chunk_size) as usize;
                let read_size = size as usize;

                if let Some(buf) = sess.cache.get(&chunk_idx) {
                    // Cache hit — serve from RAM
                    let end = (local_offset + read_size).min(buf.data.len());
                    let data = if local_offset < buf.data.len() {
                        &buf.data[local_offset..end]
                    } else {
                        &[]
                    };
                    send_response(&mut stream, SanResponseHeader::ok(data.len() as u32), data).await;
                } else {
                    // Cache miss — read from chunk system
                    let data = {
                        let db = state.db.lock().unwrap();
                        crate::storage::chunk::read_chunk_data(
                            &db, sess.file_id, offset, size,
                            &sess.volume_id, &state.node_id, sess.chunk_size,
                        ).unwrap_or_else(|_| vec![0u8; size as usize])
                    };
                    send_response(&mut stream, SanResponseHeader::ok(data.len() as u32), &data).await;
                }
            }

            SanCommand::Write => {
                let sess = match session.as_mut() {
                    Some(s) => s,
                    None => {
                        // Drain payload
                        let mut discard = vec![0u8; req.size as usize];
                        stream.read_exact(&mut discard).await.ok();
                        send_response(&mut stream, SanResponseHeader::err(SanStatus::ErrNotFound), &[]).await;
                        continue;
                    }
                };

                // Read write data
                let mut data = vec![0u8; req.size as usize];
                if stream.read_exact(&mut data).await.is_err() {
                    break;
                }

                let offset = req.offset;
                let chunk_idx = (offset / sess.chunk_size) as u32;
                let local_offset = (offset % sess.chunk_size) as usize;

                // Write to cache
                let buf = sess.cache.entry(chunk_idx).or_insert_with(|| {
                    // Check if chunk exists on disk
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

                // Patch data into buffer
                let end = local_offset + data.len();
                if buf.data.len() < end {
                    buf.data.resize(end, 0);
                }
                buf.data[local_offset..end].copy_from_slice(&data);
                if !buf.dirty {
                    buf.first_dirty = std::time::Instant::now();
                }
                buf.dirty = true;
                buf.last_write = std::time::Instant::now();

                // Flush stale chunks (>5s idle or >10s dirty)
                flush_stale(&state, sess);

                // Evict if cache too large
                if sess.cache.len() > MAX_CHUNK_CACHE {
                    evict_oldest(&state, sess);
                }

                send_response(&mut stream, SanResponseHeader::ok(0), &[]).await;
            }

            SanCommand::Flush => {
                if let Some(sess) = session.as_mut() {
                    flush_all(&state, sess);
                    // Also update file_map size
                    let max_size: u64 = sess.cache.iter()
                        .map(|(idx, buf)| *idx as u64 * sess.chunk_size + buf.data.len() as u64)
                        .max().unwrap_or(0);
                    if max_size > 0 {
                        let db = state.db.lock().unwrap();
                        db.execute(
                            "UPDATE file_map SET size_bytes = MAX(size_bytes, ?1) WHERE id = ?2",
                            rusqlite::params![max_size as i64, sess.file_id],
                        ).ok();
                    }
                }
                send_response(&mut stream, SanResponseHeader::ok(0), &[]).await;
            }

            SanCommand::Close => {
                if let Some(mut sess) = session.take() {
                    flush_all(&state, &mut sess);
                    // Release write lease
                    let db = state.db.lock().unwrap();
                    crate::engine::write_lease::release_lease(
                        &db, &sess.volume_id, &sess.rel_path, &state.node_id,
                    );
                    tracing::info!("Disk server: closed {}/{}", sess.volume_id, sess.rel_path);
                }
                send_response(&mut stream, SanResponseHeader::ok(0), &[]).await;
            }

            SanCommand::GetSize => {
                let size = if let Some(sess) = &session {
                    let db = state.db.lock().unwrap();
                    db.query_row(
                        "SELECT size_bytes FROM file_map WHERE id = ?1",
                        rusqlite::params![sess.file_id], |row| row.get::<_, u64>(0),
                    ).unwrap_or(0)
                } else {
                    0
                };
                send_response(&mut stream, SanResponseHeader::ok(8), &size.to_le_bytes()).await;
            }
        }
    }

    // Connection closed — flush remaining data
    if let Some(mut sess) = session.take() {
        flush_all(&state, &mut sess);
        let db = state.db.lock().unwrap();
        crate::engine::write_lease::release_lease(
            &db, &sess.volume_id, &sess.rel_path, &state.node_id,
        );
        tracing::info!("Disk server: connection closed, flushed {}/{}", sess.volume_id, sess.rel_path);
    }

    Ok(())
}

async fn send_response(stream: &mut UnixStream, header: SanResponseHeader, data: &[u8]) {
    let _ = stream.write_all(&header.to_bytes()).await;
    if !data.is_empty() {
        let _ = stream.write_all(data).await;
    }
}

/// Flush stale cached chunks to disk (>5s idle or >10s dirty).
fn flush_stale(state: &CoreSanState, sess: &mut DiskSession) {
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

/// Flush ALL dirty chunks to disk + trigger replication.
fn flush_all(state: &CoreSanState, sess: &mut DiskSession) {
    let dirty: Vec<(u32, Vec<u8>)> = sess.cache.drain()
        .filter(|(_, b)| b.dirty)
        .map(|(k, b)| (k, b.data))
        .collect();

    for (idx, data) in &dirty {
        write_chunk(state, sess, *idx, data);
    }

    if !dirty.is_empty() {
        tracing::info!("Disk server: flushed {} chunks for file_id={}", dirty.len(), sess.file_id);

        // Trigger push replication
        let version = {
            let db = state.db.lock().unwrap();
            db.execute(
                "UPDATE file_map SET version = version + 1, updated_at = datetime('now') WHERE id = ?1",
                rusqlite::params![sess.file_id],
            ).ok();
            db.query_row("SELECT version FROM file_map WHERE id = ?1",
                rusqlite::params![sess.file_id], |row| row.get::<_, i64>(0)).unwrap_or(0)
        };
        let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
            volume_id: sess.volume_id.clone(),
            rel_path: sess.rel_path.clone(),
            file_id: sess.file_id,
            version,
            writer_node_id: state.node_id.clone(),
        });
    }
}

/// Evict the oldest cached chunk.
fn evict_oldest(state: &CoreSanState, sess: &mut DiskSession) {
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
fn write_chunk(state: &CoreSanState, sess: &DiskSession, chunk_index: u32, data: &[u8]) {
    let offset = chunk_index as u64 * sess.chunk_size;
    let db = state.db.lock().unwrap();
    match crate::storage::chunk::write_chunk_data(
        &db, sess.file_id, offset, data,
        &sess.volume_id, &state.node_id, sess.chunk_size, &sess.local_raid,
    ) {
        Ok(_) => {}
        Err(e) => tracing::error!("Disk server: write_chunk_data failed: {}", e),
    }
}
