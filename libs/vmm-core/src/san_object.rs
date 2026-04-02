//! SAN Object Protocol — binary protocol for object storage I/O via Unix Domain Socket.
//!
//! Used by vmm-s3gw to communicate with vmm-san for S3-compatible object operations.
//! Analogous to san_disk.rs but key-based instead of offset-based.
//!
//! Wire format: fixed-size header + key bytes + body bytes.
//! All integers are little-endian.

/// Protocol magic for object requests
pub const OBJ_REQUEST_MAGIC: u32 = 0x4F424A53; // "OBJS"
/// Protocol magic for object responses
pub const OBJ_RESPONSE_MAGIC: u32 = 0x4F424A52; // "OBJR"

/// Socket path template for object sockets: `$VMM_SAN_SOCK_DIR/obj-{volume_id}.sock`
pub fn object_socket_path(volume_id: &str) -> String {
    format!("{}/obj-{}.sock", crate::san_disk::socket_dir(), volume_id)
}

/// Object storage commands
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ObjectCommand {
    /// Store an object. Key = object key, Body = object data.
    Put = 1,
    /// Retrieve an object. Key = object key. Response body = object data.
    Get = 2,
    /// Get object metadata only. Key = object key. Response metadata = JSON.
    Head = 3,
    /// Delete an object. Key = object key.
    Delete = 4,
    /// List objects by prefix. Key = JSON {prefix, marker, max_keys}. Response body = JSON array.
    List = 5,
    /// Copy an object. Key = JSON {src_key, dst_key}. Server-side copy.
    Copy = 6,
    /// Initiate multipart upload. Key = object key. Response metadata = JSON {upload_id}.
    InitMultipart = 7,
    /// Upload a part. Key = JSON {upload_id, part_number}. Body = part data.
    UploadPart = 8,
    /// Complete multipart upload. Key = JSON {upload_id, parts: [{part_number, etag}]}.
    CompleteMultipart = 9,
    /// Abort multipart upload. Key = JSON {upload_id}.
    AbortMultipart = 10,
}

impl ObjectCommand {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Put),
            2 => Some(Self::Get),
            3 => Some(Self::Head),
            4 => Some(Self::Delete),
            5 => Some(Self::List),
            6 => Some(Self::Copy),
            7 => Some(Self::InitMultipart),
            8 => Some(Self::UploadPart),
            9 => Some(Self::CompleteMultipart),
            10 => Some(Self::AbortMultipart),
            _ => None,
        }
    }
}

/// Object response status codes
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ObjectStatus {
    Ok = 0,
    NotFound = 1,
    AccessDenied = 2,
    AlreadyExists = 3,
    InvalidKey = 4,
    NoSpace = 5,
    LeaseDenied = 6,
    IoError = 7,
    ProtocolError = 8,
}

impl ObjectStatus {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Ok),
            1 => Some(Self::NotFound),
            2 => Some(Self::AccessDenied),
            3 => Some(Self::AlreadyExists),
            4 => Some(Self::InvalidKey),
            5 => Some(Self::NoSpace),
            6 => Some(Self::LeaseDenied),
            7 => Some(Self::IoError),
            8 => Some(Self::ProtocolError),
            _ => None,
        }
    }
}

/// Fixed-size object request header (28 bytes).
///
/// Followed by: key_len bytes of key, then body_len bytes of body.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ObjectRequestHeader {
    pub magic: u32,
    pub cmd: u32,
    pub key_len: u32,
    pub body_len: u64,
    pub flags: u32,
    pub _reserved: u32,
}

impl ObjectRequestHeader {
    pub const SIZE: usize = 28;

    pub fn new(cmd: ObjectCommand, key_len: u32, body_len: u64) -> Self {
        Self {
            magic: OBJ_REQUEST_MAGIC,
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

/// Fixed-size object response header (24 bytes).
///
/// Followed by: metadata_len bytes of JSON metadata, then body_len bytes of body.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ObjectResponseHeader {
    pub magic: u32,
    pub status: u32,
    pub body_len: u64,
    pub metadata_len: u32,
    pub _reserved: u32,
}

impl ObjectResponseHeader {
    pub const SIZE: usize = 24;

    pub fn ok(body_len: u64, metadata_len: u32) -> Self {
        Self {
            magic: OBJ_RESPONSE_MAGIC,
            status: ObjectStatus::Ok as u32,
            body_len,
            metadata_len,
            _reserved: 0,
        }
    }

    pub fn err(status: ObjectStatus) -> Self {
        Self {
            magic: OBJ_RESPONSE_MAGIC,
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
        self.status == ObjectStatus::Ok as u32
    }
}
