//! Storage service — pools, disk images, ISOs.

use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

pub struct StorageService;

// ── Storage Pools ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StoragePool {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub pool_type: String,
    pub shared: bool,
    pub mount_source: Option<String>,
    pub mount_opts: Option<String>,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

impl StorageService {
    pub fn list_pools(db: &Connection) -> Result<Vec<StoragePool>, String> {
        let mut stmt = db.prepare(
            "SELECT id, name, path, pool_type, shared, mount_source, mount_opts FROM storage_pools ORDER BY name"
        ).map_err(|e| e.to_string())?;
        let pools = stmt.query_map([], |row| {
            Ok((
                row.get::<_,i64>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?,
                row.get::<_,String>(3)?, row.get::<_,bool>(4)?,
                row.get::<_,Option<String>>(5)?, row.get::<_,Option<String>>(6)?,
            ))
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|(id, name, path, pool_type, shared, mount_source, mount_opts)| {
            let (total_bytes, free_bytes) = get_disk_space(&path);
            StoragePool { id, name, path, pool_type, shared, mount_source, mount_opts, total_bytes, free_bytes }
        }).collect();
        Ok(pools)
    }

    pub fn create_pool(
        db: &Connection, name: &str, path: &str, pool_type: &str,
        mount_source: Option<&str>, mount_opts: Option<&str>,
    ) -> Result<i64, String> {
        let valid_types = ["local", "nfs", "cephfs", "glusterfs"];
        if !valid_types.contains(&pool_type) {
            return Err(format!("Invalid pool_type. Must be one of: {}", valid_types.join(", ")));
        }
        let shared = pool_type != "local";
        if shared && mount_source.is_none() {
            return Err("mount_source is required for shared storage".into());
        }
        let p = Path::new(path);
        if !p.exists() {
            std::fs::create_dir_all(p).map_err(|e| format!("Cannot create directory: {}", e))?;
        }
        db.execute(
            "INSERT INTO storage_pools (name, path, pool_type, shared, mount_source, mount_opts) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![name, path, pool_type, shared, mount_source, mount_opts],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE") { "Pool path already registered".into() }
            else { e.to_string() }
        })?;
        Ok(db.last_insert_rowid())
    }

    pub fn delete_pool(db: &Connection, pool_id: i64) -> Result<(), String> {
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM disk_images WHERE pool_id = ?1", rusqlite::params![pool_id], |r| r.get(0),
        ).map_err(|e| e.to_string())?;
        if count > 0 {
            return Err(format!("{} disk images still in this pool", count));
        }
        let affected = db.execute("DELETE FROM storage_pools WHERE id = ?1", rusqlite::params![pool_id])
            .map_err(|e| e.to_string())?;
        if affected == 0 { Err("Pool not found".into()) } else { Ok(()) }
    }
}

// ── Disk Images ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DiskImage {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub size_bytes: i64,
    pub format: String,
    pub pool_id: Option<i64>,
    pub vm_id: Option<String>,
    pub created_at: String,
}

impl StorageService {
    pub fn list_images(db: &Connection) -> Result<Vec<DiskImage>, String> {
        let mut stmt = db.prepare(
            "SELECT id, name, path, size_bytes, format, pool_id, vm_id, created_at FROM disk_images ORDER BY name"
        ).map_err(|e| e.to_string())?;
        let images = stmt.query_map([], |row| {
            Ok(DiskImage {
                id: row.get(0)?, name: row.get(1)?, path: row.get(2)?,
                size_bytes: row.get(3)?, format: row.get(4)?,
                pool_id: row.get(5)?, vm_id: row.get(6)?, created_at: row.get(7)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(images)
    }

    pub fn create_image(db: &Connection, name: &str, size_gb: u64, pool_id: i64, max_size_gb: u64) -> Result<(i64, String), String> {
        if size_gb > max_size_gb {
            return Err(format!("Max disk size is {} GB", max_size_gb));
        }
        let size_bytes = size_gb * 1024 * 1024 * 1024;
        let pool_path: String = db.query_row(
            "SELECT path FROM storage_pools WHERE id = ?1", rusqlite::params![pool_id], |r| r.get(0),
        ).map_err(|_| "Storage pool not found".to_string())?;

        let filename = format!("{}.raw", name.replace(' ', "_").to_lowercase());
        let disk_path = Path::new(&pool_path).join(&filename);
        if disk_path.exists() {
            return Err("Disk image already exists".into());
        }
        let file = std::fs::File::create(&disk_path)
            .map_err(|e| format!("Create failed: {}", e))?;
        file.set_len(size_bytes)
            .map_err(|e| format!("Allocate failed: {}", e))?;

        let path_str = disk_path.to_string_lossy().to_string();
        db.execute(
            "INSERT INTO disk_images (name, path, size_bytes, format, pool_id) VALUES (?1, ?2, ?3, 'raw', ?4)",
            rusqlite::params![name, &path_str, size_bytes as i64, pool_id],
        ).map_err(|e| e.to_string())?;
        Ok((db.last_insert_rowid(), path_str))
    }

    pub fn delete_image(db: &Connection, image_id: i64) -> Result<(), String> {
        let path: String = db.query_row(
            "SELECT path FROM disk_images WHERE id = ?1", rusqlite::params![image_id], |r| r.get(0),
        ).map_err(|_| "Disk image not found".to_string())?;
        let _ = std::fs::remove_file(&path);
        db.execute("DELETE FROM disk_images WHERE id = ?1", rusqlite::params![image_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn resize_image(db: &Connection, image_id: i64, size_gb: u64) -> Result<(), String> {
        let new_size = size_gb * 1024 * 1024 * 1024;
        let (path, current_size): (String, i64) = db.query_row(
            "SELECT path, size_bytes FROM disk_images WHERE id = ?1", rusqlite::params![image_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).map_err(|_| "Disk image not found".to_string())?;

        if (new_size as i64) < current_size {
            return Err("Cannot shrink disk image (data loss risk)".into());
        }
        let file = std::fs::OpenOptions::new().write(true).open(&path)
            .map_err(|e| format!("Open failed: {}", e))?;
        file.set_len(new_size).map_err(|e| format!("Resize failed: {}", e))?;
        db.execute("UPDATE disk_images SET size_bytes = ?1 WHERE id = ?2",
            rusqlite::params![new_size as i64, image_id]).map_err(|e| e.to_string())?;
        Ok(())
    }
}

// ── ISOs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Iso {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub size_bytes: i64,
    pub uploaded_at: String,
}

impl StorageService {
    pub fn list_isos(db: &Connection) -> Result<Vec<Iso>, String> {
        let mut stmt = db.prepare("SELECT id, name, path, size_bytes, uploaded_at FROM isos ORDER BY name")
            .map_err(|e| e.to_string())?;
        let isos = stmt.query_map([], |row| {
            Ok(Iso { id: row.get(0)?, name: row.get(1)?, path: row.get(2)?,
                size_bytes: row.get(3)?, uploaded_at: row.get(4)? })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(isos)
    }

    pub fn save_iso(db: &Connection, name: &str, path: &str, size: i64) -> Result<i64, String> {
        db.execute(
            "INSERT INTO isos (name, path, size_bytes) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, path, size],
        ).map_err(|e| e.to_string())?;
        Ok(db.last_insert_rowid())
    }

    pub fn delete_iso(db: &Connection, iso_id: i64) -> Result<(), String> {
        let path: String = db.query_row(
            "SELECT path FROM isos WHERE id = ?1", rusqlite::params![iso_id], |r| r.get(0),
        ).map_err(|_| "ISO not found".to_string())?;
        let _ = std::fs::remove_file(&path);
        db.execute("DELETE FROM isos WHERE id = ?1", rusqlite::params![iso_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

// ── Pool Browsing + Auto-Create ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PoolFile {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub is_dir: bool,
}

impl StorageService {
    /// Browse files in a storage pool. Optionally filter by extension.
    pub fn browse_pool(db: &Connection, pool_id: i64, filter_ext: Option<&str>) -> Result<Vec<PoolFile>, String> {
        let pool_path: String = db.query_row(
            "SELECT path FROM storage_pools WHERE id = ?1", rusqlite::params![pool_id], |r| r.get(0),
        ).map_err(|_| "Pool not found".to_string())?;

        let mut files = Vec::new();
        Self::scan_dir(std::path::Path::new(&pool_path), &pool_path, filter_ext, &mut files, 3);
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(files)
    }

    fn scan_dir(dir: &std::path::Path, base: &str, filter_ext: Option<&str>, out: &mut Vec<PoolFile>, depth: u8) {
        if depth == 0 { return; }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path().to_string_lossy().to_string();

            if meta.is_dir() {
                out.push(PoolFile { name: name.clone(), path: path.clone(), size_bytes: 0, is_dir: true });
                Self::scan_dir(&entry.path(), base, filter_ext, out, depth - 1);
            } else {
                if let Some(ext) = filter_ext {
                    if !name.to_lowercase().ends_with(&ext.to_lowercase()) { continue; }
                }
                out.push(PoolFile { name, path, size_bytes: meta.len(), is_dir: false });
            }
        }
    }

    /// Create a disk image for a VM inside its own subdirectory in the pool.
    /// Returns the full path of the created disk.
    pub fn create_vm_disk(db: &Connection, vm_name: &str, vm_id: &str, size_gb: u64, pool_id: i64) -> Result<(i64, String), String> {
        let pool_path: String = db.query_row(
            "SELECT path FROM storage_pools WHERE id = ?1", rusqlite::params![pool_id], |r| r.get(0),
        ).map_err(|_| "Storage pool not found".to_string())?;

        // Create VM subdirectory
        let safe_name = vm_name.replace(' ', "_").replace('/', "_").to_lowercase();
        let vm_dir = std::path::Path::new(&pool_path).join(&safe_name);
        std::fs::create_dir_all(&vm_dir).map_err(|e| format!("Cannot create VM dir: {}", e))?;

        // Find next disk number
        let existing: Vec<String> = std::fs::read_dir(&vm_dir)
            .map(|entries| entries.flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n.ends_with(".raw") { Some(n) } else { None }
                }).collect())
            .unwrap_or_default();
        let disk_num = existing.len();
        let filename = if disk_num == 0 { "disk.raw".to_string() } else { format!("disk{}.raw", disk_num) };
        let disk_path = vm_dir.join(&filename);

        let size_bytes = size_gb * 1024 * 1024 * 1024;
        let file = std::fs::File::create(&disk_path).map_err(|e| format!("Create failed: {}", e))?;
        file.set_len(size_bytes).map_err(|e| format!("Allocate failed: {}", e))?;

        let path_str = disk_path.to_string_lossy().to_string();
        let disk_name = format!("{}/{}", safe_name, filename);
        // vm_id may be empty for new VMs not yet saved — store NULL to avoid FK violation
        let vm_id_param: Option<&str> = if vm_id.is_empty() { None } else { Some(vm_id) };
        db.execute(
            "INSERT INTO disk_images (name, path, size_bytes, format, pool_id, vm_id) VALUES (?1, ?2, ?3, 'raw', ?4, ?5)",
            rusqlite::params![&disk_name, &path_str, size_bytes as i64, pool_id, vm_id_param],
        ).map_err(|e| e.to_string())?;
        Ok((db.last_insert_rowid(), path_str))
    }

    /// Get aggregate stats across all pools.
    pub fn aggregate_stats(db: &Connection) -> Result<StorageStats, String> {
        let pools = Self::list_pools(db)?;
        let total_bytes: u64 = pools.iter().map(|p| p.total_bytes).sum();
        let free_bytes: u64 = pools.iter().map(|p| p.free_bytes).sum();
        let used_bytes = total_bytes.saturating_sub(free_bytes);

        let total_images: i64 = db.query_row("SELECT COUNT(*) FROM disk_images", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let image_bytes: i64 = db.query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM disk_images", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let total_isos: i64 = db.query_row("SELECT COUNT(*) FROM isos", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let orphaned: i64 = db.query_row("SELECT COUNT(*) FROM disk_images WHERE vm_id IS NULL", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;

        Ok(StorageStats {
            total_pools: pools.len() as u32,
            online_pools: pools.iter().filter(|p| p.total_bytes > 0).count() as u32,
            total_bytes, used_bytes, free_bytes,
            vm_disk_bytes: image_bytes as u64,
            total_images: total_images as u32,
            total_isos: total_isos as u32,
            orphaned_images: orphaned as u32,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct StorageStats {
    pub total_pools: u32,
    pub online_pools: u32,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub vm_disk_bytes: u64,
    pub total_images: u32,
    pub total_isos: u32,
    pub orphaned_images: u32,
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn get_disk_space(path: &str) -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            let c_path = std::ffi::CString::new(path).unwrap_or_default();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                let total = stat.f_blocks as u64 * stat.f_frsize as u64;
                let free = stat.f_bavail as u64 * stat.f_frsize as u64;
                return (total, free);
            }
        }
    }
    (0, 0)
}
