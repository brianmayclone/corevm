//! Disk operations — format, mount, unmount, reset physical disks.
//!
//! All operations run system commands (mkfs, mount, wipefs, etc.)
//! and must be executed as root.

use std::path::Path;
use std::process::Command;

/// Format a disk with the specified filesystem. Wipes existing signatures first.
/// Returns the new filesystem UUID.
pub fn format_disk(device_path: &str, fs_type: &str) -> Result<String, String> {
    // Safety: verify device exists
    if !Path::new(device_path).exists() {
        return Err(format!("Device does not exist: {}", device_path));
    }

    // Wipe existing filesystem signatures
    tracing::info!("Wiping signatures on {}", device_path);
    run_cmd("wipefs", &["-a", device_path])?;

    // Format
    tracing::info!("Formatting {} as {}", device_path, fs_type);
    match fs_type {
        "ext4" => run_cmd("mkfs.ext4", &["-F", "-q", device_path])?,
        "xfs" => run_cmd("mkfs.xfs", &["-f", device_path])?,
        _ => return Err(format!("Unsupported filesystem: {}", fs_type)),
    };

    // Get the new UUID
    let uuid = get_fs_uuid(device_path)?;
    tracing::info!("Formatted {} as {} (UUID={})", device_path, fs_type, uuid);
    Ok(uuid)
}

/// Mount a device to a path.
pub fn mount_disk(device_path: &str, mount_path: &str) -> Result<(), String> {
    std::fs::create_dir_all(mount_path)
        .map_err(|e| format!("Cannot create mount point {}: {}", mount_path, e))?;

    run_cmd("mount", &[device_path, mount_path])?;
    tracing::info!("Mounted {} → {}", device_path, mount_path);
    Ok(())
}

/// Unmount a mount path.
pub fn unmount_disk(mount_path: &str) -> Result<(), String> {
    run_cmd("umount", &[mount_path])?;
    tracing::info!("Unmounted {}", mount_path);

    // Remove mount directory
    std::fs::remove_dir(mount_path).ok();
    Ok(())
}

/// Create a systemd mount unit for persistent mounts across reboots.
pub fn create_mount_unit(mount_path: &str, device_uuid: &str, fs_type: &str) -> Result<(), String> {
    let unit_name = mount_path_to_unit_name(mount_path);
    let unit_path = format!("/etc/systemd/system/{}.mount", unit_name);

    let content = format!(
        "[Unit]\n\
         Description=CoreSAN disk mount {mount_path}\n\
         After=local-fs.target\n\
         \n\
         [Mount]\n\
         What=/dev/disk/by-uuid/{uuid}\n\
         Where={mount_path}\n\
         Type={fs_type}\n\
         Options=defaults,noatime\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        mount_path = mount_path,
        uuid = device_uuid,
        fs_type = fs_type,
    );

    std::fs::write(&unit_path, content)
        .map_err(|e| format!("Cannot write mount unit {}: {}", unit_path, e))?;

    run_cmd("systemctl", &["daemon-reload"])?;
    run_cmd("systemctl", &["enable", &format!("{}.mount", unit_name)])?;

    tracing::info!("Created mount unit: {}", unit_path);
    Ok(())
}

/// Remove a systemd mount unit.
pub fn remove_mount_unit(mount_path: &str) -> Result<(), String> {
    let unit_name = mount_path_to_unit_name(mount_path);
    let unit_path = format!("/etc/systemd/system/{}.mount", unit_name);

    run_cmd("systemctl", &["disable", &format!("{}.mount", unit_name)]).ok();

    if Path::new(&unit_path).exists() {
        std::fs::remove_file(&unit_path).ok();
    }

    run_cmd("systemctl", &["daemon-reload"]).ok();
    tracing::info!("Removed mount unit: {}", unit_path);
    Ok(())
}

/// Wipe filesystem signatures from a disk (makes it "available" again).
pub fn wipe_disk(device_path: &str) -> Result<(), String> {
    if !Path::new(device_path).exists() {
        return Err(format!("Device does not exist: {}", device_path));
    }
    run_cmd("wipefs", &["-a", device_path])?;
    tracing::info!("Wiped signatures on {}", device_path);
    Ok(())
}

/// Full reset of a disk: wipe signatures + zero first 10MB (destroys partition table).
pub fn reset_disk(device_path: &str) -> Result<(), String> {
    if !Path::new(device_path).exists() {
        return Err(format!("Device does not exist: {}", device_path));
    }

    // Wipe filesystem signatures
    run_cmd("wipefs", &["-a", device_path])?;

    // Zero the first 10MB to destroy partition tables and filesystem headers
    run_cmd("dd", &[
        &format!("if=/dev/zero"),
        &format!("of={}", device_path),
        "bs=1M", "count=10", "conv=notrunc",
    ])?;

    tracing::info!("Reset disk {} (signatures wiped + first 10MB zeroed)", device_path);
    Ok(())
}

/// Get the filesystem UUID of a device via blkid.
pub fn get_fs_uuid(device_path: &str) -> Result<String, String> {
    let output = Command::new("blkid")
        .args(["-s", "UUID", "-o", "value", device_path])
        .output()
        .map_err(|e| format!("blkid failed: {}", e))?;

    if !output.status.success() {
        return Err("blkid returned no UUID".into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if a device is currently mounted (appears in /proc/mounts).
pub fn is_mounted(device_path: &str) -> bool {
    std::fs::read_to_string("/proc/mounts")
        .map(|content| content.lines().any(|line| line.starts_with(device_path)))
        .unwrap_or(false)
}

/// Convert a mount path to a systemd unit name.
/// /vmm/san-disks/abc-123 → vmm-san\\x2ddisks-abc\\x2d123
fn mount_path_to_unit_name(path: &str) -> String {
    // systemd-escape --path
    let output = Command::new("systemd-escape")
        .args(["--path", path])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => {
            // Fallback: manual escaping
            path.trim_start_matches('/')
                .replace('/', "-")
        }
    }
}

/// Run a command and return Ok/Err based on exit status.
fn run_cmd(cmd: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run {}: {}", cmd, e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{} failed: {}", cmd, stderr.trim()))
    }
}
