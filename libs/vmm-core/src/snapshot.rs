//! Snapshot support for corevm virtual machines.
//!
//! Provides types and logic for:
//! - Saving and restoring full VM state (RAM, CPU registers, device state)
//! - Differential (copy-on-write) disk images that track only changed sectors
//! - Snapshot chains with consolidation and deletion
//!
//! All file paths are provided by the caller — this module never decides
//! where files are stored.

use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::path::{Path, PathBuf};

// ── Constants ────────────────────────────────────────────────────────────

/// Sector size for differential disk tracking (512 bytes, matching ATA).
pub const SECTOR_SIZE: u64 = 512;

/// Block size for differential disk I/O grouping (4 KiB = 8 sectors).
pub const BLOCK_SIZE: u64 = 4096;

/// Magic bytes at the start of a differential disk image file.
const DIFF_DISK_MAGIC: &[u8; 8] = b"CVMDIFF\0";

/// Current differential disk format version.
const DIFF_DISK_VERSION: u32 = 1;

/// Magic bytes at the start of a VM state file.
const VM_STATE_MAGIC: &[u8; 8] = b"CVMSTATE";

/// Current VM state format version.
const VM_STATE_VERSION: u32 = 1;

// ── Snapshot metadata ────────────────────────────────────────────────────

/// Information about a single snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotInfo {
    /// Unique snapshot identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// UTC timestamp (seconds since epoch).
    pub timestamp: u64,
    /// Whether the VM was running when the snapshot was taken.
    pub live: bool,
    /// Path to the VM state file (RAM + registers + device state).
    /// Empty for offline snapshots that only capture the disk.
    pub state_file: PathBuf,
    /// Per-disk differential image paths, indexed by disk slot.
    /// Each entry is the diff layer created at snapshot time.
    pub disk_diffs: Vec<DiskLayerRef>,
}

/// Reference to a differential disk layer for one disk slot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiskLayerRef {
    /// Disk slot index (0 = primary disk, 1 = second disk, …).
    pub slot: usize,
    /// Path to the differential disk image file.
    pub diff_path: PathBuf,
    /// Path to the parent image (base or previous diff).
    pub parent_path: PathBuf,
}

/// Manifest tracking all snapshots and the current disk chain for a VM.
///
/// Persisted as JSON by the caller. This module provides the logic,
/// the caller decides where to store the manifest file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotManifest {
    /// VM UUID this manifest belongs to.
    pub vm_uuid: String,
    /// Ordered list of snapshots (oldest first).
    pub snapshots: Vec<SnapshotInfo>,
    /// Current active disk chain per slot.
    /// The last entry in each chain is the active (writable) layer.
    /// The first entry is the original base image.
    pub disk_chains: Vec<DiskChain>,
}

/// Chain of disk images for a single disk slot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiskChain {
    /// Disk slot index.
    pub slot: usize,
    /// Ordered layers: [base, diff1, diff2, …, active].
    pub layers: Vec<PathBuf>,
}

// ── VM state file ────────────────────────────────────────────────────────

/// Header for the VM state file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmStateHeader {
    /// Number of vCPUs whose state follows.
    pub vcpu_count: u32,
    /// RAM size in bytes.
    pub ram_bytes: u64,
    /// Sections present in this state file.
    pub sections: Vec<StateSection>,
}

/// A named section within the VM state file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateSection {
    /// Section identifier (e.g. "vcpu0", "pic", "pit", "cmos", …).
    pub name: String,
    /// Byte offset from start of file.
    pub offset: u64,
    /// Byte length of this section's data.
    pub length: u64,
}

// ── Serializable device state structs ────────────────────────────────────

/// Complete vCPU register state (one per vCPU).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VcpuSnapshot {
    /// General-purpose registers (RAX..R15 = indices 0..15).
    pub gpr: [u64; 16],
    pub rip: u64,
    pub rflags: u64,
    /// Segment registers: ES, CS, SS, DS, FS, GS (indices 0..5).
    pub segments: [SegmentSnapshot; 6],
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,
    pub dr: [u64; 8],
    pub gdtr_base: u64,
    pub gdtr_limit: u16,
    pub idtr_base: u64,
    pub idtr_limit: u16,
    pub ldtr: u16,
    pub tr: u16,
    pub ldt_base: u64,
    pub ldt_limit: u32,
    pub efer: u64,
    pub xcr0: u64,
    pub cpl: u8,
    /// Sparse MSR map (only non-default values).
    pub msrs: BTreeMap<u32, u64>,
    /// LAPIC state (opaque blob from backend, typically 1024 bytes).
    pub lapic_state: Vec<u8>,
}

/// Saved segment register.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct SegmentSnapshot {
    pub base: u64,
    pub limit: u32,
    pub selector: u16,
    pub access: u8,
    pub flags: u8,
    pub dpl: u8,
    pub present: bool,
    pub is_code: bool,
    pub is_conforming: bool,
    pub readable: bool,
    pub writable: bool,
    pub big: bool,
    pub long_mode: bool,
    pub granularity: bool,
}

/// PIC pair state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PicSnapshot {
    pub master: PicUnitSnapshot,
    pub slave: PicUnitSnapshot,
}

/// Single PIC unit state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PicUnitSnapshot {
    pub irr: u8,
    pub isr: u8,
    pub imr: u8,
    pub icw: [u8; 4],
    pub icw_step: u8,
    pub vector_offset: u8,
    pub read_isr: bool,
    pub auto_eoi: bool,
}

/// PIT (8254) state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PitSnapshot {
    pub channels: [PitChannelSnapshot; 3],
}

/// Single PIT channel state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PitChannelSnapshot {
    pub count: u16,
    pub output: bool,
    pub mode: u8,
    pub access_mode: u8,
    pub bcd: bool,
    pub latch: u16,
    pub latched: bool,
    pub read_hi: bool,
    pub write_hi: bool,
    pub gate: bool,
    pub enabled: bool,
    pub current: u16,
}

/// CMOS / RTC state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CmosSnapshot {
    pub index: u8,
    /// 128 bytes of CMOS NVRAM.
    pub data: Vec<u8>,
    pub nmi_disabled: bool,
}

/// PS/2 controller state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ps2Snapshot {
    pub status: u8,
    pub command_byte: u8,
    pub mouse_enabled: bool,
    pub keyboard_enabled: bool,
    pub scancode_set: u8,
    pub mouse_buffer: Vec<u8>,
    pub keyboard_buffer: Vec<u8>,
    pub output_buffer: Vec<(u8, bool)>,
    pub mouse_id: u8,
}

/// Serial port (UART 16550) state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SerialSnapshot {
    pub rbr: u8,
    pub thr: u8,
    pub ier: u8,
    pub iir: u8,
    pub fcr: u8,
    pub lcr: u8,
    pub mcr: u8,
    pub lsr: u8,
    pub msr: u8,
    pub scratch: u8,
    pub dll: u8,
    pub dlm: u8,
    pub input: Vec<u8>,
    pub output: Vec<u8>,
    pub irq_pending: bool,
}

/// ACPI PM state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AcpiPmSnapshot {
    pub pm1_status: u16,
    pub pm1_enable: u16,
    pub pm1_control: u16,
    pub timer_count: u32,
}

/// AHCI controller state (without disk data — that's in the diff layers).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AhciSnapshot {
    pub cap: u32,
    pub ghc: u32,
    pub is: u32,
    pub pi: u32,
    pub vs: u32,
    pub ports: Vec<AhciPortSnapshot>,
    pub msi_enabled: bool,
    pub msi_address: u64,
    pub msi_data: u32,
}

/// AHCI port state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AhciPortSnapshot {
    pub clb: u64,
    pub fb: u64,
    pub is: u32,
    pub ie: u32,
    pub cmd: u32,
    pub tfd: u32,
    pub sig: u32,
    pub ssts: u32,
    pub sctl: u32,
    pub serr: u32,
    pub sact: u32,
    pub ci: u32,
    pub sntf: u32,
    pub fbs: u32,
    pub drive_present: bool,
    pub drive_kind: u8,
    pub drive_total_bytes: u64,
}

/// E1000 NIC state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct E1000Snapshot {
    /// All 32768 register dwords (128 KB register space).
    pub regs: Vec<u32>,
    pub mac_address: [u8; 6],
    pub eeprom: Vec<u16>,
    pub phy_regs: Vec<u16>,
    pub rx_buffer: Vec<Vec<u8>>,
    pub tx_buffer: Vec<Vec<u8>>,
    pub msi_enabled: bool,
    pub msi_address: u64,
    pub msi_data: u32,
}

/// Complete device state bundle for a VM snapshot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceStateBundle {
    pub pic: PicSnapshot,
    pub pit: PitSnapshot,
    pub cmos: CmosSnapshot,
    pub ps2: Ps2Snapshot,
    pub serial: SerialSnapshot,
    pub acpi_pm: AcpiPmSnapshot,
    pub ahci: AhciSnapshot,
    pub e1000: Option<E1000Snapshot>,
}

/// Complete VM snapshot state (everything except RAM and disk data).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmSnapshot {
    /// Format version for forward compatibility.
    pub version: u32,
    /// Per-vCPU register state.
    pub vcpus: Vec<VcpuSnapshot>,
    /// Device state.
    pub devices: DeviceStateBundle,
    /// RAM size in bytes (RAM data is stored separately, not inline).
    pub ram_bytes: u64,
    /// IRQ assertion flags.
    pub ahci_irq_asserted: bool,
    pub e1000_irq_asserted: bool,
}

// ── Differential disk image ──────────────────────────────────────────────

/// On-disk header of a differential disk image.
#[derive(Clone, Debug)]
pub struct DiffDiskHeader {
    pub parent_size: u64,
    pub block_count: u64,
    pub data_offset: u64,
    pub parent_path: PathBuf,
}

const HEADER_FIXED_SIZE: u64 = 36;
const PARENT_PATH_MAX: usize = 256;

impl DiffDiskHeader {
    fn bitmap_bytes(block_count: u64) -> u64 {
        (block_count + 7) / 8
    }
}

/// Differential disk image with fixed-offset block layout.
///
/// File layout:
/// ```text
///   [0..8]    magic: "CVMDIFF\0"
///   [8..12]   version: u32 LE
///   [12..20]  parent_size: u64 LE
///   [20..28]  block_count: u64 LE
///   [28..36]  data_offset: u64 LE
///   [36..292] parent_path: 256 bytes UTF-8 null-padded
///   [292..292+ceil(block_count/8)] bitmap
///   [data_offset + block_idx * BLOCK_SIZE] block data (sparse, only
///     blocks with bitmap bit set contain valid data; unallocated blocks
///     are holes / zero in the file)
/// ```
///
/// This layout allows O(1) block lookup and writing without data shifting.
/// The file may be sparse on filesystems that support it.
pub struct DiffDiskImage {
    header: DiffDiskHeader,
    bitmap: Vec<u8>,
    allocated_blocks: u64,
}

impl DiffDiskImage {
    /// Create a new empty differential disk image.
    pub fn create(
        file: &mut (impl Write + Seek),
        parent_size: u64,
        parent_path: &Path,
    ) -> io::Result<Self> {
        let block_count = (parent_size + BLOCK_SIZE - 1) / BLOCK_SIZE;
        let bitmap_len = DiffDiskHeader::bitmap_bytes(block_count) as usize;
        let data_offset = HEADER_FIXED_SIZE + PARENT_PATH_MAX as u64 + bitmap_len as u64;
        let bitmap = vec![0u8; bitmap_len];

        file.seek(SeekFrom::Start(0))?;
        file.write_all(DIFF_DISK_MAGIC)?;
        file.write_all(&DIFF_DISK_VERSION.to_le_bytes())?;
        file.write_all(&parent_size.to_le_bytes())?;
        file.write_all(&block_count.to_le_bytes())?;
        file.write_all(&data_offset.to_le_bytes())?;

        let mut path_buf = [0u8; PARENT_PATH_MAX];
        let path_str = parent_path.to_string_lossy();
        let path_bytes = path_str.as_bytes();
        let copy_len = path_bytes.len().min(PARENT_PATH_MAX - 1);
        path_buf[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
        file.write_all(&path_buf)?;

        file.write_all(&bitmap)?;
        file.flush()?;

        Ok(Self {
            header: DiffDiskHeader {
                parent_size,
                block_count,
                data_offset,
                parent_path: parent_path.to_path_buf(),
            },
            bitmap,
            allocated_blocks: 0,
        })
    }

    /// Open an existing differential disk image.
    pub fn open(file: &mut (impl Read + Seek)) -> io::Result<Self> {
        file.seek(SeekFrom::Start(0))?;

        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        if &magic != DIFF_DISK_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a corevm differential disk image",
            ));
        }

        let mut buf4 = [0u8; 4];
        file.read_exact(&mut buf4)?;
        let version = u32::from_le_bytes(buf4);
        if version != DIFF_DISK_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported diff disk version {}", version),
            ));
        }

        let mut buf8 = [0u8; 8];
        file.read_exact(&mut buf8)?;
        let parent_size = u64::from_le_bytes(buf8);

        file.read_exact(&mut buf8)?;
        let block_count = u64::from_le_bytes(buf8);

        file.read_exact(&mut buf8)?;
        let data_offset = u64::from_le_bytes(buf8);

        let mut path_raw = [0u8; PARENT_PATH_MAX];
        file.read_exact(&mut path_raw)?;
        let path_end = path_raw.iter().position(|&b| b == 0).unwrap_or(PARENT_PATH_MAX);
        let parent_path = PathBuf::from(
            std::str::from_utf8(&path_raw[..path_end])
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
        );

        let bitmap_len = DiffDiskHeader::bitmap_bytes(block_count) as usize;
        let mut bitmap = vec![0u8; bitmap_len];
        file.read_exact(&mut bitmap)?;

        let allocated_blocks = bitmap.iter().map(|b| b.count_ones() as u64).sum();

        Ok(Self {
            header: DiffDiskHeader {
                parent_size,
                block_count,
                data_offset,
                parent_path,
            },
            bitmap,
            allocated_blocks,
        })
    }

    /// Virtual disk size in bytes.
    pub fn virtual_size(&self) -> u64 {
        self.header.parent_size
    }

    /// Number of blocks that contain data in this diff layer.
    pub fn allocated_blocks(&self) -> u64 {
        self.allocated_blocks
    }

    /// Total number of blocks in the virtual disk.
    pub fn total_blocks(&self) -> u64 {
        self.header.block_count
    }

    /// Parent image path stored in the header.
    pub fn parent_path(&self) -> &Path {
        &self.header.parent_path
    }

    /// Check whether a given block is present in this diff layer.
    pub fn has_block(&self, block_idx: u64) -> bool {
        if block_idx >= self.header.block_count {
            return false;
        }
        let byte = (block_idx / 8) as usize;
        let bit = (block_idx % 8) as u8;
        self.bitmap.get(byte).map_or(false, |b| b & (1 << bit) != 0)
    }

    /// File offset for a given block (fixed layout: data_offset + idx * BLOCK_SIZE).
    fn block_offset(&self, block_idx: u64) -> u64 {
        self.header.data_offset + block_idx * BLOCK_SIZE
    }

    /// Read a block. Returns `None` if the block is not in this layer.
    pub fn read_block(
        &self,
        file: &mut (impl Read + Seek),
        block_idx: u64,
    ) -> io::Result<Option<Vec<u8>>> {
        if !self.has_block(block_idx) {
            return Ok(None);
        }
        file.seek(SeekFrom::Start(self.block_offset(block_idx)))?;
        let mut buf = vec![0u8; BLOCK_SIZE as usize];
        file.read_exact(&mut buf)?;
        Ok(Some(buf))
    }

    /// Write a block, marking it as allocated if necessary.
    pub fn write_block(
        &mut self,
        file: &mut (impl Write + Seek),
        block_idx: u64,
        data: &[u8],
    ) -> io::Result<()> {
        if block_idx >= self.header.block_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "block index out of range",
            ));
        }
        if data.len() != BLOCK_SIZE as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("block data must be {} bytes", BLOCK_SIZE),
            ));
        }

        if !self.has_block(block_idx) {
            let byte = (block_idx / 8) as usize;
            let bit = (block_idx % 8) as u8;
            self.bitmap[byte] |= 1 << bit;
            self.allocated_blocks += 1;

            // Persist updated bitmap byte
            let bitmap_offset = HEADER_FIXED_SIZE + PARENT_PATH_MAX as u64 + byte as u64;
            file.seek(SeekFrom::Start(bitmap_offset))?;
            file.write_all(&[self.bitmap[byte]])?;
        }

        file.seek(SeekFrom::Start(self.block_offset(block_idx)))?;
        file.write_all(data)?;

        Ok(())
    }

    /// Iterate over all allocated block indices.
    pub fn allocated_block_indices(&self) -> Vec<u64> {
        let mut indices = Vec::with_capacity(self.allocated_blocks as usize);
        for i in 0..self.header.block_count {
            if self.has_block(i) {
                indices.push(i);
            }
        }
        indices
    }

    /// Flush the bitmap to the file (writes the entire bitmap).
    pub fn flush_bitmap(&self, file: &mut (impl Write + Seek)) -> io::Result<()> {
        let bitmap_offset = HEADER_FIXED_SIZE + PARENT_PATH_MAX as u64;
        file.seek(SeekFrom::Start(bitmap_offset))?;
        file.write_all(&self.bitmap)?;
        file.flush()
    }
}

// ── VM state file I/O ────────────────────────────────────────────────────

/// Write a complete VM state file.
///
/// Layout:
/// ```text
///   [0..8]       magic: "CVMSTATE"
///   [8..12]      version: u32 LE
///   [12..16]     header_len: u32 LE (length of JSON header)
///   [16..16+N]   header JSON (VmStateHeader)
///   [aligned]    RAM data (raw bytes, ram_bytes long)
///   [after RAM]  snapshot JSON (VmSnapshot — vcpu regs + device state)
/// ```
pub fn write_vm_state(
    file: &mut (impl Write + Seek),
    snapshot: &VmSnapshot,
    ram: &[u8],
) -> io::Result<()> {
    let snapshot_json = serde_json::to_vec(snapshot)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Iterate to find stable header length (offset digits can change the
    // serialized size). Typically converges in 1-2 passes.
    let mut header_len_estimate = 0u64;
    let header_json;
    loop {
        let preamble = 16 + header_len_estimate; // magic(8) + version(4) + len(4) + header
        let ram_offset = preamble;
        let snapshot_offset = ram_offset + ram.len() as u64;

        let header = VmStateHeader {
            vcpu_count: snapshot.vcpus.len() as u32,
            ram_bytes: ram.len() as u64,
            sections: vec![
                StateSection {
                    name: "ram".into(),
                    offset: ram_offset,
                    length: ram.len() as u64,
                },
                StateSection {
                    name: "snapshot".into(),
                    offset: snapshot_offset,
                    length: snapshot_json.len() as u64,
                },
            ],
        };

        let serialized = serde_json::to_vec(&header)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        if serialized.len() as u64 == header_len_estimate {
            header_json = serialized;
            break;
        }
        header_len_estimate = serialized.len() as u64;
    }

    file.seek(SeekFrom::Start(0))?;
    file.write_all(VM_STATE_MAGIC)?;
    file.write_all(&VM_STATE_VERSION.to_le_bytes())?;
    file.write_all(&(header_json.len() as u32).to_le_bytes())?;
    file.write_all(&header_json)?;
    file.write_all(ram)?;
    file.write_all(&snapshot_json)?;
    file.flush()?;

    Ok(())
}

/// Read a VM state header without loading RAM or snapshot data.
pub fn read_vm_state_header(
    file: &mut (impl Read + Seek),
) -> io::Result<VmStateHeader> {
    file.seek(SeekFrom::Start(0))?;

    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;
    if &magic != VM_STATE_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a corevm state file",
        ));
    }

    let mut buf4 = [0u8; 4];
    file.read_exact(&mut buf4)?;
    let version = u32::from_le_bytes(buf4);
    if version != VM_STATE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported state file version {}", version),
        ));
    }

    file.read_exact(&mut buf4)?;
    let header_len = u32::from_le_bytes(buf4) as usize;

    let mut header_buf = vec![0u8; header_len];
    file.read_exact(&mut header_buf)?;

    serde_json::from_slice(&header_buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Read RAM data from a VM state file into the provided buffer.
pub fn read_vm_state_ram(
    file: &mut (impl Read + Seek),
    header: &VmStateHeader,
    ram: &mut [u8],
) -> io::Result<()> {
    let ram_section = header.sections.iter()
        .find(|s| s.name == "ram")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no ram section"))?;

    if ram.len() < ram_section.length as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "RAM buffer too small: {} < {}",
                ram.len(),
                ram_section.length
            ),
        ));
    }

    file.seek(SeekFrom::Start(ram_section.offset))?;
    file.read_exact(&mut ram[..ram_section.length as usize])?;
    Ok(())
}

/// Read the VmSnapshot (vCPU + device state) from a VM state file.
pub fn read_vm_state_snapshot(
    file: &mut (impl Read + Seek),
    header: &VmStateHeader,
) -> io::Result<VmSnapshot> {
    let snap_section = header.sections.iter()
        .find(|s| s.name == "snapshot")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no snapshot section"))?;

    file.seek(SeekFrom::Start(snap_section.offset))?;
    let mut buf = vec![0u8; snap_section.length as usize];
    file.read_exact(&mut buf)?;

    serde_json::from_slice(&buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

// ── Snapshot manifest operations ─────────────────────────────────────────

impl SnapshotManifest {
    /// Create a new empty manifest for a VM.
    pub fn new(vm_uuid: &str, base_disks: &[(usize, PathBuf)]) -> Self {
        let disk_chains = base_disks
            .iter()
            .map(|(slot, path)| DiskChain {
                slot: *slot,
                layers: vec![path.clone()],
            })
            .collect();

        Self {
            vm_uuid: vm_uuid.to_string(),
            snapshots: Vec::new(),
            disk_chains,
        }
    }

    /// Save the manifest as JSON to the given path.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Load a manifest from a JSON file.
    pub fn load(path: &Path) -> io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Get the current active (writable) disk image path for a slot.
    pub fn active_disk(&self, slot: usize) -> Option<&Path> {
        self.disk_chains
            .iter()
            .find(|c| c.slot == slot)
            .and_then(|c| c.layers.last())
            .map(|p| p.as_path())
    }

    /// Get the base (original) disk image path for a slot.
    pub fn base_disk(&self, slot: usize) -> Option<&Path> {
        self.disk_chains
            .iter()
            .find(|c| c.slot == slot)
            .and_then(|c| c.layers.first())
            .map(|p| p.as_path())
    }

    /// Record a new snapshot.
    ///
    /// For each disk slot, a new differential layer is created. The caller
    /// must have already written the diff disk files and the state file.
    ///
    /// After calling this, the active disk for each slot is the new diff layer,
    /// and new writes go there.
    pub fn add_snapshot(&mut self, info: SnapshotInfo) {
        // Push each diff layer onto its chain
        for layer_ref in &info.disk_diffs {
            if let Some(chain) = self.disk_chains.iter_mut().find(|c| c.slot == layer_ref.slot) {
                chain.layers.push(layer_ref.diff_path.clone());
            }
        }
        self.snapshots.push(info);
    }

    /// Find a snapshot by ID.
    pub fn find_snapshot(&self, id: &str) -> Option<&SnapshotInfo> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    /// Find a snapshot index by ID.
    fn snapshot_index(&self, id: &str) -> Option<usize> {
        self.snapshots.iter().position(|s| s.id == id)
    }

    /// Delete a snapshot and repair the disk chain.
    ///
    /// When deleting snapshot N (not the most recent):
    /// - The diff layer from snapshot N is merged into the diff layer
    ///   of snapshot N+1 (or the active layer if N is the last snapshot).
    /// - The caller must perform the actual file I/O for merging.
    ///
    /// Returns the merge operations the caller must execute, or `None`
    /// if the snapshot was not found.
    pub fn delete_snapshot(&mut self, id: &str) -> Option<Vec<SnapshotDeleteOp>> {
        let idx = self.snapshot_index(id)?;
        let snap = self.snapshots.remove(idx);
        let mut ops = Vec::new();

        for layer_ref in &snap.disk_diffs {
            let chain = match self.disk_chains.iter_mut().find(|c| c.slot == layer_ref.slot) {
                Some(c) => c,
                None => continue,
            };

            let layer_pos = chain.layers.iter().position(|p| p == &layer_ref.diff_path);
            let layer_pos = match layer_pos {
                Some(p) => p,
                None => continue,
            };

            if layer_pos + 1 < chain.layers.len() {
                // Not the last layer: merge into the next layer
                let next_layer = chain.layers[layer_pos + 1].clone();
                ops.push(SnapshotDeleteOp::MergeDown {
                    slot: layer_ref.slot,
                    source: layer_ref.diff_path.clone(),
                    target: next_layer,
                });
            }
            // Remove this layer from the chain
            chain.layers.remove(layer_pos);
        }

        // Delete the state file
        if !snap.state_file.as_os_str().is_empty() {
            ops.push(SnapshotDeleteOp::DeleteFile(snap.state_file));
        }

        Some(ops)
    }

    /// Consolidate: merge all differential layers between two points
    /// into a single layer, reducing the chain depth.
    ///
    /// `from_id` and `to_id` define the range (inclusive). If `to_id`
    /// is `None`, consolidation goes up to the active layer.
    ///
    /// Returns the consolidation operations the caller must execute.
    pub fn consolidate(
        &mut self,
        slot: usize,
        from_id: Option<&str>,
        to_id: Option<&str>,
    ) -> Option<Vec<ConsolidateOp>> {
        let chain = self.disk_chains.iter().find(|c| c.slot == slot)?;
        if chain.layers.len() <= 1 {
            return None; // Nothing to consolidate
        }

        // Determine range of layers to merge
        let from_layer = if let Some(id) = from_id {
            let snap = self.find_snapshot(id)?;
            let diff = snap.disk_diffs.iter().find(|d| d.slot == slot)?;
            chain.layers.iter().position(|p| p == &diff.diff_path)?
        } else {
            1 // First diff layer (skip base)
        };

        let to_layer = if let Some(id) = to_id {
            let snap = self.find_snapshot(id)?;
            let diff = snap.disk_diffs.iter().find(|d| d.slot == slot)?;
            chain.layers.iter().position(|p| p == &diff.diff_path)?
        } else {
            chain.layers.len() - 1
        };

        if from_layer >= to_layer || from_layer == 0 {
            return None;
        }

        let layers_to_merge: Vec<PathBuf> = chain.layers[from_layer..=to_layer].to_vec();
        let target = layers_to_merge.last()?.clone();
        let sources: Vec<PathBuf> = layers_to_merge[..layers_to_merge.len() - 1].to_vec();

        Some(vec![ConsolidateOp::MergeLayers {
            slot,
            sources,
            target,
        }])
    }

    /// Apply a consolidation: remove merged layers from the chain.
    /// Call this after the caller has performed the actual merge I/O.
    pub fn apply_consolidation(&mut self, slot: usize, removed_layers: &[PathBuf]) {
        if let Some(chain) = self.disk_chains.iter_mut().find(|c| c.slot == slot) {
            chain.layers.retain(|p| !removed_layers.contains(p));
        }

        // Clean up snapshot references to removed layers
        for snap in &mut self.snapshots {
            snap.disk_diffs.retain(|d| {
                d.slot != slot || !removed_layers.contains(&d.diff_path)
            });
        }
    }
}

/// Operation the caller must execute when deleting a snapshot.
#[derive(Clone, Debug)]
pub enum SnapshotDeleteOp {
    /// Merge source diff layer into target diff layer, then delete source.
    MergeDown {
        slot: usize,
        source: PathBuf,
        target: PathBuf,
    },
    /// Delete a file (state file or orphaned diff).
    DeleteFile(PathBuf),
}

/// Operation the caller must execute when consolidating layers.
#[derive(Clone, Debug)]
pub enum ConsolidateOp {
    /// Merge all source layers into the target layer.
    MergeLayers {
        slot: usize,
        sources: Vec<PathBuf>,
        target: PathBuf,
    },
}

// ── Merge logic for differential disk images ─────────────────────────────

/// Merge a source diff layer into a target diff layer.
///
/// Any block present in source but not in target is copied to target.
/// Blocks already in target are NOT overwritten (target is newer).
pub fn merge_diff_layers(
    source_file: &mut (impl Read + Seek),
    target_file: &mut (impl Read + Write + Seek),
) -> io::Result<u64> {
    let source = DiffDiskImage::open(source_file)?;
    let mut target = DiffDiskImage::open(target_file)?;

    if source.virtual_size() != target.virtual_size() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot merge diff layers with different virtual sizes",
        ));
    }

    let mut merged_count = 0u64;

    for block_idx in source.allocated_block_indices() {
        if !target.has_block(block_idx) {
            // Read block from source
            if let Some(data) = source.read_block(source_file, block_idx)? {
                // Write to target
                target.write_block(target_file, block_idx, &data)?;
                merged_count += 1;
            }
        }
    }

    Ok(merged_count)
}

/// Consolidate a base image with a diff layer, producing a new full image.
///
/// Reads the base image and overlays all blocks from the diff layer.
pub fn flatten_to_base(
    base_file: &mut (impl Read + Seek),
    diff_file: &mut (impl Read + Seek),
    output_file: &mut (impl Write + Seek),
    base_size: u64,
) -> io::Result<()> {
    let diff = DiffDiskImage::open(diff_file)?;

    // Copy base image to output
    base_file.seek(SeekFrom::Start(0))?;
    output_file.seek(SeekFrom::Start(0))?;

    let mut buf = vec![0u8; BLOCK_SIZE as usize];
    let mut remaining = base_size;
    while remaining > 0 {
        let to_read = remaining.min(BLOCK_SIZE) as usize;
        base_file.read_exact(&mut buf[..to_read])?;
        output_file.write_all(&buf[..to_read])?;
        remaining -= to_read as u64;
    }

    // Overlay diff blocks
    for block_idx in diff.allocated_block_indices() {
        if let Some(data) = diff.read_block(diff_file, block_idx)? {
            let offset = block_idx * BLOCK_SIZE;
            if offset < base_size {
                output_file.seek(SeekFrom::Start(offset))?;
                let to_write = ((base_size - offset).min(BLOCK_SIZE)) as usize;
                output_file.write_all(&data[..to_write])?;
            }
        }
    }

    output_file.flush()?;
    Ok(())
}

/// Read a block from a chain of disk layers (diff layers + base).
///
/// Walks the chain from the top (most recent) layer downward. The first
/// layer that contains the block wins.
///
/// `files` must be ordered newest-first: [active_diff, ..., oldest_diff, base].
/// The last entry is the base image (raw, not a diff).
pub fn read_block_from_chain(
    files: &mut [(impl Read + Seek, bool)], // (file, is_diff)
    block_idx: u64,
) -> io::Result<Vec<u8>> {
    for (file, is_diff) in files.iter_mut() {
        if *is_diff {
            let diff = DiffDiskImage::open(&mut *file)?;
            if let Some(data) = diff.read_block(&mut *file, block_idx)? {
                return Ok(data);
            }
        } else {
            // Base image: read directly at block offset
            let offset = block_idx * BLOCK_SIZE;
            file.seek(SeekFrom::Start(offset))?;
            let mut buf = vec![0u8; BLOCK_SIZE as usize];
            file.read_exact(&mut buf)?;
            return Ok(buf);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "block not found in any layer",
    ))
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_diff_disk_create_and_open() {
        let mut buf = Cursor::new(Vec::new());
        let parent_size = 1024 * 1024; // 1 MB
        let parent_path = Path::new("/disks/base.img");

        let disk = DiffDiskImage::create(&mut buf, parent_size, parent_path).unwrap();
        assert_eq!(disk.virtual_size(), parent_size);
        assert_eq!(disk.allocated_blocks(), 0);
        assert_eq!(disk.total_blocks(), 256); // 1MB / 4KB
        assert_eq!(disk.parent_path(), parent_path);

        // Reopen
        let disk2 = DiffDiskImage::open(&mut buf).unwrap();
        assert_eq!(disk2.virtual_size(), parent_size);
        assert_eq!(disk2.allocated_blocks(), 0);
        assert_eq!(disk2.total_blocks(), 256);
    }

    #[test]
    fn test_diff_disk_write_read_block() {
        let mut buf = Cursor::new(Vec::new());
        let parent_size = 64 * 1024; // 64 KB = 16 blocks
        let parent_path = Path::new("/disks/base.img");

        let mut disk = DiffDiskImage::create(&mut buf, parent_size, parent_path).unwrap();

        // Write block 5
        let data = vec![0xAB; BLOCK_SIZE as usize];
        disk.write_block(&mut buf, 5, &data).unwrap();

        assert!(disk.has_block(5));
        assert!(!disk.has_block(0));
        assert_eq!(disk.allocated_blocks(), 1);

        // Read it back
        let read = disk.read_block(&mut buf, 5).unwrap().unwrap();
        assert_eq!(read, data);

        // Block 0 should return None
        assert!(disk.read_block(&mut buf, 0).unwrap().is_none());
    }

    #[test]
    fn test_diff_disk_multiple_blocks() {
        let mut buf = Cursor::new(Vec::new());
        let parent_size = 64 * 1024;
        let parent_path = Path::new("/disks/base.img");

        let mut disk = DiffDiskImage::create(&mut buf, parent_size, parent_path).unwrap();

        // Write blocks 0, 3, 15
        for idx in [0u64, 3, 15] {
            let data = vec![idx as u8; BLOCK_SIZE as usize];
            disk.write_block(&mut buf, idx, &data).unwrap();
        }

        assert_eq!(disk.allocated_blocks(), 3);

        // Verify each
        for idx in [0u64, 3, 15] {
            let read = disk.read_block(&mut buf, idx).unwrap().unwrap();
            assert_eq!(read[0], idx as u8);
        }

        // Verify unwritten block
        assert!(disk.read_block(&mut buf, 7).unwrap().is_none());
    }

    #[test]
    fn test_merge_diff_layers() {
        let parent_size = 32 * 1024; // 8 blocks

        // Create source with blocks 0, 2, 4
        let mut source_buf = Cursor::new(Vec::new());
        let mut source = DiffDiskImage::create(
            &mut source_buf, parent_size, Path::new("/base.img"),
        ).unwrap();
        for idx in [0u64, 2, 4] {
            let data = vec![(idx + 0x10) as u8; BLOCK_SIZE as usize];
            source.write_block(&mut source_buf, idx, &data).unwrap();
        }

        // Create target with blocks 2, 3 (block 2 overlaps with source)
        let mut target_buf = Cursor::new(Vec::new());
        let mut target = DiffDiskImage::create(
            &mut target_buf, parent_size, Path::new("/base.img"),
        ).unwrap();
        for idx in [2u64, 3] {
            let data = vec![(idx + 0x20) as u8; BLOCK_SIZE as usize];
            target.write_block(&mut target_buf, idx, &data).unwrap();
        }

        // Merge source into target
        let merged = merge_diff_layers(&mut source_buf, &mut target_buf).unwrap();
        assert_eq!(merged, 2); // blocks 0 and 4 copied, block 2 skipped

        // Verify target now has blocks 0, 2, 3, 4
        let target = DiffDiskImage::open(&mut target_buf).unwrap();
        assert_eq!(target.allocated_blocks(), 4);

        // Block 2 should have target's data (0x22), not source's (0x12)
        let b2 = target.read_block(&mut target_buf, 2).unwrap().unwrap();
        assert_eq!(b2[0], 0x22);

        // Block 0 should have source's data (0x10)
        let b0 = target.read_block(&mut target_buf, 0).unwrap().unwrap();
        assert_eq!(b0[0], 0x10);
    }

    #[test]
    fn test_vm_state_roundtrip() {
        let snapshot = VmSnapshot {
            version: VM_STATE_VERSION,
            vcpus: vec![VcpuSnapshot {
                gpr: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                rip: 0x1000,
                rflags: 0x202,
                cr0: 0x80000011,
                cr3: 0x3000,
                cr4: 0x20,
                efer: 0x500,
                ..Default::default()
            }],
            devices: DeviceStateBundle::default(),
            ram_bytes: 16,
            ahci_irq_asserted: false,
            e1000_irq_asserted: false,
        };

        let ram = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x11, 0x22, 0x33, 0x44,
                       0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC];

        let mut buf = Cursor::new(Vec::new());
        write_vm_state(&mut buf, &snapshot, &ram).unwrap();

        // Read back
        let header = read_vm_state_header(&mut buf).unwrap();
        assert_eq!(header.vcpu_count, 1);
        assert_eq!(header.ram_bytes, 16);

        let mut ram_out = vec![0u8; 16];
        read_vm_state_ram(&mut buf, &header, &mut ram_out).unwrap();
        assert_eq!(ram_out, ram);

        let snap_out = read_vm_state_snapshot(&mut buf, &header).unwrap();
        assert_eq!(snap_out.vcpus[0].rip, 0x1000);
        assert_eq!(snap_out.vcpus[0].gpr[0], 1);
        assert_eq!(snap_out.vcpus[0].cr0, 0x80000011);
    }

    #[test]
    fn test_snapshot_manifest_lifecycle() {
        let base_path = PathBuf::from("/disks/vm1.img");
        let mut manifest = SnapshotManifest::new(
            "test-uuid",
            &[(0, base_path.clone())],
        );

        assert_eq!(manifest.active_disk(0).unwrap(), Path::new("/disks/vm1.img"));

        // Add a snapshot
        let diff1_path = PathBuf::from("/disks/vm1-snap1.cvmdiff");
        manifest.add_snapshot(SnapshotInfo {
            id: "snap1".into(),
            name: "Snapshot 1".into(),
            timestamp: 1000,
            live: true,
            state_file: PathBuf::from("/state/snap1.cvmstate"),
            disk_diffs: vec![DiskLayerRef {
                slot: 0,
                diff_path: diff1_path.clone(),
                parent_path: base_path.clone(),
            }],
        });

        assert_eq!(manifest.active_disk(0).unwrap(), diff1_path.as_path());
        assert_eq!(manifest.base_disk(0).unwrap(), base_path.as_path());
        assert_eq!(manifest.snapshots.len(), 1);

        // Add another snapshot
        let diff2_path = PathBuf::from("/disks/vm1-snap2.cvmdiff");
        manifest.add_snapshot(SnapshotInfo {
            id: "snap2".into(),
            name: "Snapshot 2".into(),
            timestamp: 2000,
            live: false,
            state_file: PathBuf::new(),
            disk_diffs: vec![DiskLayerRef {
                slot: 0,
                diff_path: diff2_path.clone(),
                parent_path: diff1_path.clone(),
            }],
        });

        // Chain is now: base -> diff1 -> diff2
        assert_eq!(manifest.disk_chains[0].layers.len(), 3);
        assert_eq!(manifest.active_disk(0).unwrap(), diff2_path.as_path());

        // Delete snap1 — should merge diff1 into diff2
        let ops = manifest.delete_snapshot("snap1").unwrap();
        assert_eq!(ops.len(), 2); // MergeDown + DeleteFile
        match &ops[0] {
            SnapshotDeleteOp::MergeDown { source, target, .. } => {
                assert_eq!(source, &diff1_path);
                assert_eq!(target, &diff2_path);
            }
            _ => panic!("expected MergeDown"),
        }

        // Chain is now: base -> diff2
        assert_eq!(manifest.disk_chains[0].layers.len(), 2);
    }

    #[test]
    fn test_flatten_to_base() {
        let base_size = 16 * 1024; // 4 blocks

        // Create a fake base image
        let mut base_buf = Cursor::new(vec![0x11u8; base_size as usize]);

        // Create a diff with block 1 overwritten
        let mut diff_buf = Cursor::new(Vec::new());
        let mut diff = DiffDiskImage::create(
            &mut diff_buf, base_size, Path::new("/base.img"),
        ).unwrap();
        let new_data = vec![0xFF; BLOCK_SIZE as usize];
        diff.write_block(&mut diff_buf, 1, &new_data).unwrap();

        // Flatten
        let mut out_buf = Cursor::new(Vec::new());
        flatten_to_base(&mut base_buf, &mut diff_buf, &mut out_buf, base_size).unwrap();

        let output = out_buf.into_inner();
        assert_eq!(output.len(), base_size as usize);

        // Block 0 should be original
        assert_eq!(output[0], 0x11);
        // Block 1 should be overwritten
        assert_eq!(output[BLOCK_SIZE as usize], 0xFF);
        // Block 2 should be original
        assert_eq!(output[2 * BLOCK_SIZE as usize], 0x11);
    }
}
