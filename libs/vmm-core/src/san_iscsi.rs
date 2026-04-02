//! SAN iSCSI Block Protocol — binary protocol for block I/O via Unix Domain Socket.
//!
//! Used by vmm-iscsi to communicate with vmm-san for iSCSI block storage operations.
//! Flat LBA-based I/O without filesystem concepts.
//!
//! Wire format: fixed-size header + optional data payload.
//! All integers are little-endian.

/// Protocol magic for iSCSI block requests
pub const ISCSI_REQUEST_MAGIC: u32 = 0x49534353; // "ISCS"
/// Protocol magic for iSCSI block responses
pub const ISCSI_RESPONSE_MAGIC: u32 = 0x49534352; // "ISCR"

/// Socket path template for iSCSI block sockets: `$VMM_SAN_SOCK_DIR/blk-{volume_id}.sock`
pub fn block_socket_path(volume_id: &str) -> String {
    format!("{}/blk-{}.sock", crate::san_disk::socket_dir(), volume_id)
}

/// Block I/O commands
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IscsiCommand {
    /// Read bytes at LBA offset. Response body = data.
    ReadBlocks = 1,
    /// Write data at LBA offset. Payload = data.
    WriteBlocks = 2,
    /// Flush pending writes to disk.
    Flush = 3,
    /// Get volume capacity. Response body = JSON {size_bytes, block_size}.
    GetCapacity = 4,
    /// Get ALUA state for this volume on this node. Response body = JSON {state, tpg_id}.
    GetAluaState = 5,
}

impl IscsiCommand {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::ReadBlocks),
            2 => Some(Self::WriteBlocks),
            3 => Some(Self::Flush),
            4 => Some(Self::GetCapacity),
            5 => Some(Self::GetAluaState),
            _ => None,
        }
    }
}

/// Response status codes
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IscsiStatus {
    Ok = 0,
    NotFound = 1,
    IoError = 2,
    OutOfRange = 3,
    ProtocolError = 4,
    NoSpace = 5,
}

impl IscsiStatus {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Ok),
            1 => Some(Self::NotFound),
            2 => Some(Self::IoError),
            3 => Some(Self::OutOfRange),
            4 => Some(Self::ProtocolError),
            5 => Some(Self::NoSpace),
            _ => None,
        }
    }
}

/// Fixed-size iSCSI block request header (32 bytes).
///
/// Followed by: `length` bytes of data (for WriteBlocks).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct IscsiRequestHeader {
    pub magic: u32,
    pub cmd: u32,
    pub lba: u64,
    pub length: u32,
    pub flags: u32,
    pub _reserved: u64,
}

impl IscsiRequestHeader {
    pub const SIZE: usize = 32;

    pub fn new(cmd: IscsiCommand, lba: u64, length: u32) -> Self {
        Self {
            magic: ISCSI_REQUEST_MAGIC,
            cmd: cmd as u32,
            lba,
            length,
            flags: 0,
            _reserved: 0,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..8].copy_from_slice(&self.cmd.to_le_bytes());
        buf[8..16].copy_from_slice(&self.lba.to_le_bytes());
        buf[16..20].copy_from_slice(&self.length.to_le_bytes());
        buf[20..24].copy_from_slice(&self.flags.to_le_bytes());
        buf[24..32].copy_from_slice(&self._reserved.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        Self {
            magic: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            cmd: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            lba: u64::from_le_bytes([buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]]),
            length: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            flags: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            _reserved: u64::from_le_bytes([buf[24], buf[25], buf[26], buf[27], buf[28], buf[29], buf[30], buf[31]]),
        }
    }
}

/// Fixed-size iSCSI block response header (16 bytes).
///
/// Followed by: `length` bytes of data (for ReadBlocks, GetCapacity, GetAluaState).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct IscsiResponseHeader {
    pub magic: u32,
    pub status: u32,
    pub length: u32,
    pub _reserved: u32,
}

impl IscsiResponseHeader {
    pub const SIZE: usize = 16;

    pub fn ok(data_length: u32) -> Self {
        Self {
            magic: ISCSI_RESPONSE_MAGIC,
            status: IscsiStatus::Ok as u32,
            length: data_length,
            _reserved: 0,
        }
    }

    pub fn err(status: IscsiStatus) -> Self {
        Self {
            magic: ISCSI_RESPONSE_MAGIC,
            status: status as u32,
            length: 0,
            _reserved: 0,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..8].copy_from_slice(&self.status.to_le_bytes());
        buf[8..12].copy_from_slice(&self.length.to_le_bytes());
        buf[12..16].copy_from_slice(&self._reserved.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        Self {
            magic: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            status: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            length: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            _reserved: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status == IscsiStatus::Ok as u32
    }
}
