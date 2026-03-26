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

const TTL: Duration = Duration::from_secs(1);
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

/// CoreSAN FUSE filesystem — one instance per mounted volume.
pub struct CoreSanFS {
    state: Arc<CoreSanState>,
    volume_id: String,
    inodes: Mutex<InodeMap>,
    /// Tokio runtime handle for blocking on async peer operations.
    rt: tokio::runtime::Handle,
}

impl CoreSanFS {
    fn new(state: Arc<CoreSanState>, volume_id: String, rt: tokio::runtime::Handle) -> Self {
        let inodes = Mutex::new(InodeMap::new(&volume_id));
        CoreSanFS { state, volume_id, inodes, rt }
    }

    /// Find the local filesystem path for a file, or fetch from peer.
    /// Returns the path to the local file (possibly a cache copy fetched from a peer).
    fn resolve_file(&self, rel_path: &str) -> Option<PathBuf> {
        // Try local replica first
        let local = {
            let db = self.state.db.lock().unwrap();
            db.query_row(
                "SELECT b.path FROM file_replicas fr
                 JOIN backends b ON b.id = fr.backend_id
                 JOIN file_map fm ON fm.id = fr.file_id
                 WHERE fm.volume_id = ?1 AND fm.rel_path = ?2
                   AND b.node_id = ?3 AND fr.state = 'synced'
                 LIMIT 1",
                rusqlite::params![&self.volume_id, rel_path, &self.state.node_id],
                |row| row.get::<_, String>(0),
            ).ok()
        };

        if let Some(backend_path) = local {
            let full = Path::new(&backend_path).join(rel_path);
            if full.exists() {
                return Some(full);
            }
        }

        // No local replica — fetch from peer
        self.fetch_from_peer(rel_path)
    }

    /// Fetch a file from a peer and store it on a local backend.
    fn fetch_from_peer(&self, rel_path: &str) -> Option<PathBuf> {
        // Find which peer has this file
        let peer_info = {
            let db = self.state.db.lock().unwrap();
            db.query_row(
                "SELECT b.node_id FROM file_replicas fr
                 JOIN backends b ON b.id = fr.backend_id
                 JOIN file_map fm ON fm.id = fr.file_id
                 WHERE fm.volume_id = ?1 AND fm.rel_path = ?2
                   AND fr.state = 'synced' AND b.node_id != ?3
                 LIMIT 1",
                rusqlite::params![&self.volume_id, rel_path, &self.state.node_id],
                |row| row.get::<_, String>(0),
            ).ok()
        };

        let peer_node_id = peer_info?;
        let peer_addr = self.state.peers.get(&peer_node_id)?.address.clone();

        // Pull file from peer (blocking on async)
        let client = PeerClient::new(&self.state.config.peer.secret);
        let volume_id = self.volume_id.clone();
        let rel = rel_path.to_string();

        let data = self.rt.block_on(async {
            client.pull_file(&peer_addr, &volume_id, &rel).await.ok()
        })?;

        // Find a local backend to cache the file on
        let local_backend = {
            let db = self.state.db.lock().unwrap();
            db.query_row(
                "SELECT id, path FROM backends
                 WHERE volume_id = ?1 AND node_id = ?2 AND status = 'online'
                 ORDER BY free_bytes DESC LIMIT 1",
                rusqlite::params![&self.volume_id, &self.state.node_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            ).ok()
        };

        if let Some((backend_id, backend_path)) = local_backend {
            let full_path = Path::new(&backend_path).join(rel_path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            if std::fs::write(&full_path, &data).is_ok() {
                // Register as synced replica
                let db = self.state.db.lock().unwrap();
                let now = chrono::Utc::now().to_rfc3339();

                // Get file_id
                if let Ok(file_id) = db.query_row(
                    "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                    rusqlite::params![&self.volume_id, rel_path],
                    |row| row.get::<_, i64>(0),
                ) {
                    db.execute(
                        "INSERT OR REPLACE INTO file_replicas (file_id, backend_id, state, synced_at)
                         VALUES (?1, ?2, 'synced', ?3)",
                        rusqlite::params![file_id, &backend_id, &now],
                    ).ok();
                }

                tracing::debug!("FUSE: fetched {}/{} from peer {}", self.volume_id, rel_path, peer_node_id);
                return Some(full_path);
            }
        }

        // No local backend to store it, serve from memory via a temp file
        let tmp = std::env::temp_dir().join(format!("coresan-{}", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, &data).ok()?;
        Some(tmp)
    }

    /// Find the best local backend path for writing new files.
    fn local_backend_for_write(&self) -> Option<(String, String)> {
        let db = self.state.db.lock().unwrap();
        db.query_row(
            "SELECT id, path FROM backends
             WHERE volume_id = ?1 AND node_id = ?2 AND status = 'online'
             ORDER BY free_bytes DESC LIMIT 1",
            rusqlite::params![&self.volume_id, &self.state.node_id],
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

        // Also check local filesystem directly (for files not yet in DB)
        let exists_on_disk = {
            let paths: Vec<String> = {
                let db = self.state.db.lock().unwrap();
                let result = db.prepare(
                    "SELECT path FROM backends WHERE volume_id = ?1 AND node_id = ?2 AND status = 'online'"
                );
                match result {
                    Ok(mut stmt) => {
                        let rows = stmt.query_map(
                            rusqlite::params![&self.volume_id, &self.state.node_id],
                            |row| row.get(0),
                        );
                        rows.map(|r| r.filter_map(|v| v.ok()).collect()).unwrap_or_default()
                    }
                    Err(_) => Vec::new(),
                }
            };
            paths.iter().any(|bp| {
                let full = Path::new(bp).join(&rel_path);
                full.exists()
            })
        };

        if !is_dir && !is_file && !exists_on_disk {
            reply.error(libc::ENOENT);
            return;
        }

        let actual_is_dir = is_dir && !is_file;
        let ino = self.inodes.lock().unwrap().get_or_create(&self.volume_id, &rel_path, actual_is_dir);

        if actual_is_dir {
            reply.entry(&TTL, &self.attr_from_db(ino, true), 0);
        } else if let Some(path) = self.resolve_file(&rel_path) {
            reply.entry(&TTL, &self.attr_from_path(ino, &path, false), 0);
        } else {
            reply.entry(&TTL, &self.attr_from_db(ino, false), 0);
        }
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let entry = match self.inodes.lock().unwrap().get(ino).cloned() {
            Some(e) => e,
            None => { reply.error(libc::ENOENT); return; }
        };

        if entry.is_dir {
            reply.attr(&TTL, &self.attr_from_db(ino, true));
        } else if let Some(path) = self.resolve_file(&entry.rel_path) {
            reply.attr(&TTL, &self.attr_from_path(ino, &path, false));
        } else {
            reply.attr(&TTL, &self.attr_from_db(ino, false));
        }
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

        match self.resolve_file(&entry.rel_path) {
            Some(path) => {
                match std::fs::read(&path) {
                    Ok(data) => {
                        let start = offset as usize;
                        let end = (start + size as usize).min(data.len());
                        if start >= data.len() {
                            reply.data(&[]);
                        } else {
                            reply.data(&data[start..end]);
                        }
                    }
                    Err(_) => reply.error(libc::EIO),
                }
            }
            None => reply.error(libc::ENOENT),
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

        let (backend_id, backend_path) = match self.local_backend_for_write() {
            Some(b) => b,
            None => { reply.error(libc::ENOSPC); return; }
        };

        // Atomic write with lease acquisition + write_log
        let db = self.state.db.lock().unwrap();
        match crate::engine::write_lease::atomic_write(
            &db,
            &self.volume_id,
            &entry.rel_path,
            &self.state.node_id,
            &backend_id,
            &backend_path,
            data,
            Some(offset),
        ) {
            Ok(new_version) => {
                drop(db);

                // Read the full content for push replication
                let full_path = std::path::Path::new(&backend_path).join(&entry.rel_path);
                if let Ok(content) = std::fs::read(&full_path) {
                    // Push to peers immediately via channel (non-blocking)
                    let _ = self.state.write_tx.send(crate::engine::push_replicator::WriteEvent {
                        volume_id: self.volume_id.clone(),
                        rel_path: entry.rel_path.clone(),
                        version: new_version,
                        data: std::sync::Arc::new(content),
                        writer_node_id: self.state.node_id.clone(),
                    });
                }

                reply.written(data.len() as u32);
            }
            Err(e) => {
                drop(db);
                tracing::warn!("FUSE write denied for {}/{}: {}", self.volume_id, entry.rel_path, e);
                reply.error(libc::EACCES);
            }
        }
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

        let (backend_id, backend_path) = match self.local_backend_for_write() {
            Some(b) => b,
            None => { reply.error(libc::ENOSPC); return; }
        };

        let full_path = Path::new(&backend_path).join(&rel_path);
        if let Some(parent_dir) = full_path.parent() {
            std::fs::create_dir_all(parent_dir).ok();
        }

        // Create empty file
        if std::fs::write(&full_path, b"").is_err() {
            reply.error(libc::EIO);
            return;
        }

        // Register in DB
        let now = chrono::Utc::now().to_rfc3339();
        let db = self.state.db.lock().unwrap();
        db.execute(
            "INSERT OR IGNORE INTO file_map (volume_id, rel_path, size_bytes, sha256, created_at, updated_at)
             VALUES (?1, ?2, 0, '', ?3, ?3)",
            rusqlite::params![&self.volume_id, &rel_path, &now],
        ).ok();

        if let Ok(file_id) = db.query_row(
            "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
            rusqlite::params![&self.volume_id, &rel_path],
            |row| row.get::<_, i64>(0),
        ) {
            db.execute(
                "INSERT OR REPLACE INTO file_replicas (file_id, backend_id, state, synced_at)
                 VALUES (?1, ?2, 'synced', ?3)",
                rusqlite::params![file_id, &backend_id, &now],
            ).ok();
        }
        drop(db);

        let ino = self.inodes.lock().unwrap().get_or_create(&self.volume_id, &rel_path, false);
        let attr = self.attr_from_path(ino, &full_path, false);
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
                "SELECT path FROM backends WHERE volume_id = ?1 AND node_id = ?2 AND status = 'online'"
            ).unwrap();
            stmt.query_map(
                rusqlite::params![&self.volume_id, &self.state.node_id],
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

        // Delete from local backends
        let backends: Vec<String> = {
            let db = self.state.db.lock().unwrap();
            let mut stmt = db.prepare(
                "SELECT path FROM backends WHERE volume_id = ?1 AND node_id = ?2 AND status = 'online'"
            ).unwrap();
            stmt.query_map(
                rusqlite::params![&self.volume_id, &self.state.node_id],
                |row| row.get(0),
            ).unwrap().filter_map(|r| r.ok()).collect()
        };

        for bp in &backends {
            let full = Path::new(bp).join(&rel_path);
            std::fs::remove_file(&full).ok();
        }

        // Remove from DB
        {
            let db = self.state.db.lock().unwrap();
            db.execute(
                "DELETE FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                rusqlite::params![&self.volume_id, &rel_path],
            ).ok();
        }

        // Remove inode
        let ino = self.inodes.lock().unwrap().paths
            .get(&(self.volume_id.clone(), rel_path))
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
                    result.push((suffix.to_string(), false));
                }
            }
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

        if !fuse_path.exists() {
            if let Err(e) = std::fs::create_dir_all(&fuse_path) {
                tracing::warn!("Cannot create FUSE mount point {}: {}", fuse_path.display(), e);
                continue;
            }
        }

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
