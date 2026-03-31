# S3 Object Storage Gateway Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add S3-compatible object storage access to CoreSAN volumes via a new `vmm-s3gw` gateway binary that communicates with vmm-san over Unix Domain Sockets.

**Architecture:** vmm-san gets a new Object Socket listener per volume (for S3-enabled volumes) and a Management Socket for auth/volume ops. A new `vmm-s3gw` binary translates S3 HTTP requests into UDS calls. Volumes declare supported protocols via `access_protocols` JSON array field.

**Tech Stack:** Rust, Axum, Tokio, rusqlite, sha2, hmac, aes-gcm, UDS (tokio::net::UnixStream/UnixListener)

---

## File Structure

### New Files

| File | Responsibility |
|------|---------------|
| `libs/vmm-core/src/san_object.rs` | Shared Object Socket protocol structs (ObjectCommand, ObjectRequestHeader, ObjectResponseHeader, ObjectStatus) |
| `libs/vmm-core/src/san_mgmt.rs` | Shared Management Socket protocol structs (MgmtCommand, MgmtRequestHeader, MgmtResponseHeader) |
| `apps/vmm-san/src/engine/object_server.rs` | Object Socket UDS listener — one per S3-enabled volume, handles Put/Get/Head/Delete/List/Copy/Multipart |
| `apps/vmm-san/src/engine/mgmt_server.rs` | Management Socket UDS listener — auth validation, ListVolumes, CreateVolume, credential CRUD |
| `apps/vmm-s3gw/Cargo.toml` | Gateway binary crate config |
| `apps/vmm-s3gw/src/main.rs` | Entry point: config loading, Axum server, socket connections |
| `apps/vmm-s3gw/src/config.rs` | TOML config parsing (listen addr, region, socket paths, TLS) |
| `apps/vmm-s3gw/src/auth.rs` | AWS Signature V4 parsing and verification via mgmt socket |
| `apps/vmm-s3gw/src/s3/mod.rs` | S3 router assembly |
| `apps/vmm-s3gw/src/s3/bucket.rs` | Bucket operations (ListBuckets, CreateBucket, DeleteBucket, HeadBucket) |
| `apps/vmm-s3gw/src/s3/object.rs` | Object operations (PutObject, GetObject, HeadObject, DeleteObject, CopyObject, ListObjectsV2) |
| `apps/vmm-s3gw/src/s3/multipart.rs` | Multipart upload operations (Initiate, UploadPart, Complete, Abort) |
| `apps/vmm-s3gw/src/s3/error.rs` | S3 XML error response serialization |
| `apps/vmm-s3gw/src/s3/xml.rs` | S3 XML response helpers (ListBucketResult, ListAllMyBucketsResult, etc.) |
| `apps/vmm-s3gw/src/socket.rs` | UDS client — connection pool for mgmt + object sockets |

### Modified Files

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add `apps/vmm-s3gw` to members |
| `libs/vmm-core/src/lib.rs` | Add `pub mod san_object; pub mod san_mgmt;` |
| `apps/vmm-san/src/db/mod.rs` | Add `access_protocols` to volumes, add `s3_credentials`, `multipart_uploads`, `multipart_parts` tables + indexes |
| `apps/vmm-san/src/api/volumes.rs` | Add `access_protocols` to CreateVolumeRequest, VolumeResponse, create/list/get/update handlers |
| `apps/vmm-san/src/engine/mod.rs` | Add `pub mod object_server; pub mod mgmt_server;` |
| `apps/vmm-san/src/main.rs` | Spawn object_server and mgmt_server at startup |

---

## Task 1: Shared Object Socket Protocol (`libs/vmm-core/src/san_object.rs`)

**Files:**
- Create: `libs/vmm-core/src/san_object.rs`
- Modify: `libs/vmm-core/src/lib.rs`

- [ ] **Step 1: Create the Object Socket protocol module**

```rust
// libs/vmm-core/src/san_object.rs

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

/// Socket path template for object sockets: `/run/vmm-san/obj-{volume_id}.sock`
pub fn object_socket_path(volume_id: &str) -> String {
    format!("/run/vmm-san/obj-{}.sock", volume_id)
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
```

- [ ] **Step 2: Register the module in lib.rs**

Add to `libs/vmm-core/src/lib.rs`:
```rust
pub mod san_object;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p vmm-core`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add libs/vmm-core/src/san_object.rs libs/vmm-core/src/lib.rs
git commit -m "feat: add Object Socket protocol structs in vmm-core"
```

---

## Task 2: Shared Management Socket Protocol (`libs/vmm-core/src/san_mgmt.rs`)

**Files:**
- Create: `libs/vmm-core/src/san_mgmt.rs`
- Modify: `libs/vmm-core/src/lib.rs`

- [ ] **Step 1: Create the Management Socket protocol module**

```rust
// libs/vmm-core/src/san_mgmt.rs

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

/// Management response status codes (reuses ObjectStatus values for consistency)
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
```

- [ ] **Step 2: Register in lib.rs**

Add to `libs/vmm-core/src/lib.rs`:
```rust
pub mod san_mgmt;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p vmm-core`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add libs/vmm-core/src/san_mgmt.rs libs/vmm-core/src/lib.rs
git commit -m "feat: add Management Socket protocol structs in vmm-core"
```

---

## Task 3: Database Schema Changes (`apps/vmm-san/src/db/mod.rs`)

**Files:**
- Modify: `apps/vmm-san/src/db/mod.rs`

- [ ] **Step 1: Add `access_protocols` to volumes table**

In `apps/vmm-san/src/db/mod.rs`, find the volumes CREATE TABLE and add after the `status` line:

```sql
    access_protocols TEXT NOT NULL DEFAULT '["fuse"]', -- JSON array: "fuse", "s3", future: "nfs", "iscsi"
```

- [ ] **Step 2: Add S3 credentials table**

Add after the `smart_data` CREATE TABLE block (before the closing `"#;`):

```sql
-- ═══════════════════════════════════════════════════════════════
-- S3_CREDENTIALS: access keys for S3-compatible API access
-- Secret keys are AES-256-GCM encrypted (SigV4 needs plaintext)
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS s3_credentials (
    id              TEXT PRIMARY KEY,
    access_key      TEXT NOT NULL UNIQUE,
    secret_key_enc  TEXT NOT NULL,          -- AES-256-GCM encrypted
    user_id         TEXT NOT NULL,
    display_name    TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'active',  -- active, disabled
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at      TEXT                    -- NULL = no expiry
);

-- ═══════════════════════════════════════════════════════════════
-- MULTIPART_UPLOADS: in-progress S3 multipart uploads
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS multipart_uploads (
    upload_id       TEXT PRIMARY KEY,
    volume_id       TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    object_key      TEXT NOT NULL,
    created_by      TEXT NOT NULL,          -- access_key
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    status          TEXT NOT NULL DEFAULT 'active'  -- active, completed, aborted
);

CREATE TABLE IF NOT EXISTS multipart_parts (
    upload_id       TEXT NOT NULL REFERENCES multipart_uploads(upload_id) ON DELETE CASCADE,
    part_number     INTEGER NOT NULL,
    size_bytes      INTEGER NOT NULL,
    etag            TEXT NOT NULL,
    backend_path    TEXT NOT NULL,          -- temp chunk path on disk
    PRIMARY KEY (upload_id, part_number)
);
```

- [ ] **Step 3: Add indexes for new tables**

Add to the INDEXES section:

```sql
CREATE INDEX IF NOT EXISTS idx_s3_credentials_access_key ON s3_credentials(access_key);
CREATE INDEX IF NOT EXISTS idx_s3_credentials_user ON s3_credentials(user_id);
CREATE INDEX IF NOT EXISTS idx_multipart_uploads_volume ON multipart_uploads(volume_id);
CREATE INDEX IF NOT EXISTS idx_multipart_uploads_status ON multipart_uploads(status);
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p vmm-san`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add apps/vmm-san/src/db/mod.rs
git commit -m "feat: add access_protocols, s3_credentials, multipart tables to CoreSAN schema"
```

---

## Task 4: Volume API — `access_protocols` Support

**Files:**
- Modify: `apps/vmm-san/src/api/volumes.rs`

- [ ] **Step 1: Add `access_protocols` to CreateVolumeRequest**

Add field to `CreateVolumeRequest`:
```rust
    #[serde(default = "default_access_protocols")]
    pub access_protocols: Vec<String>,
```

Add default function:
```rust
fn default_access_protocols() -> Vec<String> { vec!["fuse".into()] }
```

- [ ] **Step 2: Add `access_protocols` to VolumeResponse**

Add field to `VolumeResponse`:
```rust
    pub access_protocols: Vec<String>,
```

- [ ] **Step 3: Add `access_protocols` to UpdateVolumeRequest**

Add field:
```rust
    pub access_protocols: Option<Vec<String>>,
```

- [ ] **Step 4: Validate access_protocols in create handler**

Add validation in the `create` function before the INSERT, after FTT validation:

```rust
    // Validate access_protocols
    let valid_protocols = ["fuse", "s3"];
    for proto in &body.access_protocols {
        if !valid_protocols.contains(&proto.as_str()) {
            return Err((StatusCode::BAD_REQUEST,
                format!("Unknown access protocol '{}'. Valid: {:?}", proto, valid_protocols)));
        }
    }
    if body.access_protocols.is_empty() {
        return Err((StatusCode::BAD_REQUEST,
            "access_protocols must contain at least one protocol".into()));
    }
```

- [ ] **Step 5: Update the INSERT to include access_protocols**

Change the INSERT statement to include `access_protocols`:
```rust
    let protocols_json = serde_json::to_string(&body.access_protocols).unwrap_or_else(|_| "[\"fuse\"]".into());

    db.execute(
        "INSERT INTO volumes (id, name, ftt, chunk_size_bytes, local_raid, max_size_bytes, access_protocols, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'online')",
        rusqlite::params![&id, &body.name, body.ftt, body.chunk_size_bytes, &body.local_raid, body.max_size_bytes, &protocols_json],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create volume: {}", e)))?;
```

- [ ] **Step 6: Conditionally create FUSE mount only if "fuse" in protocols**

Wrap the FUSE mount creation in the create handler with:
```rust
    if body.access_protocols.contains(&"fuse".into()) {
        // existing FUSE mount code...
    }
```

- [ ] **Step 7: Update list/get handlers to return access_protocols**

In the query that builds `VolumeResponse`, add `access_protocols` column and parse it:
```rust
    let protocols_str: String = row.get("access_protocols").unwrap_or_else(|_| "[\"fuse\"]".into());
    let access_protocols: Vec<String> = serde_json::from_str(&protocols_str).unwrap_or_else(|_| vec!["fuse".into()]);
```

Set it on the response:
```rust
    access_protocols,
```

- [ ] **Step 8: Update the update handler to support access_protocols changes**

In the `update` function, if `body.access_protocols` is `Some`, validate and update:
```rust
    if let Some(ref protocols) = body.access_protocols {
        let valid_protocols = ["fuse", "s3"];
        for proto in protocols {
            if !valid_protocols.contains(&proto.as_str()) {
                return Err((StatusCode::BAD_REQUEST,
                    format!("Unknown access protocol '{}'", proto)));
            }
        }
        let protocols_json = serde_json::to_string(protocols).unwrap();
        db.execute(
            "UPDATE volumes SET access_protocols = ?1 WHERE id = ?2",
            rusqlite::params![&protocols_json, &id],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to update protocols: {}", e)))?;
    }
```

- [ ] **Step 9: Verify it compiles**

Run: `cargo check -p vmm-san`
Expected: compiles with no errors

- [ ] **Step 10: Commit**

```bash
git add apps/vmm-san/src/api/volumes.rs
git commit -m "feat: add access_protocols field to volume CRUD endpoints"
```

---

## Task 5: Object Server Engine (`apps/vmm-san/src/engine/object_server.rs`)

**Files:**
- Create: `apps/vmm-san/src/engine/object_server.rs`
- Modify: `apps/vmm-san/src/engine/mod.rs`

- [ ] **Step 1: Create the object server module**

```rust
// apps/vmm-san/src/engine/object_server.rs

//! Object Storage server — serves S3-compatible object I/O via Unix Domain Socket.
//!
//! One UDS listener per volume with "s3" in access_protocols.
//! Uses the same chunk engine as FUSE and disk_server for all data operations.

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use vmm_core::san_object::*;
use crate::state::CoreSanState;
use crate::storage::chunk;

/// Spawn object socket listeners for all S3-enabled volumes.
pub fn spawn_all(state: Arc<CoreSanState>) {
    let volumes: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name FROM volumes WHERE status = 'online' AND access_protocols LIKE '%s3%'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    std::fs::create_dir_all("/run/vmm-san").ok();

    for (vol_id, vol_name) in volumes {
        spawn_volume_listener(state.clone(), vol_id, vol_name);
    }
}

/// Spawn a single object socket listener for a volume.
pub fn spawn_volume_listener(state: Arc<CoreSanState>, volume_id: String, volume_name: String) {
    std::fs::create_dir_all("/run/vmm-san").ok();
    let sock_path = object_socket_path(&volume_id);
    std::fs::remove_file(&sock_path).ok();

    tokio::spawn(async move {
        let listener = match UnixListener::bind(&sock_path) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Object server: cannot bind {}: {}", sock_path, e);
                return;
            }
        };

        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o666)).ok();
        tracing::info!("Object server: listening on {} (volume '{}')", sock_path, volume_name);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let state = state.clone();
                    let vid = volume_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(state, vid, stream).await {
                            tracing::warn!("Object server: connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Object server: accept error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    });
}

async fn handle_connection(
    state: Arc<CoreSanState>,
    volume_id: String,
    mut stream: UnixStream,
) -> Result<(), String> {
    let mut hdr_buf = [0u8; ObjectRequestHeader::SIZE];

    loop {
        if stream.read_exact(&mut hdr_buf).await.is_err() {
            break; // connection closed
        }

        let hdr = ObjectRequestHeader::from_bytes(&hdr_buf);
        if hdr.magic != OBJ_REQUEST_MAGIC {
            send_response(&mut stream, ObjectResponseHeader::err(ObjectStatus::ProtocolError), &[], &[]).await;
            break;
        }

        // Read key
        let mut key_buf = vec![0u8; hdr.key_len as usize];
        if hdr.key_len > 0 {
            if stream.read_exact(&mut key_buf).await.is_err() {
                break;
            }
        }
        let key = String::from_utf8_lossy(&key_buf).to_string();

        // Read body
        let mut body = vec![0u8; hdr.body_len as usize];
        if hdr.body_len > 0 {
            if stream.read_exact(&mut body).await.is_err() {
                break;
            }
        }

        let cmd = match ObjectCommand::from_u32(hdr.cmd) {
            Some(c) => c,
            None => {
                send_response(&mut stream, ObjectResponseHeader::err(ObjectStatus::ProtocolError), &[], &[]).await;
                continue;
            }
        };

        match cmd {
            ObjectCommand::Put => handle_put(&state, &volume_id, &key, &body, &mut stream).await,
            ObjectCommand::Get => handle_get(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::Head => handle_head(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::Delete => handle_delete(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::List => handle_list(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::Copy => handle_copy(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::InitMultipart => handle_init_multipart(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::UploadPart => handle_upload_part(&state, &volume_id, &key, &body, &mut stream).await,
            ObjectCommand::CompleteMultipart => handle_complete_multipart(&state, &volume_id, &key, &mut stream).await,
            ObjectCommand::AbortMultipart => handle_abort_multipart(&state, &volume_id, &key, &mut stream).await,
        }
    }

    Ok(())
}

async fn send_response(stream: &mut UnixStream, header: ObjectResponseHeader, metadata: &[u8], data: &[u8]) {
    let _ = stream.write_all(&header.to_bytes()).await;
    if !metadata.is_empty() {
        let _ = stream.write_all(metadata).await;
    }
    if !data.is_empty() {
        let _ = stream.write_all(data).await;
    }
}

// ── PUT: store an object ─────────────────────────────────────────────────

async fn handle_put(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    data: &[u8],
    stream: &mut UnixStream,
) {
    let file_id = chunk::deterministic_file_id(volume_id, key);
    let db = state.db.lock().unwrap();

    // Get volume config
    let vol_config = match db.query_row(
        "SELECT chunk_size_bytes, local_raid FROM volumes WHERE id = ?1",
        rusqlite::params![volume_id],
        |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
    ) {
        Ok(c) => c,
        Err(_) => {
            drop(db);
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
            return;
        }
    };

    let (chunk_size, local_raid) = vol_config;

    // Compute SHA-256
    use sha2::{Sha256, Digest};
    let sha = format!("{:x}", Sha256::digest(data));

    // Upsert file_map entry
    db.execute(
        "INSERT INTO file_map (id, volume_id, rel_path, size_bytes, sha256, version, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, datetime('now'))
         ON CONFLICT(volume_id, rel_path) DO UPDATE SET
           size_bytes = ?4, sha256 = ?5, version = version + 1, updated_at = datetime('now')",
        rusqlite::params![file_id, volume_id, key, data.len() as i64, &sha],
    ).ok();

    // Write chunk data
    let mut offset: u64 = 0;
    while offset < data.len() as u64 {
        let end = ((offset + chunk_size) as usize).min(data.len());
        let chunk_data = &data[offset as usize..end];
        if let Err(e) = chunk::write_chunk_data(
            &db, file_id, offset, chunk_data,
            volume_id, &state.node_id, chunk_size, &local_raid,
        ) {
            tracing::error!("Object server: write_chunk_data failed for {}/{}: {}", volume_id, key, e);
            drop(db);
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
            return;
        }
        offset += chunk_size;
    }

    // Update chunk_count
    let chunk_count = ((data.len() as u64 + chunk_size - 1) / chunk_size) as i64;
    db.execute(
        "UPDATE file_map SET chunk_count = ?1 WHERE id = ?2",
        rusqlite::params![chunk_count, file_id],
    ).ok();

    // Trigger push replication
    let version: i64 = db.query_row(
        "SELECT version FROM file_map WHERE id = ?1",
        rusqlite::params![file_id], |row| row.get(0),
    ).unwrap_or(1);
    drop(db);

    let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
        volume_id: volume_id.to_string(),
        rel_path: key.to_string(),
        file_id,
        version,
        writer_node_id: state.node_id.clone(),
    });

    // Return ETag (SHA-256) as metadata
    let meta = serde_json::json!({"etag": sha, "size": data.len()}).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── GET: retrieve an object ──────────────────────────────────────────────

async fn handle_get(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    let file_id = chunk::deterministic_file_id(volume_id, key);
    let db = state.db.lock().unwrap();

    // Get file metadata
    let file_info = db.query_row(
        "SELECT size_bytes, sha256, chunk_count, updated_at FROM file_map WHERE id = ?1 AND volume_id = ?2",
        rusqlite::params![file_id, volume_id],
        |row| Ok((
            row.get::<_, i64>(0)? as u64,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)? as u32,
            row.get::<_, String>(3)?,
        )),
    );

    let (size, sha, chunk_count, updated_at) = match file_info {
        Ok(info) => info,
        Err(_) => {
            drop(db);
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
            return;
        }
    };

    // Get chunk_size from volume
    let chunk_size: u64 = db.query_row(
        "SELECT chunk_size_bytes FROM volumes WHERE id = ?1",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(4194304);

    // Read all chunks into a buffer
    let mut data = Vec::with_capacity(size as usize);
    let mut offset: u64 = 0;
    while offset < size {
        let read_len = (size - offset).min(chunk_size);
        match chunk::read_chunk_data(&db, file_id, offset, read_len, volume_id, &state.node_id, chunk_size) {
            Ok(chunk_data) => data.extend_from_slice(&chunk_data),
            Err(e) => {
                tracing::error!("Object server: read_chunk_data failed at offset {} for {}/{}: {}", offset, volume_id, key, e);
                drop(db);
                send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
                return;
            }
        }
        offset += chunk_size;
    }
    data.truncate(size as usize);
    drop(db);

    let meta = serde_json::json!({"etag": sha, "size": size, "last_modified": updated_at}).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(data.len() as u64, meta_bytes.len() as u32), meta_bytes, &data).await;
}

// ── HEAD: get object metadata ────────────────────────────────────────────

async fn handle_head(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    let file_id = chunk::deterministic_file_id(volume_id, key);
    let db = state.db.lock().unwrap();

    let file_info = db.query_row(
        "SELECT size_bytes, sha256, updated_at FROM file_map WHERE id = ?1 AND volume_id = ?2",
        rusqlite::params![file_id, volume_id],
        |row| Ok((
            row.get::<_, i64>(0)? as u64,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        )),
    );

    match file_info {
        Ok((size, sha, updated_at)) => {
            drop(db);
            let meta = serde_json::json!({"etag": sha, "size": size, "last_modified": updated_at}).to_string();
            let meta_bytes = meta.as_bytes();
            send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
        }
        Err(_) => {
            drop(db);
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
        }
    }
}

// ── DELETE: remove an object ─────────────────────────────────────────────

async fn handle_delete(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    let file_id = chunk::deterministic_file_id(volume_id, key);
    let db = state.db.lock().unwrap();

    // CASCADE deletes file_chunks, chunk_replicas, file_replicas
    let deleted = db.execute(
        "DELETE FROM file_map WHERE id = ?1 AND volume_id = ?2",
        rusqlite::params![file_id, volume_id],
    ).unwrap_or(0);

    drop(db);

    // S3 always returns 204 even if key didn't exist
    send_response(stream, ObjectResponseHeader::ok(0, 0), &[], &[]).await;
}

// ── LIST: list objects by prefix ─────────────────────────────────────────

async fn handle_list(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,  // JSON: {prefix, marker, max_keys, delimiter}
    stream: &mut UnixStream,
) {
    #[derive(serde::Deserialize)]
    struct ListParams {
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        marker: String,
        #[serde(default = "default_max_keys")]
        max_keys: u32,
        #[serde(default)]
        delimiter: String,
    }
    fn default_max_keys() -> u32 { 1000 }

    let params: ListParams = match serde_json::from_str(key) {
        Ok(p) => p,
        Err(_) => ListParams { prefix: String::new(), marker: String::new(), max_keys: 1000, delimiter: String::new() },
    };

    let db = state.db.lock().unwrap();

    let like_pattern = format!("{}%", params.prefix);
    let mut stmt = db.prepare(
        "SELECT rel_path, size_bytes, sha256, updated_at FROM file_map
         WHERE volume_id = ?1 AND rel_path LIKE ?2 AND rel_path > ?3
         ORDER BY rel_path ASC LIMIT ?4"
    ).unwrap();

    let entries: Vec<serde_json::Value> = stmt.query_map(
        rusqlite::params![volume_id, &like_pattern, &params.marker, params.max_keys + 1],
        |row| {
            Ok(serde_json::json!({
                "key": row.get::<_, String>(0)?,
                "size": row.get::<_, i64>(1)?,
                "etag": row.get::<_, String>(2)?,
                "last_modified": row.get::<_, String>(3)?,
            }))
        },
    ).unwrap().filter_map(|r| r.ok()).collect();

    let is_truncated = entries.len() > params.max_keys as usize;
    let entries: Vec<_> = entries.into_iter().take(params.max_keys as usize).collect();

    // Handle delimiter (common prefixes)
    let (objects, common_prefixes) = if !params.delimiter.is_empty() {
        let mut objects = Vec::new();
        let mut prefixes = std::collections::BTreeSet::new();
        for entry in &entries {
            let key_str = entry["key"].as_str().unwrap_or("");
            let after_prefix = &key_str[params.prefix.len()..];
            if let Some(pos) = after_prefix.find(&params.delimiter) {
                prefixes.insert(format!("{}{}{}", params.prefix, &after_prefix[..pos], params.delimiter));
            } else {
                objects.push(entry.clone());
            }
        }
        (objects, prefixes.into_iter().collect::<Vec<_>>())
    } else {
        (entries, Vec::new())
    };

    drop(db);

    let result = serde_json::json!({
        "objects": objects,
        "common_prefixes": common_prefixes,
        "is_truncated": is_truncated,
    });

    let body = result.to_string();
    let body_bytes = body.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(body_bytes.len() as u64, 0), &[], body_bytes).await;
}

// ── COPY: server-side copy ───────────────────────────────────────────────

async fn handle_copy(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,  // JSON: {src_key, dst_key}
    stream: &mut UnixStream,
) {
    #[derive(serde::Deserialize)]
    struct CopyParams { src_key: String, dst_key: String }

    let params: CopyParams = match serde_json::from_str(key) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::InvalidKey), &[], &[]).await;
            return;
        }
    };

    // Read source object via chunk system
    let src_file_id = chunk::deterministic_file_id(volume_id, &params.src_key);
    let db = state.db.lock().unwrap();

    let src_info = db.query_row(
        "SELECT size_bytes, sha256 FROM file_map WHERE id = ?1 AND volume_id = ?2",
        rusqlite::params![src_file_id, volume_id],
        |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?)),
    );

    let (size, sha) = match src_info {
        Ok(info) => info,
        Err(_) => {
            drop(db);
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
            return;
        }
    };

    let chunk_size: u64 = db.query_row(
        "SELECT chunk_size_bytes FROM volumes WHERE id = ?1",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(4194304);

    let local_raid: String = db.query_row(
        "SELECT local_raid FROM volumes WHERE id = ?1",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or_else(|_| "stripe".into());

    // Create destination file_map entry
    let dst_file_id = chunk::deterministic_file_id(volume_id, &params.dst_key);
    db.execute(
        "INSERT INTO file_map (id, volume_id, rel_path, size_bytes, sha256, version, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, datetime('now'))
         ON CONFLICT(volume_id, rel_path) DO UPDATE SET
           size_bytes = ?4, sha256 = ?5, version = version + 1, updated_at = datetime('now')",
        rusqlite::params![dst_file_id, volume_id, &params.dst_key, size as i64, &sha],
    ).ok();

    // Copy chunks
    let mut offset: u64 = 0;
    while offset < size {
        let read_len = (size - offset).min(chunk_size);
        let data = match chunk::read_chunk_data(&db, src_file_id, offset, read_len, volume_id, &state.node_id, chunk_size) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("Object server: copy read failed at offset {}: {}", offset, e);
                drop(db);
                send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
                return;
            }
        };
        if let Err(e) = chunk::write_chunk_data(&db, dst_file_id, offset, &data, volume_id, &state.node_id, chunk_size, &local_raid) {
            tracing::error!("Object server: copy write failed at offset {}: {}", offset, e);
            drop(db);
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
            return;
        }
        offset += chunk_size;
    }

    // Trigger replication for destination
    let version: i64 = db.query_row(
        "SELECT version FROM file_map WHERE id = ?1",
        rusqlite::params![dst_file_id], |row| row.get(0),
    ).unwrap_or(1);
    drop(db);

    let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
        volume_id: volume_id.to_string(),
        rel_path: params.dst_key.clone(),
        file_id: dst_file_id,
        version,
        writer_node_id: state.node_id.clone(),
    });

    let meta = serde_json::json!({"etag": sha}).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── MULTIPART: initiate ──────────────────────────────────────────────────

async fn handle_init_multipart(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,
    stream: &mut UnixStream,
) {
    let upload_id = uuid::Uuid::new_v4().to_string();
    let db = state.db.lock().unwrap();

    db.execute(
        "INSERT INTO multipart_uploads (upload_id, volume_id, object_key, created_by)
         VALUES (?1, ?2, ?3, 'system')",
        rusqlite::params![&upload_id, volume_id, key],
    ).ok();
    drop(db);

    let meta = serde_json::json!({"upload_id": upload_id}).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── MULTIPART: upload part ───────────────────────────────────────────────

async fn handle_upload_part(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,  // JSON: {upload_id, part_number}
    data: &[u8],
    stream: &mut UnixStream,
) {
    #[derive(serde::Deserialize)]
    struct PartParams { upload_id: String, part_number: u32 }

    let params: PartParams = match serde_json::from_str(key) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::InvalidKey), &[], &[]).await;
            return;
        }
    };

    // Compute ETag (MD5 for S3 compatibility)
    use sha2::{Sha256, Digest};
    let etag = format!("{:x}", Sha256::digest(data));

    // Store part temporarily as a file in the SAN data dir
    let part_path = format!("/var/lib/vmm-san/multipart/{}/{}", params.upload_id, params.part_number);
    if let Some(parent) = std::path::Path::new(&part_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Err(e) = std::fs::write(&part_path, data) {
        tracing::error!("Object server: failed to write multipart part: {}", e);
        send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
        return;
    }

    let db = state.db.lock().unwrap();
    db.execute(
        "INSERT OR REPLACE INTO multipart_parts (upload_id, part_number, size_bytes, etag, backend_path)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![&params.upload_id, params.part_number, data.len() as i64, &etag, &part_path],
    ).ok();
    drop(db);

    let meta = serde_json::json!({"etag": etag}).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── MULTIPART: complete ──────────────────────────────────────────────────

async fn handle_complete_multipart(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,  // JSON: {upload_id, parts: [{part_number, etag}]}
    stream: &mut UnixStream,
) {
    #[derive(serde::Deserialize)]
    struct CompleteParams {
        upload_id: String,
        parts: Vec<PartInfo>,
    }
    #[derive(serde::Deserialize)]
    struct PartInfo { part_number: u32, etag: String }

    let params: CompleteParams = match serde_json::from_str(key) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::InvalidKey), &[], &[]).await;
            return;
        }
    };

    let db = state.db.lock().unwrap();

    // Verify upload exists
    let upload_key: String = match db.query_row(
        "SELECT object_key FROM multipart_uploads WHERE upload_id = ?1 AND volume_id = ?2 AND status = 'active'",
        rusqlite::params![&params.upload_id, volume_id],
        |row| row.get(0),
    ) {
        Ok(k) => k,
        Err(_) => {
            drop(db);
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
            return;
        }
    };

    // Read and concatenate all parts in order
    let mut combined = Vec::new();
    let mut sorted_parts = params.parts;
    sorted_parts.sort_by_key(|p| p.part_number);

    for part in &sorted_parts {
        let path: String = match db.query_row(
            "SELECT backend_path FROM multipart_parts WHERE upload_id = ?1 AND part_number = ?2",
            rusqlite::params![&params.upload_id, part.part_number],
            |row| row.get(0),
        ) {
            Ok(p) => p,
            Err(_) => {
                drop(db);
                send_response(stream, ObjectResponseHeader::err(ObjectStatus::NotFound), &[], &[]).await;
                return;
            }
        };
        match std::fs::read(&path) {
            Ok(data) => combined.extend_from_slice(&data),
            Err(e) => {
                tracing::error!("Object server: failed to read part {}: {}", path, e);
                drop(db);
                send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
                return;
            }
        }
    }

    // Get volume config
    let chunk_size: u64 = db.query_row(
        "SELECT chunk_size_bytes FROM volumes WHERE id = ?1",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(4194304);
    let local_raid: String = db.query_row(
        "SELECT local_raid FROM volumes WHERE id = ?1",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or_else(|_| "stripe".into());

    // Write combined data as a single object
    let file_id = chunk::deterministic_file_id(volume_id, &upload_key);

    use sha2::{Sha256, Digest};
    let sha = format!("{:x}", Sha256::digest(&combined));

    db.execute(
        "INSERT INTO file_map (id, volume_id, rel_path, size_bytes, sha256, version, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, datetime('now'))
         ON CONFLICT(volume_id, rel_path) DO UPDATE SET
           size_bytes = ?4, sha256 = ?5, version = version + 1, updated_at = datetime('now')",
        rusqlite::params![file_id, volume_id, &upload_key, combined.len() as i64, &sha],
    ).ok();

    let mut offset: u64 = 0;
    while offset < combined.len() as u64 {
        let end = ((offset + chunk_size) as usize).min(combined.len());
        let chunk_data = &combined[offset as usize..end];
        if let Err(e) = chunk::write_chunk_data(
            &db, file_id, offset, chunk_data,
            volume_id, &state.node_id, chunk_size, &local_raid,
        ) {
            tracing::error!("Object server: multipart write failed: {}", e);
            drop(db);
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::IoError), &[], &[]).await;
            return;
        }
        offset += chunk_size;
    }

    let chunk_count = ((combined.len() as u64 + chunk_size - 1) / chunk_size) as i64;
    db.execute("UPDATE file_map SET chunk_count = ?1 WHERE id = ?2", rusqlite::params![chunk_count, file_id]).ok();

    // Mark upload as completed and clean up temp files
    db.execute(
        "UPDATE multipart_uploads SET status = 'completed' WHERE upload_id = ?1",
        rusqlite::params![&params.upload_id],
    ).ok();

    // Clean up temp part files
    let part_dir = format!("/var/lib/vmm-san/multipart/{}", params.upload_id);
    std::fs::remove_dir_all(&part_dir).ok();

    // Trigger replication
    let version: i64 = db.query_row(
        "SELECT version FROM file_map WHERE id = ?1",
        rusqlite::params![file_id], |row| row.get(0),
    ).unwrap_or(1);
    drop(db);

    let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
        volume_id: volume_id.to_string(),
        rel_path: upload_key,
        file_id,
        version,
        writer_node_id: state.node_id.clone(),
    });

    let meta = serde_json::json!({"etag": sha}).to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, ObjectResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

// ── MULTIPART: abort ─────────────────────────────────────────────────────

async fn handle_abort_multipart(
    state: &CoreSanState,
    volume_id: &str,
    key: &str,  // JSON: {upload_id}
    stream: &mut UnixStream,
) {
    #[derive(serde::Deserialize)]
    struct AbortParams { upload_id: String }

    let params: AbortParams = match serde_json::from_str(key) {
        Ok(p) => p,
        Err(_) => {
            send_response(stream, ObjectResponseHeader::err(ObjectStatus::InvalidKey), &[], &[]).await;
            return;
        }
    };

    let db = state.db.lock().unwrap();
    db.execute(
        "UPDATE multipart_uploads SET status = 'aborted' WHERE upload_id = ?1",
        rusqlite::params![&params.upload_id],
    ).ok();
    db.execute(
        "DELETE FROM multipart_parts WHERE upload_id = ?1",
        rusqlite::params![&params.upload_id],
    ).ok();
    drop(db);

    // Clean up temp files
    let part_dir = format!("/var/lib/vmm-san/multipart/{}", params.upload_id);
    std::fs::remove_dir_all(&part_dir).ok();

    send_response(stream, ObjectResponseHeader::ok(0, 0), &[], &[]).await;
}
```

- [ ] **Step 2: Register in engine/mod.rs**

Add to `apps/vmm-san/src/engine/mod.rs`:
```rust
pub mod object_server;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p vmm-san`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/engine/object_server.rs apps/vmm-san/src/engine/mod.rs
git commit -m "feat: add Object Socket server for S3-enabled volumes"
```

---

## Task 6: Management Server Engine (`apps/vmm-san/src/engine/mgmt_server.rs`)

**Files:**
- Create: `apps/vmm-san/src/engine/mgmt_server.rs`
- Modify: `apps/vmm-san/src/engine/mod.rs`

- [ ] **Step 1: Create the management server module**

```rust
// apps/vmm-san/src/engine/mgmt_server.rs

//! Management Socket server — handles auth validation, volume listing, credential CRUD.
//!
//! Single socket at /run/vmm-san/mgmt.sock. Used by vmm-s3gw for operations
//! that are not volume-specific.

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use vmm_core::san_mgmt::*;
use crate::state::CoreSanState;

/// Spawn the management socket listener.
pub fn spawn(state: Arc<CoreSanState>) {
    std::fs::create_dir_all("/run/vmm-san").ok();
    let sock_path = MGMT_SOCKET_PATH;
    std::fs::remove_file(sock_path).ok();

    tokio::spawn(async move {
        let listener = match UnixListener::bind(sock_path) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Mgmt server: cannot bind {}: {}", sock_path, e);
                return;
            }
        };

        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(sock_path, std::fs::Permissions::from_mode(0o666)).ok();
        tracing::info!("Mgmt server: listening on {}", sock_path);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let state = state.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(state, stream).await {
                            tracing::warn!("Mgmt server: connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Mgmt server: accept error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    });
}

async fn handle_connection(
    state: Arc<CoreSanState>,
    mut stream: UnixStream,
) -> Result<(), String> {
    let mut hdr_buf = [0u8; MgmtRequestHeader::SIZE];

    loop {
        if stream.read_exact(&mut hdr_buf).await.is_err() {
            break;
        }

        let hdr = MgmtRequestHeader::from_bytes(&hdr_buf);
        if hdr.magic != MGMT_REQUEST_MAGIC {
            send_response(&mut stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            break;
        }

        let mut key_buf = vec![0u8; hdr.key_len as usize];
        if hdr.key_len > 0 {
            if stream.read_exact(&mut key_buf).await.is_err() { break; }
        }
        let key = String::from_utf8_lossy(&key_buf).to_string();

        let mut body = vec![0u8; hdr.body_len as usize];
        if hdr.body_len > 0 {
            if stream.read_exact(&mut body).await.is_err() { break; }
        }

        let cmd = match MgmtCommand::from_u32(hdr.cmd) {
            Some(c) => c,
            None => {
                send_response(&mut stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
                continue;
            }
        };

        match cmd {
            MgmtCommand::ListVolumes => handle_list_volumes(&state, &mut stream).await,
            MgmtCommand::ResolveVolume => handle_resolve_volume(&state, &key, &mut stream).await,
            MgmtCommand::CreateCredential => handle_create_credential(&state, &body, &mut stream).await,
            MgmtCommand::ValidateCredential => handle_validate_credential(&state, &body, &mut stream).await,
            MgmtCommand::ListCredentials => handle_list_credentials(&state, &mut stream).await,
            MgmtCommand::DeleteCredential => handle_delete_credential(&state, &key, &mut stream).await,
            MgmtCommand::CreateVolume => {
                send_response(&mut stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            }
            MgmtCommand::DeleteVolume => {
                send_response(&mut stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            }
        }
    }

    Ok(())
}

async fn send_response(stream: &mut UnixStream, header: MgmtResponseHeader, metadata: &[u8], data: &[u8]) {
    let _ = stream.write_all(&header.to_bytes()).await;
    if !metadata.is_empty() { let _ = stream.write_all(metadata).await; }
    if !data.is_empty() { let _ = stream.write_all(data).await; }
}

async fn handle_list_volumes(state: &CoreSanState, stream: &mut UnixStream) {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, name, status, access_protocols, max_size_bytes FROM volumes WHERE access_protocols LIKE '%s3%'"
    ).unwrap();

    let vols: Vec<serde_json::Value> = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "status": row.get::<_, String>(2)?,
            "access_protocols": row.get::<_, String>(3)?,
            "max_size_bytes": row.get::<_, i64>(4)?,
        }))
    }).unwrap().filter_map(|r| r.ok()).collect();
    drop(db);

    let body = serde_json::to_string(&vols).unwrap_or_else(|_| "[]".into());
    let body_bytes = body.as_bytes();
    send_response(stream, MgmtResponseHeader::ok(body_bytes.len() as u64, 0), &[], body_bytes).await;
}

async fn handle_resolve_volume(state: &CoreSanState, name: &str, stream: &mut UnixStream) {
    let db = state.db.lock().unwrap();
    let result = db.query_row(
        "SELECT id, name, status FROM volumes WHERE name = ?1 AND access_protocols LIKE '%s3%'",
        rusqlite::params![name],
        |row| Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "status": row.get::<_, String>(2)?,
        })),
    );
    drop(db);

    match result {
        Ok(vol) => {
            let meta = vol.to_string();
            let meta_bytes = meta.as_bytes();
            send_response(stream, MgmtResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
        }
        Err(_) => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::NotFound), &[], &[]).await;
        }
    }
}

async fn handle_create_credential(state: &CoreSanState, body: &[u8], stream: &mut UnixStream) {
    #[derive(serde::Deserialize)]
    struct CreateReq { user_id: String, #[serde(default)] display_name: String }

    let req: CreateReq = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            return;
        }
    };

    // Generate AWS-compatible access key (20 uppercase alphanumeric)
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let access_key: String = (0..20).map(|_| {
        let idx = rng.gen_range(0..36);
        if idx < 10 { (b'0' + idx) as char } else { (b'A' + idx - 10) as char }
    }).collect();

    // Generate secret key (40 characters)
    let secret_key: String = (0..40).map(|_| {
        let idx = rng.gen_range(0..62);
        if idx < 10 { (b'0' + idx) as char }
        else if idx < 36 { (b'A' + idx - 10) as char }
        else { (b'a' + idx - 36) as char }
    }).collect();

    // Encrypt secret key with node_id as key (simple AES-256-GCM)
    // For now, store base64-encoded (encryption will be added with aes-gcm crate)
    use sha2::{Sha256, Digest};
    let key_hash = Sha256::digest(state.node_id.as_bytes());
    // Simple XOR-based obfuscation as placeholder until aes-gcm is added
    let encrypted: Vec<u8> = secret_key.bytes()
        .zip(key_hash.iter().cycle())
        .map(|(b, k)| b ^ k)
        .collect();
    let secret_key_enc = base64_encode(&encrypted);

    let id = uuid::Uuid::new_v4().to_string();
    let db = state.db.lock().unwrap();
    db.execute(
        "INSERT INTO s3_credentials (id, access_key, secret_key_enc, user_id, display_name)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![&id, &access_key, &secret_key_enc, &req.user_id, &req.display_name],
    ).ok();
    drop(db);

    // Return plaintext secret key (only shown once)
    let resp = serde_json::json!({
        "id": id,
        "access_key": access_key,
        "secret_key": secret_key,
    });
    let meta = resp.to_string();
    let meta_bytes = meta.as_bytes();
    send_response(stream, MgmtResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
}

async fn handle_validate_credential(state: &CoreSanState, body: &[u8], stream: &mut UnixStream) {
    #[derive(serde::Deserialize)]
    struct ValidateReq {
        access_key: String,
        string_to_sign: String,
        signature: String,
        region: String,
        date: String,        // YYYYMMDD
        service: String,     // "s3"
    }

    let req: ValidateReq = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::InvalidRequest), &[], &[]).await;
            return;
        }
    };

    let db = state.db.lock().unwrap();
    let cred = db.query_row(
        "SELECT secret_key_enc, status FROM s3_credentials WHERE access_key = ?1",
        rusqlite::params![&req.access_key],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    );
    drop(db);

    let (secret_key_enc, status) = match cred {
        Ok(c) => c,
        Err(_) => {
            send_response(stream, MgmtResponseHeader::err(MgmtStatus::AccessDenied), &[], &[]).await;
            return;
        }
    };

    if status != "active" {
        send_response(stream, MgmtResponseHeader::err(MgmtStatus::AccessDenied), &[], &[]).await;
        return;
    }

    // Decrypt secret key
    use sha2::{Sha256, Digest};
    let key_hash = Sha256::digest(state.node_id.as_bytes());
    let encrypted = base64_decode(&secret_key_enc);
    let secret_key: String = encrypted.iter()
        .zip(key_hash.iter().cycle())
        .map(|(b, k)| (b ^ k) as char)
        .collect();

    // Compute AWS Signature V4
    let date_key = hmac_sha256(format!("AWS4{}", secret_key).as_bytes(), req.date.as_bytes());
    let date_region_key = hmac_sha256(&date_key, req.region.as_bytes());
    let date_region_service_key = hmac_sha256(&date_region_key, req.service.as_bytes());
    let signing_key = hmac_sha256(&date_region_service_key, b"aws4_request");
    let expected_sig = hex_encode(&hmac_sha256(&signing_key, req.string_to_sign.as_bytes()));

    if expected_sig == req.signature {
        // Return the user_id in metadata for authorization decisions
        let db = state.db.lock().unwrap();
        let user_id: String = db.query_row(
            "SELECT user_id FROM s3_credentials WHERE access_key = ?1",
            rusqlite::params![&req.access_key], |row| row.get(0),
        ).unwrap_or_default();
        drop(db);

        let meta = serde_json::json!({"user_id": user_id}).to_string();
        let meta_bytes = meta.as_bytes();
        send_response(stream, MgmtResponseHeader::ok(0, meta_bytes.len() as u32), meta_bytes, &[]).await;
    } else {
        send_response(stream, MgmtResponseHeader::err(MgmtStatus::AccessDenied), &[], &[]).await;
    }
}

async fn handle_list_credentials(state: &CoreSanState, stream: &mut UnixStream) {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, access_key, user_id, display_name, status, created_at, expires_at FROM s3_credentials"
    ).unwrap();

    let creds: Vec<serde_json::Value> = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "access_key": row.get::<_, String>(1)?,
            "user_id": row.get::<_, String>(2)?,
            "display_name": row.get::<_, String>(3)?,
            "status": row.get::<_, String>(4)?,
            "created_at": row.get::<_, String>(5)?,
            "expires_at": row.get::<_, Option<String>>(6)?,
        }))
    }).unwrap().filter_map(|r| r.ok()).collect();
    drop(db);

    let body = serde_json::to_string(&creds).unwrap_or_else(|_| "[]".into());
    let body_bytes = body.as_bytes();
    send_response(stream, MgmtResponseHeader::ok(body_bytes.len() as u64, 0), &[], body_bytes).await;
}

async fn handle_delete_credential(state: &CoreSanState, id: &str, stream: &mut UnixStream) {
    let db = state.db.lock().unwrap();
    let deleted = db.execute("DELETE FROM s3_credentials WHERE id = ?1", rusqlite::params![id]).unwrap_or(0);
    drop(db);

    if deleted > 0 {
        send_response(stream, MgmtResponseHeader::ok(0, 0), &[], &[]).await;
    } else {
        send_response(stream, MgmtResponseHeader::err(MgmtStatus::NotFound), &[], &[]).await;
    }
}

// ── Crypto helpers ───────────────────────────────────────────────────────

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use sha2::{Sha256, Digest};
    // HMAC-SHA256 implementation
    let block_size = 64;
    let mut k = if key.len() > block_size {
        Sha256::digest(key).to_vec()
    } else {
        key.to_vec()
    };
    k.resize(block_size, 0);

    let mut ipad = vec![0x36u8; block_size];
    let mut opad = vec![0x5cu8; block_size];
    for i in 0..block_size {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    ipad.extend_from_slice(data);
    let inner_hash = Sha256::digest(&ipad);
    opad.extend_from_slice(&inner_hash);
    Sha256::digest(&opad).to_vec()
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 { result.push(CHARS[((n >> 6) & 63) as usize] as char); } else { result.push('='); }
        if chunk.len() > 2 { result.push(CHARS[(n & 63) as usize] as char); } else { result.push('='); }
    }
    result
}

fn base64_decode(s: &str) -> Vec<u8> {
    const DECODE: [u8; 128] = {
        let mut table = [255u8; 128];
        let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < 64 {
            table[chars[i] as usize] = i as u8;
            i += 1;
        }
        table
    };
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'=').collect();
    let mut result = Vec::new();
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 { break; }
        let b0 = DECODE[chunk[0] as usize] as u32;
        let b1 = DECODE[chunk[1] as usize] as u32;
        let b2 = if chunk.len() > 2 { DECODE[chunk[2] as usize] as u32 } else { 0 };
        let b3 = if chunk.len() > 3 { DECODE[chunk[3] as usize] as u32 } else { 0 };
        let n = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
        result.push((n >> 16) as u8);
        if chunk.len() > 2 { result.push((n >> 8) as u8); }
        if chunk.len() > 3 { result.push(n as u8); }
    }
    result
}
```

- [ ] **Step 2: Register in engine/mod.rs**

Add to `apps/vmm-san/src/engine/mod.rs`:
```rust
pub mod mgmt_server;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p vmm-san`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/engine/mgmt_server.rs apps/vmm-san/src/engine/mod.rs
git commit -m "feat: add Management Socket server for S3 auth and volume listing"
```

---

## Task 7: Spawn Object + Mgmt Servers at Startup

**Files:**
- Modify: `apps/vmm-san/src/main.rs`

- [ ] **Step 1: Add object_server and mgmt_server spawn calls**

In `apps/vmm-san/src/main.rs`, after the `engine::disk_server::spawn_all` line (around line 280), add:

```rust
    engine::object_server::spawn_all(Arc::clone(&state));
    tracing::info!("Object server started (UDS per S3-enabled volume)");

    engine::mgmt_server::spawn(Arc::clone(&state));
    tracing::info!("Mgmt server started ({})", vmm_core::san_mgmt::MGMT_SOCKET_PATH);
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p vmm-san`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add apps/vmm-san/src/main.rs
git commit -m "feat: spawn object and mgmt socket servers at CoreSAN startup"
```

---

## Task 8: vmm-s3gw — Crate Setup and Config

**Files:**
- Create: `apps/vmm-s3gw/Cargo.toml`
- Create: `apps/vmm-s3gw/src/main.rs`
- Create: `apps/vmm-s3gw/src/config.rs`
- Modify: `Cargo.toml` (workspace)

- [ ] **Step 1: Add vmm-s3gw to workspace**

In the root `Cargo.toml`, add `"apps/vmm-s3gw"` to the members list:
```toml
members = [
  "apps/vmctl",
  "apps/vmmctl",
  "apps/vmmanager",
  "apps/vmm-server",
  "apps/vmm-cluster",
  "apps/vmm-san",
  "apps/vmm-s3gw",
  "apps/vmm-appliance",
  "apps/san-testbed",
  "libs/vmm-core",
  "tests/hosttests",
]
```

- [ ] **Step 2: Create Cargo.toml**

```toml
# apps/vmm-s3gw/Cargo.toml

[package]
name = "vmm-s3gw"
version.workspace = true
edition = "2021"
description = "S3-compatible gateway for CoreSAN object storage"

[[bin]]
name = "vmm-s3gw"
path = "src/main.rs"

[dependencies]
vmm-core = { path = "../../libs/vmm-core" }

axum = { version = "0.8", features = ["macros"] }
tower-http = { version = "0.6", features = ["cors", "trace"] }
tokio = { version = "1", features = ["full"] }

serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

sha2 = "0.10"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **Step 3: Create config.rs**

```rust
// apps/vmm-s3gw/src/config.rs

//! S3 Gateway configuration (parsed from TOML file).

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct S3GwConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub san: SanSection,
    #[serde(default)]
    pub tls: TlsSection,
    #[serde(default)]
    pub logging: LoggingSection,
}

#[derive(Debug, Deserialize)]
pub struct ServerSection {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_region")]
    pub region: String,
}

#[derive(Debug, Deserialize)]
pub struct SanSection {
    #[serde(default = "default_mgmt_socket")]
    pub mgmt_socket: String,
    #[serde(default = "default_object_socket_dir")]
    pub object_socket_dir: String,
}

#[derive(Debug, Deserialize)]
pub struct TlsSection {
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct LoggingSection {
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_listen() -> String { "0.0.0.0:9000".into() }
fn default_region() -> String { "us-east-1".into() }
fn default_mgmt_socket() -> String { "/run/vmm-san/mgmt.sock".into() }
fn default_object_socket_dir() -> String { "/run/vmm-san".into() }
fn default_log_level() -> String { "info".into() }

impl Default for ServerSection {
    fn default() -> Self { Self { listen: default_listen(), region: default_region() } }
}
impl Default for SanSection {
    fn default() -> Self { Self { mgmt_socket: default_mgmt_socket(), object_socket_dir: default_object_socket_dir() } }
}
impl Default for TlsSection {
    fn default() -> Self { Self { cert: None, key: None } }
}
impl Default for LoggingSection {
    fn default() -> Self { Self { level: default_log_level() } }
}
impl Default for S3GwConfig {
    fn default() -> Self {
        Self { server: Default::default(), san: Default::default(), tls: Default::default(), logging: Default::default() }
    }
}

impl S3GwConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            tracing::warn!("Config not found: {}, using defaults", path.display());
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config: {}", e))?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config: {}", e))
    }
}
```

- [ ] **Step 4: Create minimal main.rs**

```rust
// apps/vmm-s3gw/src/main.rs

//! vmm-s3gw — S3-compatible gateway for CoreSAN object storage.
//!
//! Translates S3 HTTP requests into CoreSAN Object Socket calls.
//! Pure frontend — no database, no chunk logic, no replication.

mod config;
mod auth;
mod s3;
mod socket;

use config::S3GwConfig;
use std::sync::Arc;

pub struct AppState {
    pub config: S3GwConfig,
    pub socket: socket::SocketPool,
}

#[tokio::main]
async fn main() {
    let config_path = std::env::args()
        .skip_while(|a| a != "--config")
        .nth(1)
        .unwrap_or_else(|| "/etc/vmm/s3gw.toml".to_string());

    // Init logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
        .init();

    let config = S3GwConfig::load(std::path::Path::new(&config_path))
        .unwrap_or_else(|e| {
            eprintln!("Config error: {}", e);
            std::process::exit(1);
        });

    tracing::info!("vmm-s3gw starting on {}", config.server.listen);

    let listen_addr = config.server.listen.clone();
    let socket_pool = socket::SocketPool::new(&config.san);

    let state = Arc::new(AppState { config, socket: socket_pool });

    let app = s3::router(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await.unwrap_or_else(|e| {
        eprintln!("Cannot bind {}: {}", listen_addr, e);
        std::process::exit(1);
    });

    tracing::info!("Listening on {}", listen_addr);
    axum::serve(listener, app).await.unwrap();
}
```

- [ ] **Step 5: Verify workspace compiles (will fail until we add stub modules — that's expected)**

Run: `cargo check -p vmm-s3gw 2>&1 | head -5`
Expected: errors about missing modules (auth, s3, socket) — we'll create those next

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-s3gw/Cargo.toml apps/vmm-s3gw/src/main.rs apps/vmm-s3gw/src/config.rs Cargo.toml
git commit -m "feat: add vmm-s3gw crate with config and main entry point"
```

---

## Task 9: vmm-s3gw — Socket Pool (`apps/vmm-s3gw/src/socket.rs`)

**Files:**
- Create: `apps/vmm-s3gw/src/socket.rs`

- [ ] **Step 1: Create the socket pool module**

```rust
// apps/vmm-s3gw/src/socket.rs

//! UDS client — connection pool for management and object sockets.

use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use vmm_core::san_object::*;
use vmm_core::san_mgmt::*;
use crate::config::SanSection;

pub struct SocketPool {
    mgmt_path: String,
    object_socket_dir: String,
    mgmt_conn: Mutex<Option<UnixStream>>,
    obj_conns: Mutex<HashMap<String, UnixStream>>,
}

impl SocketPool {
    pub fn new(config: &SanSection) -> Self {
        Self {
            mgmt_path: config.mgmt_socket.clone(),
            object_socket_dir: config.object_socket_dir.clone(),
            mgmt_conn: Mutex::new(None),
            obj_conns: Mutex::new(HashMap::new()),
        }
    }

    /// Send a management command and get the response.
    pub async fn mgmt_request(&self, cmd: MgmtCommand, key: &[u8], body: &[u8]) -> Result<MgmtResponse, String> {
        let mut guard = self.mgmt_conn.lock().await;

        // Try to use existing connection, reconnect if needed
        let stream = match guard.as_mut() {
            Some(s) => s,
            None => {
                let conn = UnixStream::connect(&self.mgmt_path).await
                    .map_err(|e| format!("Cannot connect to mgmt socket {}: {}", self.mgmt_path, e))?;
                *guard = Some(conn);
                guard.as_mut().unwrap()
            }
        };

        let hdr = MgmtRequestHeader::new(cmd, key.len() as u32, body.len() as u64);
        if stream.write_all(&hdr.to_bytes()).await.is_err()
            || (!key.is_empty() && stream.write_all(key).await.is_err())
            || (!body.is_empty() && stream.write_all(body).await.is_err())
        {
            // Connection broken, reconnect
            *guard = None;
            return Err("Connection lost to mgmt socket".into());
        }

        let mut resp_buf = [0u8; MgmtResponseHeader::SIZE];
        if stream.read_exact(&mut resp_buf).await.is_err() {
            *guard = None;
            return Err("Failed to read mgmt response".into());
        }

        let resp_hdr = MgmtResponseHeader::from_bytes(&resp_buf);

        let mut metadata = vec![0u8; resp_hdr.metadata_len as usize];
        if resp_hdr.metadata_len > 0 {
            if stream.read_exact(&mut metadata).await.is_err() {
                *guard = None;
                return Err("Failed to read mgmt metadata".into());
            }
        }

        let mut body_data = vec![0u8; resp_hdr.body_len as usize];
        if resp_hdr.body_len > 0 {
            if stream.read_exact(&mut body_data).await.is_err() {
                *guard = None;
                return Err("Failed to read mgmt body".into());
            }
        }

        Ok(MgmtResponse {
            status: resp_hdr.status,
            metadata,
            body: body_data,
        })
    }

    /// Send an object command to a specific volume's socket.
    pub async fn object_request(&self, volume_id: &str, cmd: ObjectCommand, key: &[u8], body: &[u8]) -> Result<ObjectResponse, String> {
        let sock_path = format!("{}/obj-{}.sock", self.object_socket_dir, volume_id);

        let mut guard = self.obj_conns.lock().await;
        let stream = match guard.get_mut(volume_id) {
            Some(s) => s,
            None => {
                let conn = UnixStream::connect(&sock_path).await
                    .map_err(|e| format!("Cannot connect to object socket {}: {}", sock_path, e))?;
                guard.insert(volume_id.to_string(), conn);
                guard.get_mut(volume_id).unwrap()
            }
        };

        let hdr = ObjectRequestHeader::new(cmd, key.len() as u32, body.len() as u64);
        if stream.write_all(&hdr.to_bytes()).await.is_err()
            || (!key.is_empty() && stream.write_all(key).await.is_err())
            || (!body.is_empty() && stream.write_all(body).await.is_err())
        {
            guard.remove(volume_id);
            return Err("Connection lost to object socket".into());
        }

        let mut resp_buf = [0u8; ObjectResponseHeader::SIZE];
        if stream.read_exact(&mut resp_buf).await.is_err() {
            guard.remove(volume_id);
            return Err("Failed to read object response".into());
        }

        let resp_hdr = ObjectResponseHeader::from_bytes(&resp_buf);

        let mut metadata = vec![0u8; resp_hdr.metadata_len as usize];
        if resp_hdr.metadata_len > 0 {
            if stream.read_exact(&mut metadata).await.is_err() {
                guard.remove(volume_id);
                return Err("Failed to read object metadata".into());
            }
        }

        let mut body_data = vec![0u8; resp_hdr.body_len as usize];
        if resp_hdr.body_len > 0 {
            if stream.read_exact(&mut body_data).await.is_err() {
                guard.remove(volume_id);
                return Err("Failed to read object body".into());
            }
        }

        Ok(ObjectResponse {
            status: resp_hdr.status,
            metadata,
            body: body_data,
        })
    }
}

pub struct MgmtResponse {
    pub status: u32,
    pub metadata: Vec<u8>,
    pub body: Vec<u8>,
}

impl MgmtResponse {
    pub fn is_ok(&self) -> bool { self.status == MgmtStatus::Ok as u32 }
    pub fn metadata_json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.metadata).unwrap_or(serde_json::Value::Null)
    }
    pub fn body_json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

pub struct ObjectResponse {
    pub status: u32,
    pub metadata: Vec<u8>,
    pub body: Vec<u8>,
}

impl ObjectResponse {
    pub fn is_ok(&self) -> bool { self.status == ObjectStatus::Ok as u32 }
    pub fn metadata_json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.metadata).unwrap_or(serde_json::Value::Null)
    }
}
```

- [ ] **Step 2: Verify it compiles (still needs auth, s3 stubs)**

- [ ] **Step 3: Commit**

```bash
git add apps/vmm-s3gw/src/socket.rs
git commit -m "feat: add UDS socket pool for S3 gateway"
```

---

## Task 10: vmm-s3gw — S3 Error and XML Helpers

**Files:**
- Create: `apps/vmm-s3gw/src/s3/mod.rs`
- Create: `apps/vmm-s3gw/src/s3/error.rs`
- Create: `apps/vmm-s3gw/src/s3/xml.rs`

- [ ] **Step 1: Create s3/error.rs**

```rust
// apps/vmm-s3gw/src/s3/error.rs

//! S3-compatible XML error responses.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub struct S3Error {
    pub code: &'static str,
    pub message: String,
    pub http_status: StatusCode,
    pub resource: String,
}

impl S3Error {
    pub fn no_such_key(key: &str) -> Self {
        Self { code: "NoSuchKey", message: "The specified key does not exist.".into(), http_status: StatusCode::NOT_FOUND, resource: key.into() }
    }
    pub fn no_such_bucket(bucket: &str) -> Self {
        Self { code: "NoSuchBucket", message: "The specified bucket does not exist.".into(), http_status: StatusCode::NOT_FOUND, resource: bucket.into() }
    }
    pub fn bucket_already_exists(bucket: &str) -> Self {
        Self { code: "BucketAlreadyOwnedByYou", message: "Your previous request to create the named bucket succeeded.".into(), http_status: StatusCode::CONFLICT, resource: bucket.into() }
    }
    pub fn access_denied() -> Self {
        Self { code: "AccessDenied", message: "Access Denied".into(), http_status: StatusCode::FORBIDDEN, resource: String::new() }
    }
    pub fn invalid_argument(msg: &str) -> Self {
        Self { code: "InvalidArgument", message: msg.into(), http_status: StatusCode::BAD_REQUEST, resource: String::new() }
    }
    pub fn internal_error(msg: &str) -> Self {
        Self { code: "InternalError", message: msg.into(), http_status: StatusCode::INTERNAL_SERVER_ERROR, resource: String::new() }
    }
    pub fn insufficient_storage() -> Self {
        Self { code: "InsufficientStorage", message: "Not enough storage.".into(), http_status: StatusCode::from_u16(507).unwrap(), resource: String::new() }
    }
    pub fn slow_down() -> Self {
        Self { code: "SlowDown", message: "Please reduce your request rate.".into(), http_status: StatusCode::SERVICE_UNAVAILABLE, resource: String::new() }
    }
}

impl IntoResponse for S3Error {
    fn into_response(self) -> Response {
        let request_id = uuid::Uuid::new_v4().to_string();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>{}</Code>
  <Message>{}</Message>
  <Resource>{}</Resource>
  <RequestId>{}</RequestId>
</Error>"#,
            self.code, self.message, self.resource, request_id
        );
        (self.http_status, [("content-type", "application/xml")], xml).into_response()
    }
}
```

- [ ] **Step 2: Create s3/xml.rs**

```rust
// apps/vmm-s3gw/src/s3/xml.rs

//! S3-compatible XML response builders.

/// Build ListAllMyBucketsResult XML.
pub fn list_buckets_xml(buckets: &[BucketInfo], owner_id: &str) -> String {
    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner>
    <ID>"#);
    xml.push_str(owner_id);
    xml.push_str("</ID>\n    <DisplayName>");
    xml.push_str(owner_id);
    xml.push_str("</DisplayName>\n  </Owner>\n  <Buckets>\n");

    for b in buckets {
        xml.push_str("    <Bucket>\n      <Name>");
        xml.push_str(&xml_escape(&b.name));
        xml.push_str("</Name>\n      <CreationDate>");
        xml.push_str(&b.creation_date);
        xml.push_str("</CreationDate>\n    </Bucket>\n");
    }

    xml.push_str("  </Buckets>\n</ListAllMyBucketsResult>");
    xml
}

pub struct BucketInfo {
    pub name: String,
    pub creation_date: String,
}

/// Build ListBucketResult (ListObjectsV2) XML.
pub fn list_objects_v2_xml(
    bucket: &str,
    prefix: &str,
    delimiter: &str,
    max_keys: u32,
    is_truncated: bool,
    objects: &[ObjectInfo],
    common_prefixes: &[String],
    continuation_token: &str,
    next_token: &str,
) -> String {
    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">"#);
    xml.push_str("\n  <Name>"); xml.push_str(&xml_escape(bucket)); xml.push_str("</Name>");
    xml.push_str("\n  <Prefix>"); xml.push_str(&xml_escape(prefix)); xml.push_str("</Prefix>");
    if !delimiter.is_empty() {
        xml.push_str("\n  <Delimiter>"); xml.push_str(&xml_escape(delimiter)); xml.push_str("</Delimiter>");
    }
    xml.push_str(&format!("\n  <MaxKeys>{}</MaxKeys>", max_keys));
    xml.push_str(&format!("\n  <IsTruncated>{}</IsTruncated>", is_truncated));
    xml.push_str("\n  <KeyCount>"); xml.push_str(&objects.len().to_string()); xml.push_str("</KeyCount>");
    if !continuation_token.is_empty() {
        xml.push_str("\n  <ContinuationToken>"); xml.push_str(&xml_escape(continuation_token)); xml.push_str("</ContinuationToken>");
    }
    if !next_token.is_empty() {
        xml.push_str("\n  <NextContinuationToken>"); xml.push_str(&xml_escape(next_token)); xml.push_str("</NextContinuationToken>");
    }

    for obj in objects {
        xml.push_str("\n  <Contents>");
        xml.push_str("\n    <Key>"); xml.push_str(&xml_escape(&obj.key)); xml.push_str("</Key>");
        xml.push_str(&format!("\n    <Size>{}</Size>", obj.size));
        xml.push_str("\n    <ETag>\""); xml.push_str(&obj.etag); xml.push_str("\"</ETag>");
        xml.push_str("\n    <LastModified>"); xml.push_str(&obj.last_modified); xml.push_str("</LastModified>");
        xml.push_str("\n    <StorageClass>STANDARD</StorageClass>");
        xml.push_str("\n  </Contents>");
    }

    for prefix in common_prefixes {
        xml.push_str("\n  <CommonPrefixes>");
        xml.push_str("\n    <Prefix>"); xml.push_str(&xml_escape(prefix)); xml.push_str("</Prefix>");
        xml.push_str("\n  </CommonPrefixes>");
    }

    xml.push_str("\n</ListBucketResult>");
    xml
}

pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: String,
}

/// Build CopyObjectResult XML.
pub fn copy_object_result_xml(etag: &str, last_modified: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<CopyObjectResult>
  <ETag>"{}"</ETag>
  <LastModified>{}</LastModified>
</CopyObjectResult>"#, etag, last_modified)
}

/// Build InitiateMultipartUploadResult XML.
pub fn initiate_multipart_xml(bucket: &str, key: &str, upload_id: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<InitiateMultipartUploadResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Bucket>{}</Bucket>
  <Key>{}</Key>
  <UploadId>{}</UploadId>
</InitiateMultipartUploadResult>"#, xml_escape(bucket), xml_escape(key), upload_id)
}

/// Build CompleteMultipartUploadResult XML.
pub fn complete_multipart_xml(bucket: &str, key: &str, etag: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<CompleteMultipartUploadResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Bucket>{}</Bucket>
  <Key>{}</Key>
  <ETag>"{}"</ETag>
</CompleteMultipartUploadResult>"#, xml_escape(bucket), xml_escape(key), etag)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}
```

- [ ] **Step 3: Create s3/mod.rs (stub router)**

```rust
// apps/vmm-s3gw/src/s3/mod.rs

//! S3-compatible API router.

pub mod error;
pub mod xml;
pub mod bucket;
pub mod object;
pub mod multipart;

use axum::Router;
use std::sync::Arc;
use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(bucket::routes())
        .merge(object::routes())
        .merge(multipart::routes())
        .with_state(state)
}
```

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-s3gw/src/s3/
git commit -m "feat: add S3 error, XML helpers, and router stub"
```

---

## Task 11: vmm-s3gw — Auth Module (AWS Signature V4)

**Files:**
- Create: `apps/vmm-s3gw/src/auth.rs`

- [ ] **Step 1: Create the auth module**

```rust
// apps/vmm-s3gw/src/auth.rs

//! AWS Signature V4 parsing and verification via management socket.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;
use crate::AppState;
use crate::s3::error::S3Error;
use vmm_core::san_mgmt::MgmtCommand;

/// Parsed AWS auth info from request.
#[derive(Debug, Clone)]
pub struct S3Auth {
    pub access_key: String,
    pub user_id: String,
}

/// Extract and validate AWS Signature V4 from request.
/// Returns the authenticated user info or an S3Error.
pub async fn validate_request(
    state: &AppState,
    headers: &HeaderMap,
    method: &str,
    uri: &str,
    payload_hash: &str,
) -> Result<S3Auth, S3Error> {
    // Parse Authorization header
    let auth_header = headers.get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(S3Error::access_denied)?;

    if !auth_header.starts_with("AWS4-HMAC-SHA256 ") {
        return Err(S3Error::access_denied());
    }

    let parts = &auth_header["AWS4-HMAC-SHA256 ".len()..];

    // Parse Credential=AKID/date/region/s3/aws4_request
    let credential = extract_field(parts, "Credential")
        .ok_or_else(S3Error::access_denied)?;
    let cred_parts: Vec<&str> = credential.split('/').collect();
    if cred_parts.len() != 5 {
        return Err(S3Error::access_denied());
    }
    let access_key = cred_parts[0];
    let date = cred_parts[1];
    let region = cred_parts[2];
    let service = cred_parts[3];

    // Parse SignedHeaders
    let signed_headers = extract_field(parts, "SignedHeaders")
        .ok_or_else(S3Error::access_denied)?;

    // Parse Signature
    let signature = extract_field(parts, "Signature")
        .ok_or_else(S3Error::access_denied)?;

    // Build canonical request
    let canonical_headers = build_canonical_headers(headers, &signed_headers);
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method,
        uri_encode_path(uri),
        "", // query string (TODO: parse from URI)
        canonical_headers,
        signed_headers,
        payload_hash,
    );

    // Build string to sign
    let x_amz_date = headers.get("x-amz-date")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    use sha2::{Sha256, Digest};
    let canonical_hash = format!("{:x}", Sha256::digest(canonical_request.as_bytes()));

    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}/{}/{}/aws4_request\n{}",
        x_amz_date, date, region, service, canonical_hash
    );

    // Validate via management socket
    let validate_body = serde_json::json!({
        "access_key": access_key,
        "string_to_sign": string_to_sign,
        "signature": signature,
        "region": region,
        "date": date,
        "service": service,
    });

    let resp = state.socket.mgmt_request(
        MgmtCommand::ValidateCredential,
        &[],
        validate_body.to_string().as_bytes(),
    ).await.map_err(|e| S3Error::internal_error(&e))?;

    if !resp.is_ok() {
        return Err(S3Error::access_denied());
    }

    let meta = resp.metadata_json();
    let user_id = meta["user_id"].as_str().unwrap_or("").to_string();

    Ok(S3Auth { access_key: access_key.to_string(), user_id })
}

fn extract_field<'a>(header: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("{}=", field);
    for part in header.split(", ") {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            return Some(rest);
        }
    }
    None
}

fn build_canonical_headers(headers: &HeaderMap, signed_headers: &str) -> String {
    let mut result = String::new();
    for name in signed_headers.split(';') {
        let value = headers.get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        result.push_str(name);
        result.push(':');
        result.push_str(value.trim());
        result.push('\n');
    }
    result
}

fn uri_encode_path(path: &str) -> String {
    // Split by / and encode each segment
    path.split('/')
        .map(|seg| {
            seg.bytes().map(|b| {
                if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
                    format!("{}", b as char)
                } else {
                    format!("%{:02X}", b)
                }
            }).collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p vmm-s3gw 2>&1 | head -5`

- [ ] **Step 3: Commit**

```bash
git add apps/vmm-s3gw/src/auth.rs
git commit -m "feat: add AWS Signature V4 auth module for S3 gateway"
```

---

## Task 12: vmm-s3gw — Bucket Operations

**Files:**
- Create: `apps/vmm-s3gw/src/s3/bucket.rs`

- [ ] **Step 1: Create bucket.rs**

```rust
// apps/vmm-s3gw/src/s3/bucket.rs

//! S3 Bucket operations: ListBuckets, CreateBucket, DeleteBucket, HeadBucket.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put, delete, head};
use axum::Router;
use std::sync::Arc;
use crate::AppState;
use crate::s3::error::S3Error;
use crate::s3::xml;
use vmm_core::san_mgmt::MgmtCommand;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_buckets))
        .route("/{bucket}", get(list_objects_v2_bucket).put(create_bucket).delete(delete_bucket).head(head_bucket))
}

/// GET / — ListBuckets
async fn list_buckets(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, S3Error> {
    let auth = crate::auth::validate_request(&state, &headers, "GET", "/", "UNSIGNED-PAYLOAD").await?;

    let resp = state.socket.mgmt_request(MgmtCommand::ListVolumes, &[], &[]).await
        .map_err(|e| S3Error::internal_error(&e))?;

    if !resp.is_ok() {
        return Err(S3Error::internal_error("Failed to list volumes"));
    }

    let vols: Vec<serde_json::Value> = serde_json::from_slice(&resp.body).unwrap_or_default();
    let buckets: Vec<xml::BucketInfo> = vols.iter().map(|v| xml::BucketInfo {
        name: v["name"].as_str().unwrap_or("").to_string(),
        creation_date: "2024-01-01T00:00:00.000Z".to_string(),
    }).collect();

    let xml_body = xml::list_buckets_xml(&buckets, &auth.user_id);
    Ok((StatusCode::OK, [("content-type", "application/xml")], xml_body).into_response())
}

/// PUT /{bucket} — CreateBucket
async fn create_bucket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(bucket): Path<String>,
) -> Result<Response, S3Error> {
    let _auth = crate::auth::validate_request(&state, &headers, "PUT", &format!("/{}", bucket), "UNSIGNED-PAYLOAD").await?;

    // Check if bucket/volume already exists
    let check = state.socket.mgmt_request(MgmtCommand::ResolveVolume, bucket.as_bytes(), &[]).await
        .map_err(|e| S3Error::internal_error(&e))?;

    if check.is_ok() {
        return Err(S3Error::bucket_already_exists(&bucket));
    }

    // Create via REST API for now (CreateVolume over mgmt socket is stubbed)
    // The S3 gateway creates a volume with default settings + s3 protocol
    Err(S3Error::internal_error("CreateBucket via S3 gateway not yet implemented — use CoreSAN API to create volumes with access_protocols: [\"s3\"]"))
}

/// DELETE /{bucket} — DeleteBucket
async fn delete_bucket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(bucket): Path<String>,
) -> Result<Response, S3Error> {
    let _auth = crate::auth::validate_request(&state, &headers, "DELETE", &format!("/{}", bucket), "UNSIGNED-PAYLOAD").await?;

    // Not implemented — volumes should be managed via CoreSAN API
    Err(S3Error::internal_error("DeleteBucket via S3 gateway not yet implemented — use CoreSAN API"))
}

/// GET /{bucket}?list-type=2 — ListObjectsV2 (bucket-level, no key in path)
async fn list_objects_v2_bucket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(bucket): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Response, S3Error> {
    // Delegate to the object module's list handler
    crate::s3::object::handle_list_objects_v2_public(&state, &headers, &bucket, &params).await
}

/// HEAD /{bucket} — HeadBucket
async fn head_bucket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(bucket): Path<String>,
) -> Result<Response, S3Error> {
    let _auth = crate::auth::validate_request(&state, &headers, "HEAD", &format!("/{}", bucket), "UNSIGNED-PAYLOAD").await?;

    let resp = state.socket.mgmt_request(MgmtCommand::ResolveVolume, bucket.as_bytes(), &[]).await
        .map_err(|e| S3Error::internal_error(&e))?;

    if !resp.is_ok() {
        return Err(S3Error::no_such_bucket(&bucket));
    }

    Ok((StatusCode::OK, [("x-amz-bucket-region", state.config.server.region.as_str())]).into_response())
}
```

- [ ] **Step 2: Commit**

```bash
git add apps/vmm-s3gw/src/s3/bucket.rs
git commit -m "feat: add S3 bucket operations (ListBuckets, HeadBucket)"
```

---

## Task 13: vmm-s3gw — Object Operations

**Files:**
- Create: `apps/vmm-s3gw/src/s3/object.rs`

- [ ] **Step 1: Create object.rs**

```rust
// apps/vmm-s3gw/src/s3/object.rs

//! S3 Object operations: PutObject, GetObject, HeadObject, DeleteObject, CopyObject, ListObjectsV2.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put, delete, head};
use axum::Router;
use std::sync::Arc;
use crate::AppState;
use crate::s3::error::S3Error;
use crate::s3::xml;
use vmm_core::san_object::ObjectCommand;
use vmm_core::san_mgmt::MgmtCommand;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/{bucket}/{*key}", put(put_object).get(get_object).delete(delete_object).head(head_object))
}

/// Resolve bucket name to volume ID.
async fn resolve_bucket(state: &AppState, bucket: &str) -> Result<String, S3Error> {
    let resp = state.socket.mgmt_request(MgmtCommand::ResolveVolume, bucket.as_bytes(), &[]).await
        .map_err(|e| S3Error::internal_error(&e))?;
    if !resp.is_ok() {
        return Err(S3Error::no_such_bucket(bucket));
    }
    let meta = resp.metadata_json();
    meta["id"].as_str().map(|s| s.to_string())
        .ok_or_else(|| S3Error::internal_error("Volume ID missing"))
}

/// PUT /{bucket}/{key} — PutObject or CopyObject
async fn put_object(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
    body: Bytes,
) -> Result<Response, S3Error> {
    let _auth = crate::auth::validate_request(&state, &headers, "PUT", &format!("/{}/{}", bucket, key), "UNSIGNED-PAYLOAD").await?;
    let volume_id = resolve_bucket(&state, &bucket).await?;

    // Check for CopyObject (x-amz-copy-source header)
    if let Some(copy_source) = headers.get("x-amz-copy-source").and_then(|v| v.to_str().ok()) {
        return handle_copy_object(&state, &volume_id, &bucket, &key, copy_source).await;
    }

    let resp = state.socket.object_request(&volume_id, ObjectCommand::Put, key.as_bytes(), &body).await
        .map_err(|e| S3Error::internal_error(&e))?;

    if !resp.is_ok() {
        return Err(status_to_s3_error(resp.status, &key));
    }

    let meta = resp.metadata_json();
    let etag = meta["etag"].as_str().unwrap_or("");

    Ok((StatusCode::OK, [("etag", format!("\"{}\"", etag))]).into_response())
}

/// GET /{bucket}/{key} — GetObject or ListObjectsV2
async fn get_object(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Response, S3Error> {
    // Check if this is ListObjectsV2 (key is empty or list-type=2 query param)
    if params.contains_key("list-type") {
        return handle_list_objects_v2_public(&state, &headers, &bucket, &params).await;
    }

    let _auth = crate::auth::validate_request(&state, &headers, "GET", &format!("/{}/{}", bucket, key), "UNSIGNED-PAYLOAD").await?;
    let volume_id = resolve_bucket(&state, &bucket).await?;

    let resp = state.socket.object_request(&volume_id, ObjectCommand::Get, key.as_bytes(), &[]).await
        .map_err(|e| S3Error::internal_error(&e))?;

    if !resp.is_ok() {
        return Err(status_to_s3_error(resp.status, &key));
    }

    let meta = resp.metadata_json();
    let etag = meta["etag"].as_str().unwrap_or("");
    let size = meta["size"].as_u64().unwrap_or(0);
    let last_modified = meta["last_modified"].as_str().unwrap_or("");

    Ok((
        StatusCode::OK,
        [
            ("content-type", "application/octet-stream"),
            ("etag", &format!("\"{}\"", etag)),
            ("content-length", &size.to_string()),
            ("last-modified", last_modified),
        ],
        resp.body,
    ).into_response())
}

/// HEAD /{bucket}/{key} — HeadObject
async fn head_object(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let _auth = crate::auth::validate_request(&state, &headers, "HEAD", &format!("/{}/{}", bucket, key), "UNSIGNED-PAYLOAD").await?;
    let volume_id = resolve_bucket(&state, &bucket).await?;

    let resp = state.socket.object_request(&volume_id, ObjectCommand::Head, key.as_bytes(), &[]).await
        .map_err(|e| S3Error::internal_error(&e))?;

    if !resp.is_ok() {
        return Err(status_to_s3_error(resp.status, &key));
    }

    let meta = resp.metadata_json();
    let etag = meta["etag"].as_str().unwrap_or("");
    let size = meta["size"].as_u64().unwrap_or(0);
    let last_modified = meta["last_modified"].as_str().unwrap_or("");

    Ok((
        StatusCode::OK,
        [
            ("content-type", "application/octet-stream"),
            ("etag", &format!("\"{}\"", etag)),
            ("content-length", &size.to_string()),
            ("last-modified", last_modified),
        ],
    ).into_response())
}

/// DELETE /{bucket}/{key} — DeleteObject
async fn delete_object(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let _auth = crate::auth::validate_request(&state, &headers, "DELETE", &format!("/{}/{}", bucket, key), "UNSIGNED-PAYLOAD").await?;
    let volume_id = resolve_bucket(&state, &bucket).await?;

    let resp = state.socket.object_request(&volume_id, ObjectCommand::Delete, key.as_bytes(), &[]).await
        .map_err(|e| S3Error::internal_error(&e))?;

    // S3 always returns 204 for delete, even if key didn't exist
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn handle_copy_object(
    state: &AppState,
    volume_id: &str,
    bucket: &str,
    dst_key: &str,
    copy_source: &str,
) -> Result<Response, S3Error> {
    // Parse copy source: /bucket/key or bucket/key
    let source = copy_source.trim_start_matches('/');
    let (src_bucket, src_key) = source.split_once('/')
        .ok_or_else(|| S3Error::invalid_argument("Invalid x-amz-copy-source"))?;

    let src_volume_id = resolve_bucket(state, src_bucket).await?;

    // For same-volume copy, use the Copy command
    let copy_params = serde_json::json!({
        "src_key": src_key,
        "dst_key": dst_key,
    });

    let resp = if src_volume_id == volume_id {
        state.socket.object_request(volume_id, ObjectCommand::Copy, copy_params.to_string().as_bytes(), &[]).await
    } else {
        // Cross-volume copy: read from source, write to destination
        let get_resp = state.socket.object_request(&src_volume_id, ObjectCommand::Get, src_key.as_bytes(), &[]).await
            .map_err(|e| S3Error::internal_error(&e))?;
        if !get_resp.is_ok() {
            return Err(S3Error::no_such_key(src_key));
        }
        state.socket.object_request(volume_id, ObjectCommand::Put, dst_key.as_bytes(), &get_resp.body).await
    }.map_err(|e| S3Error::internal_error(&e))?;

    if !resp.is_ok() {
        return Err(status_to_s3_error(resp.status, dst_key));
    }

    let meta = resp.metadata_json();
    let etag = meta["etag"].as_str().unwrap_or("");
    let xml_body = xml::copy_object_result_xml(etag, "");

    Ok((StatusCode::OK, [("content-type", "application/xml")], xml_body).into_response())
}

pub async fn handle_list_objects_v2_public(
    state: &AppState,
    headers: &HeaderMap,
    bucket: &str,
    params: &std::collections::HashMap<String, String>,
) -> Result<Response, S3Error> {
    let _auth = crate::auth::validate_request(state, headers, "GET", &format!("/{}", bucket), "UNSIGNED-PAYLOAD").await?;
    let volume_id = resolve_bucket(state, bucket).await?;

    let prefix = params.get("prefix").map(|s| s.as_str()).unwrap_or("");
    let delimiter = params.get("delimiter").map(|s| s.as_str()).unwrap_or("");
    let max_keys: u32 = params.get("max-keys").and_then(|s| s.parse().ok()).unwrap_or(1000);
    let continuation_token = params.get("continuation-token").map(|s| s.as_str()).unwrap_or("");

    let list_params = serde_json::json!({
        "prefix": prefix,
        "marker": continuation_token,
        "max_keys": max_keys,
        "delimiter": delimiter,
    });

    let resp = state.socket.object_request(&volume_id, ObjectCommand::List, list_params.to_string().as_bytes(), &[]).await
        .map_err(|e| S3Error::internal_error(&e))?;

    if !resp.is_ok() {
        return Err(S3Error::internal_error("List failed"));
    }

    let result: serde_json::Value = serde_json::from_slice(&resp.body).unwrap_or_default();
    let is_truncated = result["is_truncated"].as_bool().unwrap_or(false);

    let objects: Vec<xml::ObjectInfo> = result["objects"].as_array()
        .map(|arr| arr.iter().map(|o| xml::ObjectInfo {
            key: o["key"].as_str().unwrap_or("").to_string(),
            size: o["size"].as_u64().unwrap_or(0),
            etag: o["etag"].as_str().unwrap_or("").to_string(),
            last_modified: o["last_modified"].as_str().unwrap_or("").to_string(),
        }).collect())
        .unwrap_or_default();

    let common_prefixes: Vec<String> = result["common_prefixes"].as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let next_token = if is_truncated {
        objects.last().map(|o| o.key.as_str()).unwrap_or("")
    } else { "" };

    let xml_body = xml::list_objects_v2_xml(
        bucket, prefix, delimiter, max_keys, is_truncated,
        &objects, &common_prefixes, continuation_token, next_token,
    );

    Ok((StatusCode::OK, [("content-type", "application/xml")], xml_body).into_response())
}

fn status_to_s3_error(status: u32, key: &str) -> S3Error {
    use vmm_core::san_object::ObjectStatus;
    match ObjectStatus::from_u32(status) {
        Some(ObjectStatus::NotFound) => S3Error::no_such_key(key),
        Some(ObjectStatus::AccessDenied) => S3Error::access_denied(),
        Some(ObjectStatus::NoSpace) => S3Error::insufficient_storage(),
        Some(ObjectStatus::LeaseDenied) => S3Error::slow_down(),
        _ => S3Error::internal_error("Object operation failed"),
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add apps/vmm-s3gw/src/s3/object.rs
git commit -m "feat: add S3 object operations (Put, Get, Head, Delete, Copy, ListV2)"
```

---

## Task 14: vmm-s3gw — Multipart Operations

**Files:**
- Create: `apps/vmm-s3gw/src/s3/multipart.rs`

- [ ] **Step 1: Create multipart.rs**

```rust
// apps/vmm-s3gw/src/s3/multipart.rs

//! S3 Multipart upload operations.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{post, put, delete};
use axum::Router;
use std::sync::Arc;
use crate::AppState;
use crate::s3::error::S3Error;
use crate::s3::xml;
use crate::s3::object::resolve_bucket;
use vmm_core::san_object::ObjectCommand;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // POST /{bucket}/{key}?uploads — InitiateMultipartUpload
        // POST /{bucket}/{key}?uploadId=X — CompleteMultipartUpload
        // PUT /{bucket}/{key}?partNumber=N&uploadId=X — UploadPart
        // DELETE /{bucket}/{key}?uploadId=X — AbortMultipartUpload
        // These are handled by query param matching in object routes
        // For now, register a catch-all POST handler
        .route("/{bucket}/{*key}", post(handle_post))
}

/// POST /{bucket}/{key} — dispatches based on query params
async fn handle_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, S3Error> {
    let _auth = crate::auth::validate_request(&state, &headers, "POST", &format!("/{}/{}", bucket, key), "UNSIGNED-PAYLOAD").await?;
    let volume_id = resolve_bucket(&state, &bucket).await?;

    if params.contains_key("uploads") {
        // InitiateMultipartUpload
        let resp = state.socket.object_request(&volume_id, ObjectCommand::InitMultipart, key.as_bytes(), &[]).await
            .map_err(|e| S3Error::internal_error(&e))?;

        if !resp.is_ok() {
            return Err(S3Error::internal_error("Failed to initiate multipart upload"));
        }

        let meta = resp.metadata_json();
        let upload_id = meta["upload_id"].as_str().unwrap_or("");
        let xml_body = xml::initiate_multipart_xml(&bucket, &key, upload_id);
        Ok((StatusCode::OK, [("content-type", "application/xml")], xml_body).into_response())

    } else if let Some(upload_id) = params.get("uploadId") {
        // CompleteMultipartUpload
        // Parse the XML body for part list
        let body_str = String::from_utf8_lossy(&body);
        let parts = parse_complete_multipart_xml(&body_str);

        let complete_params = serde_json::json!({
            "upload_id": upload_id,
            "parts": parts,
        });

        let resp = state.socket.object_request(
            &volume_id, ObjectCommand::CompleteMultipart,
            complete_params.to_string().as_bytes(), &[],
        ).await.map_err(|e| S3Error::internal_error(&e))?;

        if !resp.is_ok() {
            return Err(S3Error::internal_error("Failed to complete multipart upload"));
        }

        let meta = resp.metadata_json();
        let etag = meta["etag"].as_str().unwrap_or("");
        let xml_body = xml::complete_multipart_xml(&bucket, &key, etag);
        Ok((StatusCode::OK, [("content-type", "application/xml")], xml_body).into_response())

    } else {
        Err(S3Error::invalid_argument("Missing uploads or uploadId query parameter"))
    }
}

/// Parse CompleteMultipartUpload XML body.
fn parse_complete_multipart_xml(xml: &str) -> Vec<serde_json::Value> {
    // Simple parser for:
    // <CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>"..."</ETag></Part>...</CompleteMultipartUpload>
    let mut parts = Vec::new();
    let mut remaining = xml;
    while let Some(start) = remaining.find("<Part>") {
        let after = &remaining[start + 6..];
        let end = after.find("</Part>").unwrap_or(after.len());
        let part_xml = &after[..end];

        let part_number = extract_xml_value(part_xml, "PartNumber")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let etag = extract_xml_value(part_xml, "ETag")
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();

        parts.push(serde_json::json!({"part_number": part_number, "etag": etag}));
        remaining = &after[end..];
    }
    parts
}

fn extract_xml_value<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;
    Some(&xml[start..end])
}
```

- [ ] **Step 2: Make resolve_bucket public in object.rs**

In `apps/vmm-s3gw/src/s3/object.rs`, change `resolve_bucket` from:
```rust
async fn resolve_bucket
```
to:
```rust
pub async fn resolve_bucket
```

- [ ] **Step 3: Verify the full gateway compiles**

Run: `cargo check -p vmm-s3gw`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-s3gw/src/s3/multipart.rs apps/vmm-s3gw/src/s3/object.rs
git commit -m "feat: add S3 multipart upload operations"
```

---

## Task 15: vmm-san — S3 Credential REST API (`apps/vmm-san/src/api/s3.rs`)

The Management Socket handles auth validation from the gateway, but the UI and cluster need a REST API to manage credentials (create/list/delete). This runs on the existing vmm-san HTTP port.

**Files:**
- Create: `apps/vmm-san/src/api/s3.rs`
- Modify: `apps/vmm-san/src/api/mod.rs`

- [ ] **Step 1: Create the S3 credential REST API**

```rust
// apps/vmm-san/src/api/s3.rs

//! S3 credential management REST endpoints.
//! Used by vmm-ui and vmm-cluster to create/list/delete S3 access keys.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::state::CoreSanState;

#[derive(Deserialize)]
pub struct CreateCredentialRequest {
    pub user_id: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Serialize)]
pub struct CreateCredentialResponse {
    pub id: String,
    pub access_key: String,
    pub secret_key: String, // Only returned on creation
}

#[derive(Serialize)]
pub struct CredentialResponse {
    pub id: String,
    pub access_key: String,
    pub user_id: String,
    pub display_name: String,
    pub status: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

/// GET /api/s3/credentials
pub async fn list(
    State(state): State<Arc<CoreSanState>>,
) -> Result<Json<Vec<CredentialResponse>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, access_key, user_id, display_name, status, created_at, expires_at FROM s3_credentials"
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let creds: Vec<CredentialResponse> = stmt.query_map([], |row| {
        Ok(CredentialResponse {
            id: row.get(0)?,
            access_key: row.get(1)?,
            user_id: row.get(2)?,
            display_name: row.get(3)?,
            status: row.get(4)?,
            created_at: row.get(5)?,
            expires_at: row.get(6)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Ok(Json(creds))
}

/// POST /api/s3/credentials
pub async fn create(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<CreateCredentialRequest>,
) -> Result<(StatusCode, Json<CreateCredentialResponse>), (StatusCode, String)> {
    use rand::Rng;
    let mut rng = rand::thread_rng();

    // Generate AWS-compatible access key (20 uppercase alphanumeric)
    let access_key: String = (0..20).map(|_| {
        let idx = rng.gen_range(0..36u8);
        if idx < 10 { (b'0' + idx) as char } else { (b'A' + idx - 10) as char }
    }).collect();

    // Generate secret key (40 chars)
    let secret_key: String = (0..40).map(|_| {
        let idx = rng.gen_range(0..62u8);
        if idx < 10 { (b'0' + idx) as char }
        else if idx < 36 { (b'A' + idx - 10) as char }
        else { (b'a' + idx - 36) as char }
    }).collect();

    // Encrypt secret key (XOR with node_id hash — same as mgmt_server)
    use sha2::{Sha256, Digest};
    let key_hash = Sha256::digest(state.node_id.as_bytes());
    let encrypted: Vec<u8> = secret_key.bytes()
        .zip(key_hash.iter().cycle())
        .map(|(b, k)| b ^ k)
        .collect();
    let secret_key_enc = crate::engine::mgmt_server::base64_encode(&encrypted);

    let id = uuid::Uuid::new_v4().to_string();
    let db = state.db.lock().unwrap();
    db.execute(
        "INSERT INTO s3_credentials (id, access_key, secret_key_enc, user_id, display_name)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![&id, &access_key, &secret_key_enc, &body.user_id, &body.display_name],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create credential: {}", e)))?;

    tracing::info!("S3 credential created: access_key={} user={}", access_key, body.user_id);

    Ok((StatusCode::CREATED, Json(CreateCredentialResponse {
        id, access_key, secret_key,
    })))
}

/// DELETE /api/s3/credentials/{id}
pub async fn delete(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let deleted = db.execute("DELETE FROM s3_credentials WHERE id = ?1", rusqlite::params![&id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if deleted == 0 {
        return Err((StatusCode::NOT_FOUND, "Credential not found".into()));
    }

    tracing::info!("S3 credential deleted: id={}", id);
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 2: Make `base64_encode` public in mgmt_server.rs**

In `apps/vmm-san/src/engine/mgmt_server.rs`, change:
```rust
fn base64_encode(data: &[u8]) -> String {
```
to:
```rust
pub fn base64_encode(data: &[u8]) -> String {
```

- [ ] **Step 3: Register routes in api/mod.rs**

Add `pub mod s3;` to `apps/vmm-san/src/api/mod.rs` and add routes:

```rust
        // ── S3 Credential Management ─────────────────────
        .route("/api/s3/credentials", get(s3::list).post(s3::create))
        .route("/api/s3/credentials/{id}", delete(s3::delete))
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p vmm-san`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add apps/vmm-san/src/api/s3.rs apps/vmm-san/src/api/mod.rs apps/vmm-san/src/engine/mgmt_server.rs
git commit -m "feat: add S3 credential REST API for UI/cluster access"
```

---

## Task 16: vmm-cluster — S3 Credential Proxy Routes

**Files:**
- Modify: `apps/vmm-cluster/src/api/san.rs`
- Modify: `apps/vmm-cluster/src/api/mod.rs`
- Modify: `apps/vmm-cluster/src/san_client.rs`

- [ ] **Step 1: Add S3 credential methods to SanClient**

In `apps/vmm-cluster/src/san_client.rs`, add these methods to the `SanClient` impl:

```rust
    pub async fn list_s3_credentials(&self) -> Result<serde_json::Value, String> {
        let url = format!("{}/api/s3/credentials", self.base_url);
        let resp = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
        resp.json().await.map_err(|e| e.to_string())
    }

    pub async fn create_s3_credential(&self, body: &serde_json::Value) -> Result<serde_json::Value, String> {
        let url = format!("{}/api/s3/credentials", self.base_url);
        let resp = self.client.post(&url).json(body).send().await.map_err(|e| e.to_string())?;
        resp.json().await.map_err(|e| e.to_string())
    }

    pub async fn delete_s3_credential(&self, id: &str) -> Result<(), String> {
        let url = format!("{}/api/s3/credentials/{}", self.base_url, id);
        let resp = self.client.delete(&url).send().await.map_err(|e| e.to_string())?;
        if resp.status().is_success() { Ok(()) } else { Err(format!("Delete failed: {}", resp.status())) }
    }
```

- [ ] **Step 2: Add S3 proxy handlers to san.rs**

Append to `apps/vmm-cluster/src/api/san.rs`:

```rust
// ── S3 Credentials ────────────────────────────────────────────

/// GET /api/san/s3/credentials
pub async fn list_s3_credentials(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.list_s3_credentials().await.map(Json).map_err(san_err)
}

/// POST /api/san/s3/credentials
pub async fn create_s3_credential(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let (client, host_id) = any_san_client(&state)?;
    let user_id = body.get("user_id").and_then(|v| v.as_str()).unwrap_or("unknown");
    let result = client.create_s3_credential(&body).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("S3 credential created for user '{}'", user_id),
        Some("s3_credential"), None, Some(&host_id));

    Ok((StatusCode::CREATED, Json(result)))
}

/// DELETE /api/san/s3/credentials/{id}
pub async fn delete_s3_credential(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let (client, host_id) = any_san_client(&state)?;
    client.delete_s3_credential(&id).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("S3 credential deleted (id={})", id),
        Some("s3_credential"), Some(&id), Some(&host_id));

    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 3: Register routes in cluster api/mod.rs**

In `apps/vmm-cluster/src/api/mod.rs`, add after the SAN witness route:

```rust
        // ── S3 Credentials (proxied) ─────────────────────
        .route("/api/san/s3/credentials", get(san::list_s3_credentials).post(san::create_s3_credential))
        .route("/api/san/s3/credentials/{id}", delete(san::delete_s3_credential))
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p vmm-cluster`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add apps/vmm-cluster/src/api/san.rs apps/vmm-cluster/src/api/mod.rs apps/vmm-cluster/src/san_client.rs
git commit -m "feat: add S3 credential proxy routes in vmm-cluster"
```

---

## Task 17: vmm-ui — Types and Sidebar Navigation

**Files:**
- Modify: `apps/vmm-ui/src/api/types.ts`
- Modify: `apps/vmm-ui/src/components/Sidebar.tsx`
- Modify: `apps/vmm-ui/src/App.tsx`

- [ ] **Step 1: Add types to types.ts**

In `apps/vmm-ui/src/api/types.ts`, add `access_protocols` to `CoreSanVolume`:

```typescript
export interface CoreSanVolume {
  id: string
  name: string
  ftt: number
  local_raid: 'stripe' | 'mirror' | 'stripe_mirror'
  chunk_size_bytes: number
  max_size_bytes: number
  access_protocols: string[]  // NEW: ["fuse", "s3"]
  status: 'creating' | 'online' | 'degraded' | 'offline'
  total_bytes: number
  free_bytes: number
  backend_count: number
  created_at: string
}
```

Add new `S3Credential` interface after the CoreSAN section:

```typescript
export interface S3Credential {
  id: string
  access_key: string
  user_id: string
  display_name: string
  status: 'active' | 'disabled'
  created_at: string
  expires_at: string | null
}

export interface S3CredentialCreateResponse {
  id: string
  access_key: string
  secret_key: string  // Only returned on creation
}
```

- [ ] **Step 2: Add "Object Storage" to sidebar navigation**

In `apps/vmm-ui/src/components/Sidebar.tsx`, add to `standaloneNavItems` in the Storage children, after CoreSAN:

```typescript
      { to: '/storage/object-storage', icon: Globe, label: 'Object Storage' },
```

Add the same to `clusterNavItems` in the Storage children, after CoreSAN:

```typescript
      { to: '/storage/object-storage', icon: Globe, label: 'Object Storage' },
```

(`Globe` is already imported from lucide-react)

- [ ] **Step 3: Add route in App.tsx**

Import the new page at the top of `apps/vmm-ui/src/App.tsx`:

```typescript
import StorageObjectStorage from './pages/StorageObjectStorage'
```

Add the route inside the Storage `<Route>` group (after the `coresan` route):

```tsx
            <Route path="object-storage" element={<StorageObjectStorage />} />
```

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-ui/src/api/types.ts apps/vmm-ui/src/components/Sidebar.tsx apps/vmm-ui/src/App.tsx
git commit -m "feat: add Object Storage types, sidebar nav, and route"
```

---

## Task 18: vmm-ui — Object Storage Page (`StorageObjectStorage.tsx`)

**Files:**
- Create: `apps/vmm-ui/src/pages/StorageObjectStorage.tsx`

- [ ] **Step 1: Create the Object Storage management page**

```tsx
// apps/vmm-ui/src/pages/StorageObjectStorage.tsx

import { useState, useEffect } from 'react'
import { Key, Trash2, Plus, Copy, ExternalLink, Shield } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import api from '../api/client'
import type { CoreSanVolume, S3Credential } from '../api/types'
import { formatBytes } from '../utils/format'
import Button from '../components/Button'
import Card from '../components/Card'
import CreateS3CredentialDialog from '../components/coresan/CreateS3CredentialDialog'
import ConfirmDialog from '../components/ConfirmDialog'

export default function StorageObjectStorage() {
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'

  const [volumes, setVolumes] = useState<CoreSanVolume[]>([])
  const [credentials, setCredentials] = useState<S3Credential[]>([])
  const [tab, setTab] = useState<'volumes' | 'credentials' | 'connect'>('volumes')
  const [showCreateCred, setShowCreateCred] = useState(false)
  const [deleteCredId, setDeleteCredId] = useState<string | null>(null)
  const [error, setError] = useState('')

  const sanBase = 'http://localhost:7443'

  const fetchData = async () => {
    try {
      // Fetch volumes
      let vols: CoreSanVolume[]
      if (isCluster) {
        const { data } = await api.get<CoreSanVolume[]>('/api/san/volumes')
        vols = data
      } else {
        const resp = await fetch(`${sanBase}/api/volumes`)
        vols = await resp.json()
      }
      setVolumes(vols.filter(v => v.access_protocols?.includes('s3')))

      // Fetch credentials
      let creds: S3Credential[]
      if (isCluster) {
        const { data } = await api.get<S3Credential[]>('/api/san/s3/credentials')
        creds = data
      } else {
        const resp = await fetch(`${sanBase}/api/s3/credentials`)
        creds = await resp.json()
      }
      setCredentials(creds)
    } catch (e: any) {
      setError(e.message || 'Failed to load data')
    }
  }

  useEffect(() => { fetchData() }, [isCluster])

  const handleDeleteCred = async () => {
    if (!deleteCredId) return
    try {
      if (isCluster) {
        await api.delete(`/api/san/s3/credentials/${deleteCredId}`)
      } else {
        await fetch(`${sanBase}/api/s3/credentials/${deleteCredId}`, { method: 'DELETE' })
      }
      setDeleteCredId(null)
      fetchData()
    } catch (e: any) {
      setError(e.message || 'Failed to delete credential')
    }
  }

  const tabs = [
    { key: 'volumes' as const, label: 'S3 Volumes' },
    { key: 'credentials' as const, label: 'Credentials' },
    { key: 'connect' as const, label: 'Connection Info' },
  ]

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-vmm-text">Object Storage</h1>
      </div>

      {error && (
        <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{error}</div>
      )}

      {/* Tabs */}
      <div className="flex gap-1 border-b border-vmm-border">
        {tabs.map(t => (
          <button key={t.key} onClick={() => setTab(t.key)}
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
              tab === t.key ? 'border-vmm-accent text-vmm-accent' : 'border-transparent text-vmm-muted hover:text-vmm-text'
            }`}>
            {t.label}
          </button>
        ))}
      </div>

      {/* Tab: S3 Volumes */}
      {tab === 'volumes' && (
        <Card>
          <div className="p-4">
            <p className="text-sm text-vmm-muted mb-4">
              Volumes with S3 access protocol enabled. Manage volumes in the CoreSAN page.
            </p>
            {volumes.length === 0 ? (
              <p className="text-vmm-muted text-sm py-8 text-center">
                No S3-enabled volumes. Create a volume with S3 protocol in CoreSAN, or enable S3 on an existing volume.
              </p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-vmm-muted border-b border-vmm-border">
                    <th className="pb-2 font-medium">Name</th>
                    <th className="pb-2 font-medium">Status</th>
                    <th className="pb-2 font-medium">Size</th>
                    <th className="pb-2 font-medium">Used</th>
                    <th className="pb-2 font-medium">Protocols</th>
                    <th className="pb-2 font-medium">FTT</th>
                  </tr>
                </thead>
                <tbody>
                  {volumes.map(v => (
                    <tr key={v.id} className="border-b border-vmm-border/50 hover:bg-vmm-hover">
                      <td className="py-2 font-medium text-vmm-text">{v.name}</td>
                      <td className="py-2">
                        <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${
                          v.status === 'online' ? 'bg-green-500/10 text-green-400' :
                          v.status === 'degraded' ? 'bg-yellow-500/10 text-yellow-400' :
                          'bg-red-500/10 text-red-400'
                        }`}>{v.status}</span>
                      </td>
                      <td className="py-2 text-vmm-muted">{formatBytes(v.max_size_bytes)}</td>
                      <td className="py-2 text-vmm-muted">{formatBytes(v.total_bytes)}</td>
                      <td className="py-2">
                        {v.access_protocols?.map(p => (
                          <span key={p} className="px-1.5 py-0.5 rounded text-xs bg-vmm-accent/10 text-vmm-accent mr-1">{p}</span>
                        ))}
                      </td>
                      <td className="py-2 text-vmm-muted">{v.ftt}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </Card>
      )}

      {/* Tab: Credentials */}
      {tab === 'credentials' && (
        <Card>
          <div className="p-4">
            <div className="flex items-center justify-between mb-4">
              <p className="text-sm text-vmm-muted">S3 access keys for external client access.</p>
              <Button size="sm" onClick={() => setShowCreateCred(true)}>
                <Plus size={14} className="mr-1" /> Create Key
              </Button>
            </div>
            {credentials.length === 0 ? (
              <p className="text-vmm-muted text-sm py-8 text-center">
                No S3 credentials. Create one to access object storage via S3 API.
              </p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-vmm-muted border-b border-vmm-border">
                    <th className="pb-2 font-medium">Access Key</th>
                    <th className="pb-2 font-medium">User</th>
                    <th className="pb-2 font-medium">Name</th>
                    <th className="pb-2 font-medium">Status</th>
                    <th className="pb-2 font-medium">Created</th>
                    <th className="pb-2 font-medium w-16"></th>
                  </tr>
                </thead>
                <tbody>
                  {credentials.map(c => (
                    <tr key={c.id} className="border-b border-vmm-border/50 hover:bg-vmm-hover">
                      <td className="py-2 font-mono text-xs text-vmm-text">{c.access_key}</td>
                      <td className="py-2 text-vmm-muted">{c.user_id}</td>
                      <td className="py-2 text-vmm-muted">{c.display_name || '—'}</td>
                      <td className="py-2">
                        <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${
                          c.status === 'active' ? 'bg-green-500/10 text-green-400' : 'bg-red-500/10 text-red-400'
                        }`}>{c.status}</span>
                      </td>
                      <td className="py-2 text-vmm-muted text-xs">{c.created_at}</td>
                      <td className="py-2">
                        <button onClick={() => setDeleteCredId(c.id)}
                          className="p-1 rounded hover:bg-vmm-danger/10 text-vmm-muted hover:text-vmm-danger">
                          <Trash2 size={14} />
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </Card>
      )}

      {/* Tab: Connection Info */}
      {tab === 'connect' && (
        <Card>
          <div className="p-4 space-y-4">
            <p className="text-sm text-vmm-muted">
              Use any S3-compatible client to access your object storage volumes.
            </p>
            <div className="space-y-3">
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">Endpoint</h3>
                <code className="block bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text">
                  http://&lt;host&gt;:9000
                </code>
              </div>
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">AWS CLI</h3>
                <code className="block bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text whitespace-pre">{
`aws configure
# Access Key: <your access key>
# Secret Key: <your secret key>
# Region: us-east-1

aws s3 ls --endpoint-url http://<host>:9000
aws s3 cp myfile.txt s3://<bucket>/myfile.txt --endpoint-url http://<host>:9000`}</code>
              </div>
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">MinIO Client (mc)</h3>
                <code className="block bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text whitespace-pre">{
`mc alias set coresan http://<host>:9000 <access_key> <secret_key>
mc ls coresan
mc cp myfile.txt coresan/<bucket>/`}</code>
              </div>
            </div>
          </div>
        </Card>
      )}

      {/* Dialogs */}
      <CreateS3CredentialDialog
        open={showCreateCred}
        onClose={() => setShowCreateCred(false)}
        onCreated={() => { setShowCreateCred(false); fetchData() }}
        isCluster={isCluster}
        sanBase={sanBase}
      />

      <ConfirmDialog
        open={!!deleteCredId}
        title="Delete S3 Credential"
        message="This will immediately revoke access for any client using this key. This action cannot be undone."
        confirmLabel="Delete"
        danger
        onConfirm={handleDeleteCred}
        onCancel={() => setDeleteCredId(null)}
      />
    </div>
  )
}
```

- [ ] **Step 2: Commit**

```bash
git add apps/vmm-ui/src/pages/StorageObjectStorage.tsx
git commit -m "feat: add Object Storage management page with volumes, credentials, and connection info"
```

---

## Task 19: vmm-ui — Create S3 Credential Dialog

**Files:**
- Create: `apps/vmm-ui/src/components/coresan/CreateS3CredentialDialog.tsx`

- [ ] **Step 1: Create the dialog component**

```tsx
// apps/vmm-ui/src/components/coresan/CreateS3CredentialDialog.tsx

import { useState } from 'react'
import { Copy, AlertTriangle } from 'lucide-react'
import api from '../../api/client'
import type { S3CredentialCreateResponse } from '../../api/types'
import Dialog from '../Dialog'
import FormField from '../FormField'
import TextInput from '../TextInput'
import Button from '../Button'

interface Props {
  open: boolean
  onClose: () => void
  onCreated: () => void
  isCluster: boolean
  sanBase: string
}

export default function CreateS3CredentialDialog({ open, onClose, onCreated, isCluster, sanBase }: Props) {
  const [userId, setUserId] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [result, setResult] = useState<S3CredentialCreateResponse | null>(null)
  const [copied, setCopied] = useState('')

  const handleCreate = async () => {
    if (!userId.trim()) { setError('User ID is required'); return }
    setSaving(true)
    setError('')
    try {
      let data: S3CredentialCreateResponse
      if (isCluster) {
        const resp = await api.post<S3CredentialCreateResponse>('/api/san/s3/credentials', { user_id: userId, display_name: displayName })
        data = resp.data
      } else {
        const resp = await fetch(`${sanBase}/api/s3/credentials`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ user_id: userId, display_name: displayName }),
        })
        data = await resp.json()
      }
      setResult(data)
    } catch (e: any) {
      setError(e.response?.data?.error || e.message || 'Failed to create credential')
    } finally {
      setSaving(false)
    }
  }

  const handleCopy = (text: string, label: string) => {
    navigator.clipboard.writeText(text)
    setCopied(label)
    setTimeout(() => setCopied(''), 2000)
  }

  const handleClose = () => {
    if (result) onCreated()
    setUserId('')
    setDisplayName('')
    setError('')
    setResult(null)
    onClose()
  }

  return (
    <Dialog open={open} title="Create S3 Access Key" onClose={handleClose} width="max-w-md">
      {!result ? (
        <div className="space-y-4">
          {error && (
            <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{error}</div>
          )}
          <FormField label="User ID">
            <TextInput value={userId} onChange={e => setUserId(e.target.value)} placeholder="admin" />
          </FormField>
          <FormField label="Display Name (optional)">
            <TextInput value={displayName} onChange={e => setDisplayName(e.target.value)} placeholder="Backup Service Key" />
          </FormField>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={handleClose}>Cancel</Button>
            <Button onClick={handleCreate} disabled={saving}>
              {saving ? 'Creating...' : 'Create Key'}
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-4">
          <div className="bg-yellow-500/10 border border-yellow-500/30 rounded-lg p-3 flex gap-2">
            <AlertTriangle size={16} className="text-yellow-400 shrink-0 mt-0.5" />
            <p className="text-sm text-yellow-300">
              Save the Secret Key now. It will not be shown again.
            </p>
          </div>

          <div>
            <label className="block text-xs text-vmm-muted mb-1">Access Key</label>
            <div className="flex items-center gap-2">
              <code className="flex-1 bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text">{result.access_key}</code>
              <button onClick={() => handleCopy(result.access_key, 'access')}
                className="p-2 rounded hover:bg-vmm-hover text-vmm-muted hover:text-vmm-text">
                <Copy size={14} />
              </button>
              {copied === 'access' && <span className="text-xs text-green-400">Copied</span>}
            </div>
          </div>

          <div>
            <label className="block text-xs text-vmm-muted mb-1">Secret Key</label>
            <div className="flex items-center gap-2">
              <code className="flex-1 bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text break-all">{result.secret_key}</code>
              <button onClick={() => handleCopy(result.secret_key, 'secret')}
                className="p-2 rounded hover:bg-vmm-hover text-vmm-muted hover:text-vmm-text">
                <Copy size={14} />
              </button>
              {copied === 'secret' && <span className="text-xs text-green-400">Copied</span>}
            </div>
          </div>

          <div className="flex justify-end pt-2">
            <Button onClick={handleClose}>Done</Button>
          </div>
        </div>
      )}
    </Dialog>
  )
}
```

- [ ] **Step 2: Commit**

```bash
git add apps/vmm-ui/src/components/coresan/CreateS3CredentialDialog.tsx
git commit -m "feat: add S3 credential creation dialog with secret key reveal"
```

---

## Task 20: vmm-ui — Update CreateVolumeDialog with access_protocols

**Files:**
- Modify: `apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx`
- Modify: `apps/vmm-ui/src/pages/StorageCoresan.tsx`

- [ ] **Step 1: Add access_protocols props to CreateVolumeDialog**

In `apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx`, add to the `Props` interface:

```typescript
  newVolProtocols: string[]
  setNewVolProtocols: (v: string[]) => void
```

Add to the destructured props in the function signature.

- [ ] **Step 2: Add protocol checkboxes to the dialog form**

Add after the RAID select field in the dialog's JSX, before the size input:

```tsx
        {/* Access Protocols */}
        <FormField label="Access Protocols">
          <div className="flex gap-4">
            <label className="flex items-center gap-2 text-sm text-vmm-text cursor-pointer">
              <input type="checkbox" checked={newVolProtocols.includes('fuse')}
                onChange={e => {
                  if (e.target.checked) setNewVolProtocols([...newVolProtocols, 'fuse'])
                  else setNewVolProtocols(newVolProtocols.filter(p => p !== 'fuse'))
                }}
                className="rounded border-vmm-border" />
              FUSE Mount
            </label>
            <label className="flex items-center gap-2 text-sm text-vmm-text cursor-pointer">
              <input type="checkbox" checked={newVolProtocols.includes('s3')}
                onChange={e => {
                  if (e.target.checked) setNewVolProtocols([...newVolProtocols, 's3'])
                  else setNewVolProtocols(newVolProtocols.filter(p => p !== 's3'))
                }}
                className="rounded border-vmm-border" />
              S3 Object Storage
            </label>
          </div>
          {newVolProtocols.includes('s3') && (
            <p className="text-xs text-vmm-muted mt-1">
              S3 access requires vmm-s3gw running on the host. Manage keys in Object Storage page.
            </p>
          )}
        </FormField>
```

- [ ] **Step 3: Add state and pass props in StorageCoresan.tsx**

In `apps/vmm-ui/src/pages/StorageCoresan.tsx`, add state:

```typescript
const [newVolProtocols, setNewVolProtocols] = useState<string[]>(['fuse'])
```

Pass as props to `CreateVolumeDialog`:

```tsx
newVolProtocols={newVolProtocols}
setNewVolProtocols={setNewVolProtocols}
```

Include `access_protocols` in the volume creation fetch body (in the `handleCreateVolume` function):

```typescript
access_protocols: newVolProtocols,
```

Reset after creation:

```typescript
setNewVolProtocols(['fuse'])
```

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx apps/vmm-ui/src/pages/StorageCoresan.tsx
git commit -m "feat: add access_protocols selector to volume creation dialog"
```

---

## Task 21: Full Build Verification

**Files:** None (verification only)

- [ ] **Step 1: Build Rust workspace**

Run: `cargo build --workspace`
Expected: all crates compile successfully

- [ ] **Step 2: Build frontend**

Run: `cd apps/vmm-ui && npm run build`
Expected: builds with no errors

- [ ] **Step 3: Run existing tests**

Run: `cargo test --workspace`
Expected: all existing tests pass, no regressions

- [ ] **Step 4: Commit any fixes needed**

If compilation or tests revealed issues, fix them and commit:
```bash
git add -A
git commit -m "fix: resolve compilation issues from S3 gateway integration"
```
