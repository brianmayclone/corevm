//! SAN Management Protocol — binary protocol for management operations via Unix Domain Socket.
//!
//! Used by vmm-s3gw for auth validation, volume listing, and credential management.
//! Single socket at /run/vmm-san/mgmt.sock (not per-volume).

/// Protocol magic for management requests
pub const MGMT_REQUEST_MAGIC: u32 = 0x4D474D54; // "MGMT"
/// Protocol magic for management responses
pub const MGMT_RESPONSE_MAGIC: u32 = 0x4D474D52; // "MGMR"

/// Management socket path
pub const MGMT_SOCKET_PATH: &str = "/run/vmm-san/mgmt.sock";

/// Management commands
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MgmtCommand {
    /// List volumes with "s3" in access_protocols. Response body = JSON array.
    ListVolumes = 1,
    /// Create a volume. Body = JSON {name, max_size_bytes, ftt, local_raid, chunk_size_bytes}.
    CreateVolume = 2,
    /// Delete a volume. Key = volume_id.
    DeleteVolume = 3,
    /// Create S3 credential. Body = JSON {user_id, display_name}. Response = JSON {access_key, secret_key}.
    CreateCredential = 20,
    /// Validate S3 auth. Body = JSON {access_key, string_to_sign, signature, region, date}.
    /// Response: Ok = valid, AccessDenied = invalid.
    ValidateCredential = 21,
    /// List S3 credentials. Response body = JSON array.
    ListCredentials = 22,
    /// Delete S3 credential. Key = credential_id.
    DeleteCredential = 23,
    /// Resolve volume name to id. Key = volume_name. Response metadata = JSON {id, name, status}.
    ResolveVolume = 30,
}

impl MgmtCommand {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::ListVolumes),
            2 => Some(Self::CreateVolume),
            3 => Some(Self::DeleteVolume),
            20 => Some(Self::CreateCredential),
            21 => Some(Self::ValidateCredential),
            22 => Some(Self::ListCredentials),
            23 => Some(Self::DeleteCredential),
            30 => Some(Self::ResolveVolume),
            _ => None,
        }
    }
}

/// Management response status codes
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MgmtStatus {
    Ok = 0,
    NotFound = 1,
    AccessDenied = 2,
    AlreadyExists = 3,
    InvalidRequest = 4,
    InternalError = 5,
}

impl MgmtStatus {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Ok),
            1 => Some(Self::NotFound),
            2 => Some(Self::AccessDenied),
            3 => Some(Self::AlreadyExists),
            4 => Some(Self::InvalidRequest),
            5 => Some(Self::InternalError),
            _ => None,
        }
    }
}

/// Fixed-size management request header (28 bytes).
/// Same layout as ObjectRequestHeader for consistency.
///
/// Followed by: key_len bytes of key, then body_len bytes of body.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MgmtRequestHeader {
    pub magic: u32,
    pub cmd: u32,
    pub key_len: u32,
    pub body_len: u64,
    pub flags: u32,
    pub _reserved: u32,
}

impl MgmtRequestHeader {
    pub const SIZE: usize = 28;

    pub fn new(cmd: MgmtCommand, key_len: u32, body_len: u64) -> Self {
        Self {
            magic: MGMT_REQUEST_MAGIC,
            cmd: cmd as u32,
            key_len,
            body_len,
            flags: 0,
            _reserved: 0,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..8].copy_from_slice(&self.cmd.to_le_bytes());
        buf[8..12].copy_from_slice(&self.key_len.to_le_bytes());
        buf[12..20].copy_from_slice(&self.body_len.to_le_bytes());
        buf[20..24].copy_from_slice(&self.flags.to_le_bytes());
        buf[24..28].copy_from_slice(&self._reserved.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        Self {
            magic: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            cmd: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            key_len: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            body_len: u64::from_le_bytes([buf[12], buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19]]),
            flags: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            _reserved: u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
        }
    }
}

/// Fixed-size management response header (24 bytes).
/// Same layout as ObjectResponseHeader for consistency.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MgmtResponseHeader {
    pub magic: u32,
    pub status: u32,
    pub body_len: u64,
    pub metadata_len: u32,
    pub _reserved: u32,
}

impl MgmtResponseHeader {
    pub const SIZE: usize = 24;

    pub fn ok(body_len: u64, metadata_len: u32) -> Self {
        Self {
            magic: MGMT_RESPONSE_MAGIC,
            status: MgmtStatus::Ok as u32,
            body_len,
            metadata_len,
            _reserved: 0,
        }
    }

    pub fn err(status: MgmtStatus) -> Self {
        Self {
            magic: MGMT_RESPONSE_MAGIC,
            status: status as u32,
            body_len: 0,
            metadata_len: 0,
            _reserved: 0,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..8].copy_from_slice(&self.status.to_le_bytes());
        buf[8..16].copy_from_slice(&self.body_len.to_le_bytes());
        buf[16..20].copy_from_slice(&self.metadata_len.to_le_bytes());
        buf[20..24].copy_from_slice(&self._reserved.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        Self {
            magic: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            status: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            body_len: u64::from_le_bytes([buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]]),
            metadata_len: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            _reserved: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status == MgmtStatus::Ok as u32
    }
}
