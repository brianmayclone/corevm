//! Local backend abstraction — manages files on a local mountpoint.

use std::path::{Path, PathBuf};

/// Refresh filesystem stats for a backend path.
pub fn refresh_stats(path: &str) -> (u64, u64) {
    use std::ffi::CString;
    let c_path = match CString::new(path) {
        Ok(p) => p,
        Err(_) => return (0, 0),
    };

    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
            let total = stat.f_blocks as u64 * stat.f_frsize as u64;
            let free = stat.f_bavail as u64 * stat.f_frsize as u64;
            (total, free)
        } else {
            (0, 0)
        }
    }
}

/// Check if a backend path is accessible and writable.
pub fn is_healthy(path: &str) -> bool {
    let p = Path::new(path);
    if !p.exists() || !p.is_dir() {
        return false;
    }
    // Try to create and remove a test file
    let test_file = p.join(".coresan_health_check");
    match std::fs::write(&test_file, b"ok") {
        Ok(_) => {
            std::fs::remove_file(&test_file).ok();
            true
        }
        Err(_) => false,
    }
}

/// Build the full filesystem path for a file on a backend.
pub fn file_path(backend_path: &str, rel_path: &str) -> PathBuf {
    Path::new(backend_path).join(rel_path)
}

/// List all files recursively under a backend path, returning relative paths.
pub fn list_files(backend_path: &str) -> Vec<String> {
    let base = Path::new(backend_path);
    let mut files = Vec::new();
    collect_files(base, base, &mut files);
    files
}

fn collect_files(base: &Path, current: &Path, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip hidden files and health check files
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }
        if path.is_dir() {
            collect_files(base, &path, out);
        } else if let Ok(rel) = path.strip_prefix(base) {
            if let Some(s) = rel.to_str() {
                out.push(s.to_string());
            }
        }
    }
}
