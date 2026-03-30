//! FUSE filesystem driver — mounts each volume as a local filesystem.
//!
//! VMs access SAN storage through FUSE mounts at /vmm/san/<volume_name>.
//! Reads prefer local replicas; writes go local-first with background replication.
//! If a file has no local replica, it is fetched transparently from a peer.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyWrite, Request,
};

use crate::peer::client::PeerClient;
use crate::state::CoreSanState;

const TTL: Duration = Duration::from_secs(0);
const BLOCK_SIZE: u32 = 4096;

/// Inode-to-path mapping for the FUSE filesystem.
struct InodeMap {
    /// inode -> (volume_id, relative_path, is_dir)
    entries: HashMap<u64, InodeEntry>,
    /// (volume_id, rel_path) -> inode (reverse lookup)
    paths: HashMap<(String, String), u64>,
    next_ino: u64,
}

#[derive(Clone)]
struct InodeEntry {
    volume_id: String,
    rel_path: String,
    is_dir: bool,
}

impl InodeMap {
    fn new(volume_id: &str) -> Self {
        let mut map = InodeMap {
            entries: HashMap::new(),
            paths: HashMap::new(),
            next_ino: 2, // 1 is reserved for root
        };
        // Root inode
        map.entries.insert(1, InodeEntry {
            volume_id: volume_id.to_string(),
            rel_path: String::new(),
            is_dir: true,
        });
        map.paths.insert((volume_id.to_string(), String::new()), 1);
        map
    }

    fn get_or_create(&mut self, volume_id: &str, rel_path: &str, is_dir: bool) -> u64 {
        let key = (volume_id.to_string(), rel_path.to_string());
        if let Some(&ino) = self.paths.get(&key) {
            return ino;
        }
        let ino = self.next_ino;
        self.next_ino += 1;
        self.entries.insert(ino, InodeEntry {
            volume_id: volume_id.to_string(),
            rel_path: rel_path.to_string(),
            is_dir,
        });
        self.paths.insert(key, ino);
        ino
    }

    fn get(&self, ino: u64) -> Option<&InodeEntry> {
        self.entries.get(&ino)
    }

    fn remove(&mut self, ino: u64) {
        if let Some(entry) = self.entries.remove(&ino) {
            self.paths.remove(&(entry.volume_id, entry.rel_path));
        }
    }
}

/// Buffered chunk data in RAM — avoids read-modify-write for every 4K write.
struct ChunkBuffer {
    data: Vec<u8>,
    dirty: bool,
    last_write: std::time::Instant,
    first_dirty: std::time::Instant,
}

/// CoreSAN FUSE filesystem — one instance per mounted volume.
pub struct CoreSanFS {
    state: Arc<CoreSanState>,
    volume_id: String,
    inodes: Mutex<InodeMap>,
    /// Tokio runtime handle for blocking on async peer operations.
    rt: tokio::runtime::Handle,
    /// Write-through cache: (file_id, chunk_index) → chunk data in RAM.
    /// Writes go to RAM first, flushed to disk on fsync/release or when buffer is full.
    chunk_cache: std::sync::Mutex<std::collections::HashMap<(i64, u32), ChunkBuffer>>,
}

const MAX_CACHED_CHUNKS: usize = 128; // ~512 MB RAM max at 4MB chunks

impl CoreSanFS {
    pub fn new(state: Arc<CoreSanState>, volume_id: String, rt: tokio::runtime::Handle) -> Self {
        let inodes = Mutex::new(InodeMap::new(&volume_id));
        CoreSanFS {
            state, volume_id, inodes, rt,
            chunk_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Write a single dirty chunk buffer to disk. NO locks held when calling this.
    fn write_chunk_to_disk(&self, file_id: i64, chunk_index: u32, data: &[u8]) {
        let (chunk_size, _, local_raid) = self.volume_config();
        let offset = chunk_index as u64 * chunk_size;
        let db = self.state.db.lock().unwrap();
        let _ = crate::storage::chunk::write_chunk_data(
            &db, file_id, offset, data,
            &self.volume_id, &self.state.node_id, chunk_size, &local_raid,
        );
    }

    /// Flush all dirty chunks for a file to disk.
    fn flush_file_chunks(&self, file_id: i64) {
        // Extract dirty buffers from cache first (release cache lock), then write
        let dirty: Vec<(u32, Vec<u8>)> = {
            let mut cache = self.chunk_cache.lock().unwrap();
            let keys: Vec<(i64, u32)> = cache.keys()
                .filter(|(fid, _)| *fid == file_id)
                .cloned().collect();
            keys.into_iter().filter_map(|key| {
                cache.remove(&key).and_then(|buf| {
                    if buf.dirty { Some((key.1, buf.data)) } else { None }
                })
            }).collect()
        };
        // Now write each chunk — no cache lock held
        for (ci, data) in &dirty {
            self.write_chunk_to_disk(file_id, *ci, data);
        }
    }

    /// Flush all dirty chunks (for all files) to disk.
    fn flush_all_chunks(&self) {
        let dirty: Vec<(i64, u32, Vec<u8>)> = {
            let mut cache = self.chunk_cache.lock().unwrap();
            cache.drain().filter_map(|((fid, ci), buf)| {
                if buf.dirty { Some((fid, ci, buf.data)) } else { None }
            }).collect()
        };
        for (fid, ci, data) in &dirty {
            self.write_chunk_to_disk(*fid, *ci, data);
        }
    }

    /// Evict oldest cached chunks if cache is too large.
    fn evict_if_needed(&self) {
        let evicted = {
            let mut cache = self.chunk_cache.lock().unwrap();
            if cache.len() <= MAX_CACHED_CHUNKS { return; }
            let oldest_key = cache.iter()
                .min_by_key(|(_, buf)| buf.last_write)
                .map(|(k, _)| *k);
            oldest_key.and_then(|key| {
                cache.remove(&key).map(|buf| (key, buf))
            })
        };
        if let Some(((fid, ci), buf)) = evicted {
            if buf.dirty {
                self.write_chunk_to_disk(fid, ci, &buf.data);
            }
        }
    }

    /// Flush dirty chunks that are idle (>5s) or too old (>10s dirty).
    fn flush_stale_chunks(&self) {
        let idle_threshold = std::time::Duration::from_secs(5);
        let max_dirty_threshold = std::time::Duration::from_secs(10);
        let now = std::time::Instant::now();

        let dirty: Vec<(i64, u32, Vec<u8>)> = {
            let mut cache = self.chunk_cache.lock().unwrap();
            let stale_keys: Vec<(i64, u32)> = cache.iter()
                .filter(|(_, buf)| buf.dirty && (
                    now.duration_since(buf.last_write) > idle_threshold ||
                    now.duration_since(buf.first_dirty) > max_dirty_threshold
                ))
                .map(|(k, _)| *k)
                .collect();
            stale_keys.into_iter().filter_map(|key| {
                cache.remove(&key).and_then(|buf| {
                    if buf.dirty { Some((key.0, key.1, buf.data)) } else { None }
                })
            }).collect()
        };
        // Write outside cache lock
        for (fid, ci, data) in &dirty {
            self.write_chunk_to_disk(*fid, *ci, data);
            tracing::debug!("Flushed stale chunk file_id={} idx={}", fid, ci);
        }
    }

    /// Get file_id from file_map for a given rel_path.
    fn get_file_id(&self, rel_path: &str) -> Option<i64> {
        let db = self.state.db.lock().unwrap();
        db.query_row(
            "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
            rusqlite::params![&self.volume_id, rel_path],
            |row| row.get(0),
        ).ok()
    }

    /// Get volume config (chunk_size, ftt, local_raid).
    fn volume_config(&self) -> (u64, u32, String) {
        let db = self.state.db.lock().unwrap();
        db.query_row(
            "SELECT chunk_size_bytes, ftt, local_raid FROM volumes WHERE id = ?1",
            rusqlite::params![&self.volume_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap_or((crate::storage::chunk::DEFAULT_CHUNK_SIZE, 1, "stripe".into()))
    }

    /// Check if a file has local chunk data available.
    /// Returns true if at least one chunk replica is synced locally.
    /// Used by lookup/getattr to determine if the file is accessible.
    fn has_local_chunks(&self, rel_path: &str) -> bool {
        let db = self.state.db.lock().unwrap();
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN file_map fm ON fm.id = fc.file_id
             WHERE fm.volume_id = ?1 AND fm.rel_path = ?2
               AND cr.node_id = ?3 AND cr.state = 'synced'",
            rusqlite::params![&self.volume_id, rel_path, &self.state.node_id],
            |row| row.get(0),
        ).unwrap_or(0);
        count > 0
    }

    /// Check if a file exists (in file_map or has remote chunks).
    fn file_exists(&self, rel_path: &str) -> bool {
        let db = self.state.db.lock().unwrap();
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
            rusqlite::params![&self.volume_id, rel_path], |row| row.get(0),
        ).unwrap_or(0);
        count > 0
    }

    /// Fetch all chunks of a file from peers and store locally.
    /// Used when FUSE needs data that isn't available locally.
    fn fetch_chunks_from_peer(&self, rel_path: &str) -> bool {
        let file_info = {
            let db = self.state.db.lock().unwrap();
            db.query_row(
                "SELECT fm.id, fm.size_bytes, v.chunk_size_bytes, v.local_raid
                 FROM file_map fm
                 JOIN volumes v ON v.id = fm.volume_id
                 WHERE fm.volume_id = ?1 AND fm.rel_path = ?2",
                rusqlite::params![&self.volume_id, rel_path],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, u64>(1)?,
                           row.get::<_, u64>(2)?, row.get::<_, String>(3)?)),
            ).ok()
        };

        let (file_id, file_size, chunk_size, local_raid) = match file_info {
            Some(info) => info,
            None => return false,
        };

        if file_size == 0 {
            return true;
        }

        let chunk_count = ((file_size - 1) / chunk_size + 1) as u32;

        // Find peers that have chunks for this file
        let peer_sources: Vec<(u32, String)> = {
            let db = self.state.db.lock().unwrap();
            let mut stmt = db.prepare(
                "SELECT fc.chunk_index, cr.node_id FROM chunk_replicas cr
                 JOIN file_chunks fc ON fc.id = cr.chunk_id
                 WHERE fc.file_id = ?1 AND cr.state = 'synced' AND cr.node_id != ?2
                 GROUP BY fc.chunk_index"
            ).unwrap();
            stmt.query_map(
                rusqlite::params![file_id, &self.state.node_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).unwrap().filter_map(|r| r.ok()).collect()
        };

        let client = PeerClient::new(&self.state.config.peer.secret);
        let volume_id = self.volume_id.clone();

        for (chunk_index, source_node_id) in &peer_sources {
            let peer_addr = match self.state.peers.get(source_node_id) {
                Some(p) => p.address.clone(),
                None => continue,
            };

            let ci = *chunk_index;
            let fid = file_id;
            let vid = volume_id.clone();
            let addr = peer_addr.clone();

            let data = self.rt.block_on(async {
                client.pull_chunk(&addr, &vid, fid, ci).await.ok()
            });

            if let Some(data) = data {
                // Store chunk locally
                let db = self.state.db.lock().unwrap();
                let placements = crate::storage::chunk::place_chunk(
                    &db, &volume_id, &self.state.node_id, ci, &local_raid,
                );
                for (backend_id, backend_path) in &placements {
                    let path = crate::storage::chunk::chunk_path(backend_path, &volume_id, fid, ci);
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    if std::fs::write(&path, &data).is_ok() {
                        // Ensure file_chunks entry exists
                        let offset = ci as u64 * chunk_size;
                        let size = chunk_size.min(file_size.saturating_sub(offset));
                        db.execute(
                            "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
                             VALUES (?1, ?2, ?3, ?4)",
                            rusqlite::params![fid, ci, offset, size],
                        ).ok();

                        if let Ok(chunk_id) = db.query_row(
                            "SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2",
                            rusqlite::params![fid, ci], |row| row.get::<_, i64>(0),
                        ) {
                            let now = chrono::Utc::now().to_rfc3339();
                            db.execute(
                                "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
                                 VALUES (?1, ?2, ?3, 'synced', ?4)",
                                rusqlite::params![chunk_id, backend_id, &self.state.node_id, &now],
                            ).ok();
                        }
                    }
                }
            }
        }

        true
    }

    /// Legacy resolve_file — kept for attr_from_path compatibility.
    /// Now checks chunk-based storage instead of file_replicas.
    fn resolve_file(&self, rel_path: &str) -> Option<PathBuf> {
        // For chunk-based storage, there's no single "file path" on disk.
        // We return None and let attr_from_db handle the metadata.
        // Actual reads go through read_chunk_data in the FUSE read() handler.
        if self.has_local_chunks(rel_path) || self.file_exists(rel_path) {
            // Return a dummy path — attr_from_db will be used for metadata
            return None;
        }
        // File not known at all
        None
    }

    /// Find a local backend that holds chunk replicas for an existing file.
    fn backend_for_existing_file(&self, rel_path: &str) -> Option<(String, String)> {
        let db = self.state.db.lock().unwrap();
        db.query_row(
            "SELECT b.id, b.path FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN file_map fm ON fm.id = fc.file_id
             WHERE fm.volume_id = ?1 AND fm.rel_path = ?2
               AND cr.node_id = ?3 AND b.status = 'online'
             LIMIT 1",
            rusqlite::params![&self.volume_id, rel_path, &self.state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok()
    }

    /// Find the best local backend for writing new files (most free space).
    fn local_backend_for_write(&self) -> Option<(String, String)> {
        let db = self.state.db.lock().unwrap();
        db.query_row(
            "SELECT id, path FROM backends
             WHERE node_id = ?1 AND status = 'online'
             ORDER BY free_bytes DESC LIMIT 1",
            rusqlite::params![&self.state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok()
    }

    /// Build FileAttr for a file or directory on disk.
    fn attr_from_path(&self, ino: u64, path: &Path, is_dir: bool) -> FileAttr {
        if let Ok(meta) = std::fs::metadata(path) {
            let kind = if meta.is_dir() { FileType::Directory } else { FileType::RegularFile };
            let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
            let atime = meta.accessed().unwrap_or(UNIX_EPOCH);
            let ctime = meta.created().unwrap_or(UNIX_EPOCH);

            FileAttr {
                ino,
                size: meta.len(),
                blocks: (meta.len() + 511) / 512,
                atime,
                mtime,
                ctime,
                crtime: ctime,
                kind,
                perm: if meta.is_dir() { 0o755 } else { 0o644 },
                nlink: if meta.is_dir() { 2 } else { 1 },
                uid: unsafe { libc::getuid() },
                gid: unsafe { libc::getgid() },
                rdev: 0,
                blksize: BLOCK_SIZE,
                flags: 0,
            }
        } else {
            // File not on local disk — return metadata from DB
            self.attr_from_db(ino, is_dir)
        }
    }

    /// Build FileAttr from database metadata (for files not stored locally).
    fn attr_from_db(&self, ino: u64, is_dir: bool) -> FileAttr {
        let entry = self.inodes.lock().unwrap().get(ino).cloned();
        let (size, mtime_str) = if let Some(ref e) = entry {
            let db = self.state.db.lock().unwrap();
            db.query_row(
                "SELECT size_bytes, updated_at FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                rusqlite::params![&e.volume_id, &e.rel_path],
                |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
            ).unwrap_or((0, String::new()))
        } else {
            (0, String::new())
        };

        let now = SystemTime::now();
        FileAttr {
            ino,
            size,
            blocks: (size + 511) / 512,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: if is_dir { FileType::Directory } else { FileType::RegularFile },
            perm: if is_dir { 0o755 } else { 0o644 },
            nlink: if is_dir { 2 } else { 1 },
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: BLOCK_SIZE,
            flags: 0,
        }
    }
}

impl Filesystem for CoreSanFS {
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let parent_entry = match self.inodes.lock().unwrap().get(parent).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        let name_str = name.to_string_lossy();
        let rel_path = if parent_entry.rel_path.is_empty() {
            name_str.to_string()
        } else {
            format!("{}/{}", parent_entry.rel_path, name_str)
        };

        tracing::debug!("FUSE lookup: '{}'", rel_path);

        // Check if it's a directory (exists on any backend)
        let is_dir = {
            let db = self.state.db.lock().unwrap();
            // Check if there are files with this prefix (it's a directory)
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM file_map WHERE volume_id = ?1 AND rel_path LIKE ?2",
                rusqlite::params![&self.volume_id, format!("{}/%", rel_path)],
                |row| row.get(0),
            ).unwrap_or(0);
            count > 0
        };

        // Check if it's a file
        let is_file = {
            let db = self.state.db.lock().unwrap();
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                rusqlite::params![&self.volume_id, &rel_path],
                |row| row.get(0),
            ).unwrap_or(0);
            count > 0
        };

        // Also check if chunks exist locally (file data present as chunks)
        let has_chunks = self.has_local_chunks(&rel_path);

        if !is_dir && !is_file && !has_chunks {
            tracing::debug!("FUSE lookup: '{}' → ENOENT (dir={}, file={}, chunks={})", rel_path, is_dir, is_file, has_chunks);
            reply.error(libc::ENOENT);
            return;
        }
        tracing::debug!("FUSE lookup: '{}' → found (dir={}, file={}, chunks={})", rel_path, is_dir, is_file, has_chunks);

        let actual_is_dir = is_dir && !is_file;
        let ino = self.inodes.lock().unwrap().get_or_create(&self.volume_id, &rel_path, actual_is_dir);

        // All metadata comes from the database — chunks are on disk but not as single files
        reply.entry(&TTL, &self.attr_from_db(ino, actual_is_dir), 0);
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let entry = match self.inodes.lock().unwrap().get(ino).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        // All metadata comes from DB — no single-file paths on disk anymore
        reply.attr(&TTL, &self.attr_from_db(ino, entry.is_dir));
    }

    fn setattr(
        &mut self, _req: &Request<'_>, ino: u64, _mode: Option<u32>,
        _uid: Option<u32>, _gid: Option<u32>, size: Option<u64>,
        _atime: Option<fuser::TimeOrNow>, _mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<SystemTime>, _fh: Option<u64>, _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>, _bkuptime: Option<SystemTime>,
        _flags: Option<u32>, reply: ReplyAttr,
    ) {
        let entry = match self.inodes.lock().unwrap().get(ino).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        // Handle truncate/extend (size change via ftruncate) — update file_map size
        if let Some(new_size) = size {
            tracing::info!("FUSE setattr: '{}' size={} -> {}", entry.rel_path, 0, new_size);
            if !entry.is_dir {
                let db = self.state.db.lock().unwrap();
                let now = chrono::Utc::now().to_rfc3339();
                // Use UPSERT — the file_map row may not exist yet if create() hasn't committed
                log_err!(db.execute(
                    "INSERT INTO file_map (volume_id, rel_path, size_bytes, version, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 0, ?4, ?4)
                     ON CONFLICT(volume_id, rel_path) DO UPDATE SET
                        size_bytes = ?3, updated_at = ?4",
                    rusqlite::params![&self.volume_id, &entry.rel_path, new_size as i64, &now],
                ), "FUSE setattr: update size");
            }
        }

        // All metadata comes from DB
        reply.attr(&TTL, &self.attr_from_db(ino, entry.is_dir));
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        let exists = self.inodes.lock().unwrap().get(ino).is_some();
        if exists {
            // Return fh=0, direct_io=false — we handle caching
            reply.opened(0, 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn flush(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        // Flush dirty chunks for this file to disk
        if let Some(entry) = self.inodes.lock().unwrap().get(ino).cloned() {
            if !entry.is_dir {
                if let Some(file_id) = self.get_file_id(&entry.rel_path) {
                    self.flush_file_chunks(file_id);

                    // Trigger replication after flush
                    let version = {
                        let db = self.state.db.lock().unwrap();
                        db.query_row("SELECT version FROM file_map WHERE id = ?1",
                            rusqlite::params![file_id], |row| row.get::<_, i64>(0)).unwrap_or(0)
                    };
                    let _ = self.state.write_tx.send(crate::engine::push_replicator::WriteEvent {
                        volume_id: self.volume_id.clone(),
                        rel_path: entry.rel_path.clone(),
                        file_id,
                        version,
                        writer_node_id: self.state.node_id.clone(),
                    });
                }
            }
        }
        reply.ok();
    }

    fn release(
        &mut self, _req: &Request<'_>, ino: u64, _fh: u64, _flags: i32,
        _lock_owner: Option<u64>, _flush: bool, reply: ReplyEmpty,
    ) {
        // Flush + release write lease when file handle is closed
        if let Some(entry) = self.inodes.lock().unwrap().get(ino).cloned() {
            if !entry.is_dir {
                if let Some(file_id) = self.get_file_id(&entry.rel_path) {
                    self.flush_file_chunks(file_id);
                }
                let db = self.state.db.lock().unwrap();
                crate::engine::write_lease::release_lease(
                    &db, &self.volume_id, &entry.rel_path, &self.state.node_id,
                );
            }
        }
        reply.ok();
    }

    fn read(
        &mut self, _req: &Request<'_>, ino: u64, _fh: u64,
        offset: i64, size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData,
    ) {
        let entry = match self.inodes.lock().unwrap().get(ino).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        if entry.is_dir {
            reply.error(libc::EISDIR);
            return;
        }

        tracing::debug!("FUSE read: '{}' offset={} size={}", entry.rel_path, offset, size);

        let file_id = match self.get_file_id(&entry.rel_path) {
            Some(id) => id,
            None => { reply.data(&[]); return; }
        };

        // Check if we have local chunks — if not, fetch from peers first
        if !self.has_local_chunks(&entry.rel_path) {
            tracing::debug!("FUSE read: no local chunks for '{}', fetching from peers", entry.rel_path);
            self.fetch_chunks_from_peer(&entry.rel_path);
        }

        let (chunk_size, _, _) = self.volume_config();

        // Check write cache first — may have unflushed data
        let ranges = crate::storage::chunk::affected_chunks(offset as u64, size as u64, chunk_size);
        let cache = self.chunk_cache.lock().unwrap();
        let has_cached = ranges.iter().any(|r| cache.contains_key(&(file_id, r.chunk_index)));
        drop(cache);

        if has_cached {
            // Read from cache for cached chunks, disk for others
            let mut result = Vec::with_capacity(size as usize);
            let cache = self.chunk_cache.lock().unwrap();
            for range in &ranges {
                let key = (file_id, range.chunk_index);
                if let Some(buf) = cache.get(&key) {
                    let start = range.local_offset as usize;
                    let end = (start + range.size as usize).min(buf.data.len());
                    if start < buf.data.len() {
                        result.extend_from_slice(&buf.data[start..end]);
                        if (end - start) < range.size as usize {
                            result.extend(std::iter::repeat(0u8).take(range.size as usize - (end - start)));
                        }
                    } else {
                        result.extend(std::iter::repeat(0u8).take(range.size as usize));
                    }
                } else {
                    // Read this chunk from disk
                    drop(cache);
                    let db = self.state.db.lock().unwrap();
                    match crate::storage::chunk::read_chunk_data(
                        &db, file_id, range.chunk_index as u64 * chunk_size + range.local_offset,
                        range.size, &self.volume_id, &self.state.node_id, chunk_size,
                    ) {
                        Ok(d) => result.extend_from_slice(&d),
                        Err(_) => result.extend(std::iter::repeat(0u8).take(range.size as usize)),
                    }
                    drop(db);
                    let cache2 = self.chunk_cache.lock().unwrap();
                    // Reborrow for next iteration — need to re-enter the pattern
                    // This is ugly but safe
                    std::mem::drop(cache2);
                    break; // Fall back to full disk read for remaining
                }
            }
            if result.len() < size as usize {
                // Fallback: read rest from disk
                let remaining_offset = offset as u64 + result.len() as u64;
                let remaining_size = size as u64 - result.len() as u64;
                let db = self.state.db.lock().unwrap();
                if let Ok(d) = crate::storage::chunk::read_chunk_data(
                    &db, file_id, remaining_offset, remaining_size,
                    &self.volume_id, &self.state.node_id, chunk_size,
                ) {
                    result.extend_from_slice(&d);
                }
            }
            reply.data(&result);
        } else {
            // Fast path: no cached chunks, read directly from disk
            let db = self.state.db.lock().unwrap();
            match crate::storage::chunk::read_chunk_data(
                &db, file_id, offset as u64, size as u64,
                &self.volume_id, &self.state.node_id, chunk_size,
            ) {
                Ok(data) => reply.data(&data),
                Err(e) => {
                    tracing::warn!("FUSE read error: {}", e);
                    reply.error(libc::EIO);
                }
            }
        }
    }

    fn write(
        &mut self, _req: &Request<'_>, ino: u64, _fh: u64,
        offset: i64, data: &[u8], _write_flags: u32, _flags: i32,
        _lock_owner: Option<u64>, reply: ReplyWrite,
    ) {
        let entry = match self.inodes.lock().unwrap().get(ino).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        if entry.is_dir {
            reply.error(libc::EISDIR);
            return;
        }

        tracing::trace!("FUSE write: '{}' offset={} len={}", entry.rel_path, offset, data.len());

        let (chunk_size, _ftt, _local_raid) = self.volume_config();

        // Ensure file exists in file_map + acquire write lease
        let file_id = {
            let db = self.state.db.lock().unwrap();

            let quorum = *self.state.quorum_status.read().unwrap();
            match crate::engine::write_lease::acquire_lease(
                &db, &self.volume_id, &entry.rel_path, &self.state.node_id, quorum,
            ) {
                crate::engine::write_lease::LeaseResult::Acquired { .. } |
                crate::engine::write_lease::LeaseResult::Renewed { .. } => {}
                crate::engine::write_lease::LeaseResult::Denied { owner_node_id, .. } => {
                    tracing::warn!("FUSE write denied: owned by {}", owner_node_id);
                    reply.error(libc::EACCES);
                    return;
                }
            }

            match db.query_row(
                "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                rusqlite::params![&self.volume_id, &entry.rel_path],
                |row| row.get::<_, i64>(0),
            ) {
                Ok(id) => id,
                Err(_) => {
                    // Create file_map entry
                    let now = chrono::Utc::now().to_rfc3339();
                    db.execute(
                        "INSERT INTO file_map (volume_id, rel_path, size_bytes, version, created_at, updated_at)
                         VALUES (?1, ?2, 0, 1, ?3, ?3)",
                        rusqlite::params![&self.volume_id, &entry.rel_path, &now],
                    ).ok();
                    db.query_row(
                        "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                        rusqlite::params![&self.volume_id, &entry.rel_path],
                        |row| row.get(0),
                    ).unwrap_or(0)
                }
            }
        };

        if file_id == 0 {
            reply.error(libc::EIO);
            return;
        }

        // Write to in-memory chunk cache (fast path — no disk I/O on cache hit)
        {
            let ranges = crate::storage::chunk::affected_chunks(offset as u64, data.len() as u64, chunk_size);
            let mut data_offset = 0usize;

            for range in &ranges {
                let key = (file_id, range.chunk_index);

                // Check if we need to load the chunk into cache (do DB work BEFORE locking cache)
                let needs_load = !self.chunk_cache.lock().unwrap().contains_key(&key);

                if needs_load {
                    // Load or create the chunk buffer WITHOUT holding the cache lock
                    let has_data = {
                        let db = self.state.db.lock().unwrap();
                        db.query_row(
                            "SELECT COUNT(*) FROM chunk_replicas cr
                             JOIN file_chunks fc ON fc.id = cr.chunk_id
                             WHERE fc.file_id = ?1 AND fc.chunk_index = ?2
                               AND cr.node_id = ?3 AND cr.state = 'synced'",
                            rusqlite::params![file_id, range.chunk_index, &self.state.node_id],
                            |row| row.get::<_, i64>(0),
                        ).unwrap_or(0) > 0
                    };

                    let chunk_data = if has_data {
                        let db = self.state.db.lock().unwrap();
                        crate::storage::chunk::read_chunk_data(
                            &db, file_id, range.chunk_index as u64 * chunk_size, chunk_size,
                            &self.volume_id, &self.state.node_id, chunk_size,
                        ).unwrap_or_else(|_| vec![0u8; chunk_size as usize])
                    } else {
                        vec![0u8; chunk_size as usize]
                    };

                    let now = std::time::Instant::now();
                    self.chunk_cache.lock().unwrap().entry(key).or_insert(
                        ChunkBuffer { data: chunk_data, dirty: false, last_write: now, first_dirty: now }
                    );
                }

                // Now patch data — only cache lock held, no DB lock
                let mut cache = self.chunk_cache.lock().unwrap();
                if let Some(buf) = cache.get_mut(&key) {
                    let end = range.local_offset as usize + range.size as usize;
                    if buf.data.len() < end {
                        buf.data.resize(end, 0);
                    }
                    buf.data[range.local_offset as usize..end]
                        .copy_from_slice(&data[data_offset..data_offset + range.size as usize]);
                    if !buf.dirty {
                        buf.first_dirty = std::time::Instant::now();
                    }
                    buf.dirty = true;
                    buf.last_write = std::time::Instant::now();
                }
                data_offset += range.size as usize;
            }

            // File size update happens at flush time, not on every write
        }

        // Evict old chunks if cache is full + flush stale chunks (>5s idle)
        self.evict_if_needed();
        self.flush_stale_chunks();

        reply.written(data.len() as u32);
    }

    fn create(
        &mut self, _req: &Request<'_>, parent: u64, name: &OsStr,
        _mode: u32, _umask: u32, _flags: i32, reply: ReplyCreate,
    ) {
        let parent_entry = match self.inodes.lock().unwrap().get(parent).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        let name_str = name.to_string_lossy();
        let rel_path = if parent_entry.rel_path.is_empty() {
            name_str.to_string()
        } else {
            format!("{}/{}", parent_entry.rel_path, name_str)
        };

        tracing::info!("FUSE create: '{}'", rel_path);

        // Register in file_map only — chunks are created on first write
        let now = chrono::Utc::now().to_rfc3339();
        let db = self.state.db.lock().unwrap();
        db.execute(
            "INSERT OR IGNORE INTO file_map (volume_id, rel_path, size_bytes, version, created_at, updated_at)
             VALUES (?1, ?2, 0, 0, ?3, ?3)",
            rusqlite::params![&self.volume_id, &rel_path, &now],
        ).ok();
        drop(db);

        let ino = self.inodes.lock().unwrap().get_or_create(&self.volume_id, &rel_path, false);
        let attr = self.attr_from_db(ino, false);
        reply.created(&TTL, &attr, 0, 0, 0);
    }

    fn mkdir(
        &mut self, _req: &Request<'_>, parent: u64, name: &OsStr,
        _mode: u32, _umask: u32, reply: ReplyEntry,
    ) {
        let parent_entry = match self.inodes.lock().unwrap().get(parent).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        let name_str = name.to_string_lossy();
        let rel_path = if parent_entry.rel_path.is_empty() {
            name_str.to_string()
        } else {
            format!("{}/{}", parent_entry.rel_path, name_str)
        };

        // Create directory on all local backends for this volume
        let backends: Vec<String> = {
            let db = self.state.db.lock().unwrap();
            let mut stmt = db.prepare(
                "SELECT path FROM backends WHERE node_id = ?1 AND status = 'online'"
            ).unwrap();
            stmt.query_map(
                rusqlite::params![&self.state.node_id],
                |row| row.get(0),
            ).unwrap().filter_map(|r| r.ok()).collect()
        };

        for bp in &backends {
            let dir = Path::new(bp).join(&rel_path);
            std::fs::create_dir_all(&dir).ok();
        }

        let ino = self.inodes.lock().unwrap().get_or_create(&self.volume_id, &rel_path, true);
        reply.entry(&TTL, &self.attr_from_db(ino, true), 0);
    }

    fn getxattr(&mut self, _req: &Request<'_>, _ino: u64, _name: &OsStr, _size: u32, reply: fuser::ReplyXattr) {
        reply.error(libc::ENODATA); // No extended attributes supported
    }

    fn listxattr(&mut self, _req: &Request<'_>, _ino: u64, _size: u32, reply: fuser::ReplyXattr) {
        reply.size(0); // No xattrs
    }

    fn opendir(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        let exists = self.inodes.lock().unwrap().get(ino).is_some();
        if exists {
            reply.opened(0, 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn releasedir(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _flags: i32, reply: ReplyEmpty) {
        reply.ok();
    }

    fn access(&mut self, _req: &Request<'_>, ino: u64, _mask: i32, reply: ReplyEmpty) {
        let exists = self.inodes.lock().unwrap().get(ino).is_some();
        if exists {
            reply.ok();
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let parent_entry = match self.inodes.lock().unwrap().get(parent).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        let name_str = name.to_string_lossy();
        let rel_path = if parent_entry.rel_path.is_empty() {
            name_str.to_string()
        } else {
            format!("{}/{}", parent_entry.rel_path, name_str)
        };

        tracing::info!("FUSE unlink: {}/{}", self.volume_id, rel_path);

        // Remove from DB and delete chunk files
        {
            let db = self.state.db.lock().unwrap();

            let file_id: Option<i64> = db.query_row(
                "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                rusqlite::params![&self.volume_id, &rel_path],
                |row| row.get(0),
            ).ok();

            if let Some(fid) = file_id {
                // Delete physical chunk files on local backends
                let chunk_files: Vec<(u32, String)> = {
                    let mut stmt = db.prepare(
                        "SELECT fc.chunk_index, b.path FROM chunk_replicas cr
                         JOIN file_chunks fc ON fc.id = cr.chunk_id
                         JOIN backends b ON b.id = cr.backend_id
                         WHERE fc.file_id = ?1 AND cr.node_id = ?2"
                    ).unwrap();
                    stmt.query_map(
                        rusqlite::params![fid, &self.state.node_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    ).unwrap().filter_map(|r| r.ok()).collect()
                };

                for (chunk_index, backend_path) in &chunk_files {
                    let path = crate::storage::chunk::chunk_path(backend_path, &self.volume_id, fid, *chunk_index);
                    std::fs::remove_file(&path).ok();
                    if let Some(parent) = path.parent() {
                        std::fs::remove_dir(parent).ok();
                    }
                }

                // Clean up DB records via FileService
                log_err!(crate::services::file::FileService::delete(&db, &self.volume_id, &rel_path),
                    "FUSE unlink: FileService::delete");
            }

            tracing::info!("FUSE unlink: deleted '{}' (chunks removed)", rel_path);
        }

        // Remove inode
        let rel_path_clone = rel_path.clone();
        let ino = self.inodes.lock().unwrap().paths
            .get(&(self.volume_id.clone(), rel_path_clone))
            .copied();
        if let Some(ino) = ino {
            self.inodes.lock().unwrap().remove(ino);
        }

        reply.ok();
    }

    fn readdir(
        &mut self, _req: &Request<'_>, ino: u64, _fh: u64,
        offset: i64, mut reply: ReplyDirectory,
    ) {
        let entry = match self.inodes.lock().unwrap().get(ino).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        if !entry.is_dir {
            reply.error(libc::ENOTDIR);
            return;
        }

        let mut entries = vec![
            (ino, FileType::Directory, ".".to_string()),
            (1, FileType::Directory, "..".to_string()), // parent = root for simplicity
        ];

        // Query files and subdirectories at this level
        let prefix = if entry.rel_path.is_empty() {
            String::new()
        } else {
            format!("{}/", entry.rel_path)
        };

        let children: Vec<(String, bool)> = {
            let db = self.state.db.lock().unwrap();

            // Get direct children files
            let pattern = if prefix.is_empty() {
                "%".to_string()
            } else {
                format!("{}%", prefix)
            };

            let mut stmt = db.prepare(
                "SELECT rel_path FROM file_map WHERE volume_id = ?1 AND rel_path LIKE ?2"
            ).unwrap();

            let all_paths: Vec<String> = stmt.query_map(
                rusqlite::params![&self.volume_id, &pattern],
                |row| row.get(0),
            ).unwrap().filter_map(|r| r.ok()).collect();

            // Extract direct children (one level deep)
            let mut seen = std::collections::HashSet::new();
            let mut result = Vec::new();
            for path in all_paths {
                let suffix = if prefix.is_empty() {
                    path.as_str()
                } else if let Some(s) = path.strip_prefix(&prefix) {
                    s
                } else {
                    continue;
                };

                if let Some(slash_pos) = suffix.find('/') {
                    // This is a subdirectory
                    let dir_name = &suffix[..slash_pos];
                    if seen.insert(dir_name.to_string()) {
                        result.push((dir_name.to_string(), true));
                    }
                } else if !suffix.is_empty() {
                    // This is a direct file
                    if seen.insert(suffix.to_string()) {
                        result.push((suffix.to_string(), false));
                    }
                }
            }

            // file_map is the single source of truth — no disk scanning needed
            // (chunks are stored under .coresan/ structure, not as flat files)
            result
        };

        for (name, is_dir) in children {
            let child_path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}{}", prefix, name)
            };
            let child_ino = self.inodes.lock().unwrap()
                .get_or_create(&self.volume_id, &child_path, is_dir);
            let kind = if is_dir { FileType::Directory } else { FileType::RegularFile };
            entries.push((child_ino, kind, name));
        }

        for (i, (ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
            if reply.add(*ino, (i + 1) as i64, *kind, name) {
                break; // buffer full
            }
        }

        reply.ok();
    }
}

// ── Mount/Unmount ────────────────────────────────────────────────────────

/// Active FUSE session handle, stored so we can unmount later.
struct FuseSession {
    _volume_name: String,
    mount_path: PathBuf,
    // The FUSE background session thread handle
    _thread: std::thread::JoinHandle<()>,
}

/// Spawn FUSE mounts for all online volumes.
pub fn spawn_all(state: Arc<CoreSanState>) {
    let volumes: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name FROM volumes WHERE status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    let rt = tokio::runtime::Handle::current();

    for (vol_id, vol_name) in volumes {
        let fuse_path = state.config.data.fuse_root.join(&vol_name);

        // Clean up stale/dead FUSE mounts from previous crashed runs
        if std::fs::metadata(&fuse_path).is_err() && fuse_path.as_os_str().len() > 0 {
            // Path exists on disk but stat fails → dead FUSE endpoint
            tracing::info!("Cleaning up stale FUSE mount: {}", fuse_path.display());
            std::process::Command::new("fusermount3")
                .args(["-u", &fuse_path.to_string_lossy()])
                .output().ok();
            std::process::Command::new("umount")
                .args(["-l", &fuse_path.to_string_lossy()])
                .output().ok();
        }

        // Ensure mount point directory exists
        std::fs::create_dir_all(&fuse_path).ok();

        let fs_name = format!("coresan:{}", vol_name);
        let mount_path = fuse_path.clone();
        let vol_name_clone = vol_name.clone();

        // Check if user_allow_other is enabled in /etc/fuse.conf
        let allow_other = std::fs::read_to_string("/etc/fuse.conf")
            .map(|c| c.lines().any(|l| {
                let trimmed = l.trim();
                trimmed == "user_allow_other" && !trimmed.starts_with('#')
            }))
            .unwrap_or(false) || unsafe { libc::getuid() } == 0;

        let state_clone = Arc::clone(&state);
        let rt_clone = rt.clone();

        let thread = std::thread::spawn(move || {
            let mut options = vec![
                MountOption::FSName(fs_name),
            ];
            if allow_other {
                options.push(MountOption::AllowOther);
                options.push(MountOption::AutoUnmount);
            }

            match fuser::mount2(CoreSanFS::new(state_clone, vol_id, rt_clone), &mount_path, &options) {
                Ok(_) => tracing::info!("FUSE unmounted: {}", mount_path.display()),
                Err(e) => tracing::error!("FUSE mount failed for {}: {}", mount_path.display(), e),
            }
        });

        tracing::info!("FUSE mounted: {} (volume '{}'{})",
            fuse_path.display(), vol_name_clone,
            if allow_other { ", allow_other" } else { "" });

        // Store handle — in the future we'll track these for cleanup
        let _ = FuseSession {
            _volume_name: vol_name,
            mount_path: fuse_path,
            _thread: thread,
        };
    }
}

/// Unmount all FUSE filesystems (called during graceful shutdown).
pub fn unmount_all(state: &CoreSanState) {
    let names: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT name FROM volumes WHERE status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    for name in names {
        let fuse_path = state.config.data.fuse_root.join(&name);
        tracing::info!("Unmounting FUSE: {}", fuse_path.display());
        // fusermount3 -u to cleanly unmount
        std::process::Command::new("fusermount3")
            .args(["-u", &fuse_path.to_string_lossy()])
            .output()
            .ok();
    }
}
