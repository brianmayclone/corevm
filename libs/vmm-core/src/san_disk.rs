//! SAN Disk Protocol — binary protocol for direct VM disk I/O via Unix Domain Socket.
//!
//! This bypasses FUSE entirely. vmm-san runs a UDS listener per volume,
//! libcorevm connects and sends read/write commands directly.
//!
//! Wire format: fixed-size headers + optional data payload.
//! All integers are little-endian.

/// Protocol magic bytes for requests
pub const REQUEST_MAGIC: u32 = 0x53414E31; // "SAN1"
/// Protocol magic bytes for responses
pub const RESPONSE_MAGIC: u32 = 0x53414E52; // "SANR"

/// Returns the socket directory. Reads `VMM_SAN_SOCK_DIR` env var,
/// defaults to `/run/vmm-san`.
pub fn socket_dir() -> String {
    std::env::var("VMM_SAN_SOCK_DIR")
        .unwrap_or_else(|_| "/run/vmm-san".to_string())
}

/// Socket path template: `$VMM_SAN_SOCK_DIR/{volume_id}.sock`
/// Defaults to `/run/vmm-san/` if the env var is not set.
pub fn socket_path(volume_id: &str) -> String {
    format!("{}/{}.sock", socket_dir(), volume_id)
}

/// Request commands
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SanCommand {
    /// Open a file for I/O. Payload: rel_path as UTF-8 bytes.
    Open = 0,
    /// Read data at offset. No payload. Response carries data.
    Read = 1,
    /// Write data at offset. Payload: data bytes.
    Write = 2,
    /// Flush cached data to disk. No payload.
    Flush = 3,
    /// Close file handle and release lease. No payload.
    Close = 4,
    /// Get file size. No payload. Response: size as u64 in data.
    GetSize = 5,
}

impl SanCommand {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Open),
            1 => Some(Self::Read),
            2 => Some(Self::Write),
            3 => Some(Self::Flush),
            4 => Some(Self::Close),
            5 => Some(Self::GetSize),
            _ => None,
        }
    }
}

/// Response status codes
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SanStatus {
    Ok = 0,
    ErrNotFound = 1,
    ErrLeaseDenied = 2,
    ErrIo = 3,
    ErrProtocol = 4,
    ErrFull = 5,
}

/// Fixed-size request header (32 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SanRequestHeader {
    pub magic: u32,
    pub cmd: u32,
    pub file_id: u64,
    pub offset: u64,
    pub size: u32,
    pub flags: u32,
}

/// Fixed-size response header (16 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SanResponseHeader {
    pub magic: u32,
    pub status: u32,
    pub size: u32,
    pub reserved: u32,
}

impl SanRequestHeader {
    pub const SIZE: usize = 32;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..8].copy_from_slice(&self.cmd.to_le_bytes());
        buf[8..16].copy_from_slice(&self.file_id.to_le_bytes());
        buf[16..24].copy_from_slice(&self.offset.to_le_bytes());
        buf[24..28].copy_from_slice(&self.size.to_le_bytes());
        buf[28..32].copy_from_slice(&self.flags.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        Self {
            magic: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            cmd: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            file_id: u64::from_le_bytes([buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]]),
            offset: u64::from_le_bytes([buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23]]),
            size: u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
            flags: u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]),
        }
    }
}

impl SanResponseHeader {
    pub const SIZE: usize = 16;

    pub fn ok(data_size: u32) -> Self {
        Self { magic: RESPONSE_MAGIC, status: SanStatus::Ok as u32, size: data_size, reserved: 0 }
    }

    pub fn err(status: SanStatus) -> Self {
        Self { magic: RESPONSE_MAGIC, status: status as u32, size: 0, reserved: 0 }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..8].copy_from_slice(&self.status.to_le_bytes());
        buf[8..12].copy_from_slice(&self.size.to_le_bytes());
        buf[12..16].copy_from_slice(&self.reserved.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        Self {
            magic: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            status: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            size: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            reserved: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status == SanStatus::Ok as u32
    }
}
