//! Disk discovery — detects block devices and classifies their availability.
//!
//! Uses `lsblk` to enumerate all disks (not partitions) and cross-references
//! with claimed_disks table and /proc/mounts to determine status.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::process::Command;

/// A physical block device discovered on this node.
#[derive(Clone, Debug, Serialize)]
pub struct BlockDevice {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub model: String,
    pub serial: String,
    pub fs_type: Option<String>,
    pub mountpoint: Option<String>,
    pub has_partitions: bool,
    pub is_os_disk: bool,
}

/// Availability classification for a discovered disk.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status")]
pub enum DiskStatus {
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "has_data")]
    HasData { fs_type: String },
    #[serde(rename = "os_disk")]
    OsDisk,
    #[serde(rename = "in_use")]
    InUse { mountpoint: String },
    #[serde(rename = "claimed")]
    Claimed { disk_id: String, volume_id: String },
}

/// Combined disk info for the API response.
#[derive(Clone, Debug, Serialize)]
pub struct DiscoveredDisk {
    #[serde(flatten)]
    pub device: BlockDevice,
    #[serde(flatten)]
    pub status: DiskStatus,
}

// ── lsblk JSON structures ───────────────────────────────────────────────

#[derive(Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Deserialize)]
struct LsblkDevice {
    name: String,
    size: Option<u64>,
    model: Option<String>,
    serial: Option<String>,
    #[serde(rename = "type")]
    dev_type: String,
    fstype: Option<String>,
    mountpoints: Option<Vec<Option<String>>>,
    children: Option<Vec<LsblkDevice>>,
    /// Transport type: sata, nvme, scsi, usb, ata, virtio, …
    tran: Option<String>,
    /// Removable device flag (1 = removable)
    rm: Option<bool>,
}

/// Discover all block devices on this node with their availability status.
pub fn discover_disks(db: &Connection) -> Vec<DiscoveredDisk> {
    let output = Command::new("lsblk")
        .args(["-Jb", "-o", "NAME,SIZE,MODEL,TYPE,FSTYPE,SERIAL,MOUNTPOINTS,TRAN,RM"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => {
            tracing::warn!("Failed to run lsblk");
            return Vec::new();
        }
    };

    let parsed: LsblkOutput = match serde_json::from_str(&output) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse lsblk output: {}", e);
            return Vec::new();
        }
    };

    // Load claimed disks from DB
    let claimed: Vec<(String, String, String)> = db.prepare(
        "SELECT id, device_path, volume_id FROM claimed_disks WHERE status != 'released'"
    ).and_then(|mut stmt| {
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
    }).unwrap_or_default();

    let mut result = Vec::new();

    for dev in &parsed.blockdevices {
        // Only whole disks, not partitions/loops/roms/cdroms
        if dev.dev_type != "disk" {
            continue;
        }

        // Filter out unsupported device classes:
        // - Removable devices (USB sticks, floppies, card readers)
        // - CD/DVD/BD drives (show up as disk sometimes)
        // - Floppy drives (fd*)
        // - Loop devices that slipped through
        if dev.rm.unwrap_or(false) {
            continue;
        }
        if dev.name.starts_with("fd") || dev.name.starts_with("sr") || dev.name.starts_with("cd") {
            continue;
        }
        // Filter by transport: only allow sata, nvme, scsi, ata, sas, virtio, and IDE
        // Block usb, firewire, mmc (SD cards), and unknown removable transports
        if let Some(ref tran) = dev.tran {
            match tran.as_str() {
                "sata" | "nvme" | "scsi" | "ata" | "sas" | "virtio" | "ide" | "fc" => {}
                "usb" | "firewire" | "ieee1394" | "mmc" | "sdio" => continue,
                _ => {
                    // Unknown transport — allow if size > 1GB (probably a real disk)
                    if dev.size.unwrap_or(0) < 1_073_741_824 {
                        continue;
                    }
                }
            }
        }
        // Skip very small devices (< 1GB) — likely floppies or virtual devices
        if dev.size.unwrap_or(0) < 1_073_741_824 {
            continue;
        }

        let path = format!("/dev/{}", dev.name);
        let has_partitions = dev.children.as_ref().map(|c| !c.is_empty()).unwrap_or(false);

        // Check if any child partition is mounted on /, /boot, or is swap
        let is_os_disk = is_system_disk(dev);

        // Skip OS/system disks entirely — they must never be shown or claimable
        if is_os_disk {
            continue;
        }

        // Check if the disk itself or any partition is mounted
        let mountpoint = get_any_mountpoint(dev);

        let device = BlockDevice {
            name: dev.name.clone(),
            path: path.clone(),
            size_bytes: dev.size.unwrap_or(0),
            model: dev.model.clone().unwrap_or_default().trim().to_string(),
            serial: dev.serial.clone().unwrap_or_default().trim().to_string(),
            fs_type: dev.fstype.clone(),
            mountpoint: mountpoint.clone(),
            has_partitions,
            is_os_disk,
        };

        // Classify status (OS disks already filtered above)
        let status = if let Some(ref mp) = mountpoint {
            // Check if mounted by CoreSAN
            if let Some((id, _, vol)) = claimed.iter().find(|(_, dp, _)| *dp == path) {
                DiskStatus::Claimed { disk_id: id.clone(), volume_id: vol.clone() }
            } else {
                DiskStatus::InUse { mountpoint: mp.clone() }
            }
        } else if let Some((id, _, vol)) = claimed.iter().find(|(_, dp, _)| *dp == path) {
            DiskStatus::Claimed { disk_id: id.clone(), volume_id: vol.clone() }
        } else if dev.fstype.is_some() || has_partitions {
            let fs = dev.fstype.clone()
                .or_else(|| dev.children.as_ref()
                    .and_then(|c| c.first())
                    .and_then(|c| c.fstype.clone()))
                .unwrap_or_else(|| "unknown".into());
            DiskStatus::HasData { fs_type: fs }
        } else {
            DiskStatus::Available
        };

        result.push(DiscoveredDisk { device, status });
    }

    result
}

/// Check if a disk is the system/OS disk (has partitions mounted on /, /boot, or swap).
fn is_system_disk(dev: &LsblkDevice) -> bool {
    let check_mountpoints = |mps: &Option<Vec<Option<String>>>| -> bool {
        mps.as_ref().map(|list| list.iter().any(|mp| {
            mp.as_ref().map(|m| m == "/" || m == "/boot" || m == "/boot/efi" || m == "[SWAP]").unwrap_or(false)
        })).unwrap_or(false)
    };

    // Check the disk itself
    if check_mountpoints(&dev.mountpoints) {
        return true;
    }

    // Check children (partitions)
    if let Some(ref children) = dev.children {
        for child in children {
            if check_mountpoints(&child.mountpoints) {
                return true;
            }
        }
    }

    false
}

/// Get any active mountpoint for a disk or its partitions.
fn get_any_mountpoint(dev: &LsblkDevice) -> Option<String> {
    // Check disk itself
    if let Some(ref mps) = dev.mountpoints {
        for mp in mps {
            if let Some(ref m) = mp {
                if !m.is_empty() {
                    return Some(m.clone());
                }
            }
        }
    }

    // Check children
    if let Some(ref children) = dev.children {
        for child in children {
            if let Some(ref mps) = child.mountpoints {
                for mp in mps {
                    if let Some(ref m) = mp {
                        if !m.is_empty() {
                            return Some(m.clone());
                        }
                    }
                }
            }
        }
    }

    None
}
