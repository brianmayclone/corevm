# CoreSAN iSCSI Target Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add iSCSI block storage as a new access protocol to CoreSAN with a pure-Rust iSCSI target service, cluster multipathing (ALUA), and full UI management.

**Architecture:** A new `vmm-iscsi` service listens on TCP :3260 and translates iSCSI PDUs into block I/O via Unix Domain Sockets to vmm-san. vmm-san is extended with a block I/O socket handler (`blk-{volume_id}.sock`), iSCSI ACL management, and ALUA state reporting. The UI gets a new "Block Storage" page analogous to the existing "Object Storage" page.

**Tech Stack:** Rust (tokio async), SQLite, React/TypeScript, vmm-core shared protocol library

**Spec:** `docs/superpowers/specs/2026-04-01-iscsi-target-design.md`

---

## File Structure

### New Files

| File | Responsibility |
|---|---|
| `libs/vmm-core/src/san_iscsi.rs` | Block I/O socket protocol (magic, commands, headers) |
| `apps/vmm-san/src/api/iscsi.rs` | REST API for iSCSI ACL management |
| `apps/vmm-san/src/engine/iscsi_server.rs` | Block I/O UDS handler per iSCSI-enabled volume |
| `apps/vmm-iscsi/Cargo.toml` | New crate for iSCSI target service |
| `apps/vmm-iscsi/src/main.rs` | Entry point, config, TCP listener |
| `apps/vmm-iscsi/src/config.rs` | TOML config loading |
| `apps/vmm-iscsi/src/socket.rs` | SocketPool for mgmt + blk UDS connections |
| `apps/vmm-iscsi/src/pdu.rs` | iSCSI PDU parsing/serialization |
| `apps/vmm-iscsi/src/session.rs` | Session state machine (login, full-feature) |
| `apps/vmm-iscsi/src/scsi.rs` | SCSI command handler (SBC minimal set) |
| `apps/vmm-iscsi/src/alua.rs` | ALUA state tracking, REPORT TARGET PORT GROUPS |
| `apps/vmm-iscsi/src/discovery.rs` | SendTargets response builder |
| `apps/vmm-ui/src/pages/StorageBlockStorage.tsx` | iSCSI management page (3 tabs) |
| `apps/vmm-ui/src/components/coresan/CreateIscsiAclDialog.tsx` | Dialog to add initiator IQN ACL |

### Modified Files

| File | Changes |
|---|---|
| `libs/vmm-core/src/lib.rs:12` | Add `pub mod san_iscsi;` |
| `libs/vmm-core/src/san_mgmt.rs:17-35` | Add iSCSI mgmt commands (40-45) |
| `apps/vmm-san/src/db/mod.rs:265-266` | Add `iscsi_acls` table to SCHEMA |
| `apps/vmm-san/src/api/mod.rs:15,68` | Add `pub mod iscsi;` and iSCSI routes |
| `apps/vmm-san/src/api/volumes.rs:62` | Add `"iscsi"` to valid_protocols |
| `apps/vmm-san/src/engine/mod.rs:29` | Add `pub mod iscsi_server;` |
| `apps/vmm-san/src/main.rs:284` | Add `iscsi_server::spawn_all()` call |
| `apps/vmm-san/src/engine/mgmt_server.rs` | Handle new iSCSI mgmt commands |
| `apps/vmm-cluster/src/san_client.rs:210` | Add iSCSI ACL client methods |
| `apps/vmm-cluster/src/api/san.rs:748` | Add iSCSI ACL proxy handlers |
| `apps/vmm-cluster/src/api/mod.rs:187` | Add iSCSI ACL proxy routes |
| `apps/vmm-ui/src/api/types.ts:527` | Add `IscsiAcl`, `IscsiTarget` types |
| `apps/vmm-ui/src/App.tsx:22,111` | Add StorageBlockStorage import + route |
| `apps/vmm-ui/src/components/Sidebar.tsx:32,91` | Add "Block Storage" nav item |
| `apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx:139` | Add iSCSI checkbox |

---

## Task 1: Block I/O Socket Protocol (`san_iscsi.rs`)

**Files:**
- Create: `libs/vmm-core/src/san_iscsi.rs`
- Modify: `libs/vmm-core/src/lib.rs:12`

- [ ] **Step 1: Create `san_iscsi.rs` with protocol definitions**

```rust
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

/// Socket path template for iSCSI block sockets: `/run/vmm-san/blk-{volume_id}.sock`
pub fn block_socket_path(volume_id: &str) -> String {
    format!("/run/vmm-san/blk-{}.sock", volume_id)
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
```

- [ ] **Step 2: Export module in `lib.rs`**

In `libs/vmm-core/src/lib.rs`, add after line 11 (`pub mod san_object;`):

```rust
pub mod san_iscsi;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/cmoeller/Development/corevm && cargo check -p vmm-core`
Expected: compiles without errors

- [ ] **Step 4: Commit**

```bash
git add libs/vmm-core/src/san_iscsi.rs libs/vmm-core/src/lib.rs
git commit -m "feat(san): add iSCSI block socket protocol to vmm-core"
```

---

## Task 2: Database Schema + iSCSI ACL REST API in vmm-san

**Files:**
- Modify: `apps/vmm-san/src/db/mod.rs:265-266` (add iscsi_acls table)
- Modify: `apps/vmm-san/src/api/volumes.rs:62` (add "iscsi" to valid_protocols)
- Create: `apps/vmm-san/src/api/iscsi.rs`
- Modify: `apps/vmm-san/src/api/mod.rs:15,68` (add module + routes)

- [ ] **Step 1: Add `iscsi_acls` table to database schema**

In `apps/vmm-san/src/db/mod.rs`, insert after line 265 (after `CREATE INDEX ... idx_multipart_uploads_status`):

```sql

-- ═══════════════════════════════════════════════════════════════
-- ISCSI_ACLS: initiator IQN access control per volume
-- Only initiators with a matching ACL entry can log in to a target
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS iscsi_acls (
    id              TEXT PRIMARY KEY,
    volume_id       TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    initiator_iqn   TEXT NOT NULL,
    comment         TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(volume_id, initiator_iqn)
);

CREATE INDEX IF NOT EXISTS idx_iscsi_acls_volume ON iscsi_acls(volume_id);
CREATE INDEX IF NOT EXISTS idx_iscsi_acls_iqn ON iscsi_acls(initiator_iqn);
```

- [ ] **Step 2: Add `"iscsi"` to valid protocols**

In `apps/vmm-san/src/api/volumes.rs`, change line 62:

Old:
```rust
    let valid_protocols = ["fuse", "s3"];
```

New:
```rust
    let valid_protocols = ["fuse", "s3", "iscsi"];
```

- [ ] **Step 3: Create `api/iscsi.rs`**

Create `apps/vmm-san/src/api/iscsi.rs`:

```rust
//! iSCSI ACL management REST endpoints.
//! Used by vmm-ui and vmm-cluster to create/list/delete initiator ACLs.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::state::CoreSanState;

#[derive(Deserialize)]
pub struct CreateAclRequest {
    pub volume_id: String,
    pub initiator_iqn: String,
    #[serde(default)]
    pub comment: String,
}

#[derive(Serialize)]
pub struct AclResponse {
    pub id: String,
    pub volume_id: String,
    pub volume_name: String,
    pub initiator_iqn: String,
    pub comment: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct AclQuery {
    pub volume_id: Option<String>,
}

#[derive(Serialize)]
pub struct TargetResponse {
    pub volume_id: String,
    pub volume_name: String,
    pub iqn: String,
    pub portals: Vec<String>,
    pub alua_state: String,
    pub status: String,
}

/// GET /api/iscsi/acls?volume_id=X
pub async fn list_acls(
    State(state): State<Arc<CoreSanState>>,
    Query(query): Query<AclQuery>,
) -> Result<Json<Vec<AclResponse>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let acls: Vec<AclResponse> = if let Some(ref vid) = query.volume_id {
        let mut stmt = db.prepare(
            "SELECT a.id, a.volume_id, v.name, a.initiator_iqn, a.comment, a.created_at
             FROM iscsi_acls a JOIN volumes v ON a.volume_id = v.id
             WHERE a.volume_id = ?1 ORDER BY a.created_at"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        stmt.query_map(rusqlite::params![vid], |row| {
            Ok(AclResponse {
                id: row.get(0)?, volume_id: row.get(1)?, volume_name: row.get(2)?,
                initiator_iqn: row.get(3)?, comment: row.get(4)?, created_at: row.get(5)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    } else {
        let mut stmt = db.prepare(
            "SELECT a.id, a.volume_id, v.name, a.initiator_iqn, a.comment, a.created_at
             FROM iscsi_acls a JOIN volumes v ON a.volume_id = v.id ORDER BY a.created_at"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        stmt.query_map([], |row| {
            Ok(AclResponse {
                id: row.get(0)?, volume_id: row.get(1)?, volume_name: row.get(2)?,
                initiator_iqn: row.get(3)?, comment: row.get(4)?, created_at: row.get(5)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    };
    Ok(Json(acls))
}

/// POST /api/iscsi/acls
pub async fn create_acl(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<CreateAclRequest>,
) -> Result<(StatusCode, Json<AclResponse>), (StatusCode, String)> {
    if !body.initiator_iqn.starts_with("iqn.") {
        return Err((StatusCode::BAD_REQUEST, "initiator_iqn must start with 'iqn.'".into()));
    }

    let db = state.db.lock().unwrap();

    // Verify volume exists and has iscsi protocol
    let vol_name: String = db.query_row(
        "SELECT name FROM volumes WHERE id = ?1", rusqlite::params![&body.volume_id],
        |row| row.get(0),
    ).map_err(|_| (StatusCode::NOT_FOUND, format!("Volume '{}' not found", body.volume_id)))?;

    let protos: String = db.query_row(
        "SELECT access_protocols FROM volumes WHERE id = ?1", rusqlite::params![&body.volume_id],
        |row| row.get(0),
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !protos.contains("iscsi") {
        return Err((StatusCode::BAD_REQUEST, "Volume does not have iSCSI protocol enabled".into()));
    }

    let id = uuid::Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO iscsi_acls (id, volume_id, initiator_iqn, comment) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![&id, &body.volume_id, &body.initiator_iqn, &body.comment],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create ACL: {}", e)))?;

    tracing::info!("iSCSI ACL created: volume={} iqn={}", body.volume_id, body.initiator_iqn);

    Ok((StatusCode::CREATED, Json(AclResponse {
        id, volume_id: body.volume_id, volume_name: vol_name,
        initiator_iqn: body.initiator_iqn, comment: body.comment,
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    })))
}

/// DELETE /api/iscsi/acls/{id}
pub async fn delete_acl(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let deleted = db.execute("DELETE FROM iscsi_acls WHERE id = ?1", rusqlite::params![&id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if deleted == 0 {
        return Err((StatusCode::NOT_FOUND, "ACL not found".into()));
    }

    tracing::info!("iSCSI ACL deleted: id={}", id);
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/iscsi/targets
pub async fn list_targets(
    State(state): State<Arc<CoreSanState>>,
) -> Result<Json<Vec<TargetResponse>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, name, status FROM volumes WHERE access_protocols LIKE '%iscsi%' AND status != 'deleted'"
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let node_name = &state.config.hostname;

    let targets: Vec<TargetResponse> = stmt.query_map([], |row| {
        let vol_id: String = row.get(0)?;
        let vol_name: String = row.get(1)?;
        let status: String = row.get(2)?;
        Ok(TargetResponse {
            volume_id: vol_id,
            volume_name: vol_name.clone(),
            iqn: format!("iqn.2026-04.io.corevm:{}", vol_name),
            portals: vec![format!("{}:3260", node_name)],
            alua_state: "active_optimized".to_string(),
            status,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Ok(Json(targets))
}
```

- [ ] **Step 4: Register module and routes in `api/mod.rs`**

In `apps/vmm-san/src/api/mod.rs`, add after line 15 (`pub mod s3;`):

```rust
pub mod iscsi;
```

And add after line 68 (after the S3 credential routes):

```rust

        // ── iSCSI ACL Management ─────────────────────────────
        .route("/api/iscsi/acls", get(iscsi::list_acls).post(iscsi::create_acl))
        .route("/api/iscsi/acls/{id}", delete(iscsi::delete_acl))
        .route("/api/iscsi/targets", get(iscsi::list_targets))
```

- [ ] **Step 5: Verify it compiles**

Run: `cd /home/cmoeller/Development/corevm && cargo check -p vmm-san`
Expected: compiles without errors

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-san/src/db/mod.rs apps/vmm-san/src/api/iscsi.rs apps/vmm-san/src/api/mod.rs apps/vmm-san/src/api/volumes.rs
git commit -m "feat(san): add iSCSI ACL database table, REST API, and protocol validation"
```

---

## Task 3: Block I/O Socket Handler in vmm-san (`iscsi_server.rs`)

**Files:**
- Create: `apps/vmm-san/src/engine/iscsi_server.rs`
- Modify: `apps/vmm-san/src/engine/mod.rs:29`
- Modify: `apps/vmm-san/src/main.rs:284`

- [ ] **Step 1: Create `iscsi_server.rs`**

Create `apps/vmm-san/src/engine/iscsi_server.rs`:

```rust
//! iSCSI block I/O server — serves block reads/writes via Unix Domain Socket.
//!
//! One UDS listener per iSCSI-enabled volume. vmm-iscsi connects to these sockets
//! to translate iSCSI SCSI commands into block I/O on the CoreSAN volume.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use vmm_core::san_iscsi::*;
use crate::state::CoreSanState;

const BLOCK_SIZE: u64 = 512;
const MAX_IO_SIZE: u32 = 4 * 1024 * 1024; // 4 MB max per request

/// Cached chunk in RAM for block I/O.
struct ChunkBuf {
    data: Vec<u8>,
    dirty: bool,
}

/// Per-connection state for iSCSI block I/O.
struct BlockSession {
    volume_id: String,
    max_size_bytes: u64,
    chunk_size: u64,
    local_raid: String,
    cache: HashMap<u32, ChunkBuf>,
}

/// Spawn UDS listeners for all iSCSI-enabled online volumes.
pub fn spawn_all(state: Arc<CoreSanState>) {
    let volumes: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name FROM volumes WHERE status = 'online' AND access_protocols LIKE '%iscsi%'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    std::fs::create_dir_all("/run/vmm-san").ok();

    for (vol_id, vol_name) in volumes {
        spawn_volume_listener(state.clone(), vol_id, vol_name);
    }
}

/// Spawn a single UDS listener for a volume's block I/O.
pub fn spawn_volume_listener(state: Arc<CoreSanState>, volume_id: String, volume_name: String) {
    std::fs::create_dir_all("/run/vmm-san").ok();
    let sock_path = block_socket_path(&volume_id);
    std::fs::remove_file(&sock_path).ok();

    tokio::spawn(async move {
        let listener = match UnixListener::bind(&sock_path) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("iSCSI block server: cannot bind {}: {}", sock_path, e);
                return;
            }
        };

        std::fs::set_permissions(&sock_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o666)).ok();

        tracing::info!("iSCSI block server: listening on {} (volume '{}')", sock_path, volume_name);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let st = state.clone();
                    let vid = volume_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, st, &vid).await {
                            tracing::debug!("iSCSI block session ended: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("iSCSI block server accept error: {}", e);
                }
            }
        }
    });
}

async fn handle_connection(
    mut stream: UnixStream,
    state: Arc<CoreSanState>,
    volume_id: &str,
) -> Result<(), String> {
    // Load volume metadata
    let (max_size_bytes, chunk_size, local_raid) = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT max_size_bytes, chunk_size_bytes, local_raid FROM volumes WHERE id = ?1"
        ).map_err(|e| e.to_string())?;
        stmt.query_row(rusqlite::params![volume_id], |row| {
            Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?, row.get::<_, String>(2)?))
        }).map_err(|e| format!("volume lookup: {}", e))?
    };

    let mut session = BlockSession {
        volume_id: volume_id.to_string(),
        max_size_bytes,
        chunk_size,
        local_raid,
        cache: HashMap::new(),
    };

    loop {
        // Read request header
        let mut hdr_buf = [0u8; IscsiRequestHeader::SIZE];
        if let Err(e) = stream.read_exact(&mut hdr_buf).await {
            // Flush dirty chunks before disconnect
            flush_all(&mut session, &state).await;
            return Err(format!("read header: {}", e));
        }

        let hdr = IscsiRequestHeader::from_bytes(&hdr_buf);
        if hdr.magic != ISCSI_REQUEST_MAGIC {
            return Err("invalid magic".into());
        }

        let cmd = IscsiCommand::from_u32(hdr.cmd)
            .ok_or_else(|| format!("unknown command: {}", hdr.cmd))?;

        match cmd {
            IscsiCommand::ReadBlocks => {
                let offset = hdr.lba * BLOCK_SIZE;
                let length = hdr.length;
                if length > MAX_IO_SIZE {
                    send_error(&mut stream, IscsiStatus::ProtocolError).await;
                    continue;
                }
                if offset + length as u64 > max_size_bytes {
                    send_error(&mut stream, IscsiStatus::OutOfRange).await;
                    continue;
                }

                match read_blocks(&mut session, &state, offset, length as usize).await {
                    Ok(data) => {
                        let resp = IscsiResponseHeader::ok(data.len() as u32);
                        stream.write_all(&resp.to_bytes()).await.map_err(|e| e.to_string())?;
                        stream.write_all(&data).await.map_err(|e| e.to_string())?;
                    }
                    Err(_) => send_error(&mut stream, IscsiStatus::IoError).await,
                }
            }
            IscsiCommand::WriteBlocks => {
                let offset = hdr.lba * BLOCK_SIZE;
                let length = hdr.length;
                if length > MAX_IO_SIZE {
                    send_error(&mut stream, IscsiStatus::ProtocolError).await;
                    continue;
                }
                if offset + length as u64 > max_size_bytes {
                    send_error(&mut stream, IscsiStatus::OutOfRange).await;
                    continue;
                }

                let mut data = vec![0u8; length as usize];
                stream.read_exact(&mut data).await.map_err(|e| format!("read data: {}", e))?;

                match write_blocks(&mut session, &state, offset, &data).await {
                    Ok(()) => {
                        let resp = IscsiResponseHeader::ok(0);
                        stream.write_all(&resp.to_bytes()).await.map_err(|e| e.to_string())?;
                    }
                    Err(_) => send_error(&mut stream, IscsiStatus::IoError).await,
                }
            }
            IscsiCommand::Flush => {
                flush_all(&mut session, &state).await;
                let resp = IscsiResponseHeader::ok(0);
                stream.write_all(&resp.to_bytes()).await.map_err(|e| e.to_string())?;
            }
            IscsiCommand::GetCapacity => {
                let body = serde_json::json!({
                    "size_bytes": max_size_bytes,
                    "block_size": BLOCK_SIZE,
                }).to_string();
                let body_bytes = body.as_bytes();
                let resp = IscsiResponseHeader::ok(body_bytes.len() as u32);
                stream.write_all(&resp.to_bytes()).await.map_err(|e| e.to_string())?;
                stream.write_all(body_bytes).await.map_err(|e| e.to_string())?;
            }
            IscsiCommand::GetAluaState => {
                // Determine if this node is leader for the volume
                let is_leader = {
                    let db = state.db.lock().unwrap();
                    let leader: Option<String> = db.query_row(
                        "SELECT leader_node_id FROM volumes WHERE id = ?1",
                        rusqlite::params![volume_id], |row| row.get(0),
                    ).ok();
                    leader.as_deref() == Some(&state.node_id)
                };
                let alua_state = if is_leader { "active_optimized" } else { "active_non_optimized" };
                let body = serde_json::json!({
                    "state": alua_state,
                    "tpg_id": &state.node_id,
                }).to_string();
                let body_bytes = body.as_bytes();
                let resp = IscsiResponseHeader::ok(body_bytes.len() as u32);
                stream.write_all(&resp.to_bytes()).await.map_err(|e| e.to_string())?;
                stream.write_all(body_bytes).await.map_err(|e| e.to_string())?;
            }
        }

        stream.flush().await.map_err(|e| e.to_string())?;
    }
}

async fn read_blocks(
    session: &mut BlockSession,
    state: &Arc<CoreSanState>,
    offset: u64,
    length: usize,
) -> Result<Vec<u8>, String> {
    let mut result = vec![0u8; length];
    let mut pos = 0usize;
    let mut byte_offset = offset;

    while pos < length {
        let chunk_index = (byte_offset / session.chunk_size) as u32;
        let chunk_offset = (byte_offset % session.chunk_size) as usize;
        let available = session.chunk_size as usize - chunk_offset;
        let to_read = available.min(length - pos);

        let chunk = load_chunk(session, state, chunk_index).await?;
        let end = (chunk_offset + to_read).min(chunk.data.len());
        let actual = end.saturating_sub(chunk_offset);
        if actual > 0 {
            result[pos..pos + actual].copy_from_slice(&chunk.data[chunk_offset..end]);
        }

        pos += to_read;
        byte_offset += to_read as u64;
    }

    Ok(result)
}

async fn write_blocks(
    session: &mut BlockSession,
    state: &Arc<CoreSanState>,
    offset: u64,
    data: &[u8],
) -> Result<(), String> {
    let mut pos = 0usize;
    let mut byte_offset = offset;

    while pos < data.len() {
        let chunk_index = (byte_offset / session.chunk_size) as u32;
        let chunk_offset = (byte_offset % session.chunk_size) as usize;
        let available = session.chunk_size as usize - chunk_offset;
        let to_write = available.min(data.len() - pos);

        let chunk = load_chunk(session, state, chunk_index).await?;
        // Extend chunk if needed
        let needed = chunk_offset + to_write;
        if chunk.data.len() < needed {
            chunk.data.resize(needed, 0);
        }
        chunk.data[chunk_offset..chunk_offset + to_write].copy_from_slice(&data[pos..pos + to_write]);
        chunk.dirty = true;

        pos += to_write;
        byte_offset += to_write as u64;
    }

    Ok(())
}

async fn load_chunk<'a>(
    session: &'a mut BlockSession,
    state: &Arc<CoreSanState>,
    chunk_index: u32,
) -> Result<&'a mut ChunkBuf, String> {
    if !session.cache.contains_key(&chunk_index) {
        // Read chunk from storage backends via ChunkService
        let data = crate::services::chunk::ChunkService::read_chunk_data(
            state, &session.volume_id, 0, chunk_index,
            &session.local_raid, session.chunk_size,
        ).await.unwrap_or_else(|_| vec![0u8; session.chunk_size as usize]);

        session.cache.insert(chunk_index, ChunkBuf { data, dirty: false });
    }

    Ok(session.cache.get_mut(&chunk_index).unwrap())
}

async fn flush_all(session: &mut BlockSession, state: &Arc<CoreSanState>) {
    let dirty_chunks: Vec<u32> = session.cache.iter()
        .filter(|(_, c)| c.dirty)
        .map(|(idx, _)| *idx)
        .collect();

    for chunk_index in dirty_chunks {
        if let Some(chunk) = session.cache.get_mut(&chunk_index) {
            let _ = crate::services::chunk::ChunkService::write_chunk_data(
                state, &session.volume_id, 0, chunk_index,
                &chunk.data, &session.local_raid, session.chunk_size,
            ).await;
            chunk.dirty = false;
        }
    }
}

async fn send_error(stream: &mut UnixStream, status: IscsiStatus) {
    let resp = IscsiResponseHeader::err(status);
    let _ = stream.write_all(&resp.to_bytes()).await;
}
```

- [ ] **Step 2: Register module in `engine/mod.rs`**

In `apps/vmm-san/src/engine/mod.rs`, add after line 28 (`pub mod dedup;`):

```rust
pub mod iscsi_server;
```

- [ ] **Step 3: Spawn at startup in `main.rs`**

In `apps/vmm-san/src/main.rs`, add after line 284 (`tracing::info!("Object server started ...");`):

```rust
    engine::iscsi_server::spawn_all(Arc::clone(&state));
    tracing::info!("iSCSI block server started (UDS per iSCSI-enabled volume)");
```

- [ ] **Step 4: Verify it compiles**

Run: `cd /home/cmoeller/Development/corevm && cargo check -p vmm-san`
Expected: compiles without errors (may need to adjust ChunkService method signatures — check exact API in `apps/vmm-san/src/services/chunk.rs` and adapt `read_chunk_data`/`write_chunk_data` calls accordingly)

- [ ] **Step 5: Commit**

```bash
git add apps/vmm-san/src/engine/iscsi_server.rs apps/vmm-san/src/engine/mod.rs apps/vmm-san/src/main.rs
git commit -m "feat(san): add iSCSI block I/O socket handler with chunk-based read/write"
```

---

## Task 4: Extend Management Socket with iSCSI Commands

**Files:**
- Modify: `libs/vmm-core/src/san_mgmt.rs:17-35`
- Modify: `apps/vmm-san/src/engine/mgmt_server.rs`

- [ ] **Step 1: Add iSCSI commands to `san_mgmt.rs`**

In `libs/vmm-core/src/san_mgmt.rs`, add new variants to `MgmtCommand` enum after `ResolveVolume = 30` (line 34):

```rust
    // ── iSCSI ────────────────────────────────────────────
    /// List volumes with "iscsi" in access_protocols. Response body = JSON array.
    ListIscsiVolumes = 40,
    /// List iSCSI ACLs for a volume. Key = volume_id. Response body = JSON array.
    ListIscsiAcls = 41,
    /// Create iSCSI ACL. Body = JSON {volume_id, initiator_iqn, comment}.
    CreateIscsiAcl = 42,
    /// Delete iSCSI ACL. Key = acl_id.
    DeleteIscsiAcl = 43,
    /// Get ALUA state for a volume on this node. Key = volume_id. Response body = JSON.
    GetAluaState = 44,
    /// Get all target port groups for a volume. Key = volume_id. Response body = JSON array.
    GetTargetPortGroups = 45,
```

And add the match arms in `from_u32`:

```rust
            40 => Some(Self::ListIscsiVolumes),
            41 => Some(Self::ListIscsiAcls),
            42 => Some(Self::CreateIscsiAcl),
            43 => Some(Self::DeleteIscsiAcl),
            44 => Some(Self::GetAluaState),
            45 => Some(Self::GetTargetPortGroups),
```

- [ ] **Step 2: Handle new commands in `mgmt_server.rs`**

In `apps/vmm-san/src/engine/mgmt_server.rs`, find the `match cmd { ... }` dispatch block and add handlers for the new commands. The handlers follow the same pattern as `ListVolumes` and `ValidateCredential`:

For `ListIscsiVolumes`:
```rust
MgmtCommand::ListIscsiVolumes => {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, name, status, max_size_bytes, access_protocols FROM volumes WHERE access_protocols LIKE '%iscsi%' AND status != 'deleted'"
    ).unwrap();
    let vols: Vec<serde_json::Value> = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "status": row.get::<_, String>(2)?,
            "max_size_bytes": row.get::<_, u64>(3)?,
            "access_protocols": row.get::<_, String>(4)?,
        }))
    }).unwrap().filter_map(|r| r.ok()).collect();
    let body = serde_json::to_vec(&vols).unwrap_or_default();
    send_response(&mut stream, MgmtResponseHeader::ok(body.len() as u64, 0), &[], &body).await;
}
```

For `ListIscsiAcls`:
```rust
MgmtCommand::ListIscsiAcls => {
    let volume_id = std::str::from_utf8(&key).unwrap_or("");
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, volume_id, initiator_iqn, comment, created_at FROM iscsi_acls WHERE volume_id = ?1"
    ).unwrap();
    let acls: Vec<serde_json::Value> = stmt.query_map(rusqlite::params![volume_id], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "volume_id": row.get::<_, String>(1)?,
            "initiator_iqn": row.get::<_, String>(2)?,
            "comment": row.get::<_, String>(3)?,
            "created_at": row.get::<_, String>(4)?,
        }))
    }).unwrap().filter_map(|r| r.ok()).collect();
    let body = serde_json::to_vec(&acls).unwrap_or_default();
    send_response(&mut stream, MgmtResponseHeader::ok(body.len() as u64, 0), &[], &body).await;
}
```

For `CreateIscsiAcl`:
```rust
MgmtCommand::CreateIscsiAcl => {
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let volume_id = body_json["volume_id"].as_str().unwrap_or("");
    let iqn = body_json["initiator_iqn"].as_str().unwrap_or("");
    let comment = body_json["comment"].as_str().unwrap_or("");
    let id = uuid::Uuid::new_v4().to_string();
    let db = state.db.lock().unwrap();
    match db.execute(
        "INSERT INTO iscsi_acls (id, volume_id, initiator_iqn, comment) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![&id, volume_id, iqn, comment],
    ) {
        Ok(_) => {
            let resp = serde_json::json!({"id": id}).to_string();
            let resp_bytes = resp.as_bytes();
            send_response(&mut stream, MgmtResponseHeader::ok(resp_bytes.len() as u64, 0), &[], resp_bytes).await;
        }
        Err(_) => {
            send_response(&mut stream, MgmtResponseHeader::err(MgmtStatus::AlreadyExists), &[], &[]).await;
        }
    }
}
```

For `DeleteIscsiAcl`:
```rust
MgmtCommand::DeleteIscsiAcl => {
    let acl_id = std::str::from_utf8(&key).unwrap_or("");
    let db = state.db.lock().unwrap();
    let deleted = db.execute("DELETE FROM iscsi_acls WHERE id = ?1", rusqlite::params![acl_id]).unwrap_or(0);
    if deleted > 0 {
        send_response(&mut stream, MgmtResponseHeader::ok(0, 0), &[], &[]).await;
    } else {
        send_response(&mut stream, MgmtResponseHeader::err(MgmtStatus::NotFound), &[], &[]).await;
    }
}
```

For `GetAluaState`:
```rust
MgmtCommand::GetAluaState => {
    let volume_id = std::str::from_utf8(&key).unwrap_or("");
    let is_leader = {
        let db = state.db.lock().unwrap();
        let leader: Option<String> = db.query_row(
            "SELECT leader_node_id FROM volumes WHERE id = ?1",
            rusqlite::params![volume_id], |row| row.get(0),
        ).ok();
        leader.as_deref() == Some(&state.node_id)
    };
    let alua_state = if is_leader { "active_optimized" } else { "active_non_optimized" };
    let body = serde_json::json!({"state": alua_state, "tpg_id": &state.node_id}).to_string();
    let body_bytes = body.as_bytes();
    send_response(&mut stream, MgmtResponseHeader::ok(body_bytes.len() as u64, 0), &[], body_bytes).await;
}
```

For `GetTargetPortGroups`:
```rust
MgmtCommand::GetTargetPortGroups => {
    let volume_id = std::str::from_utf8(&key).unwrap_or("");
    let db = state.db.lock().unwrap();
    let leader: Option<String> = db.query_row(
        "SELECT leader_node_id FROM volumes WHERE id = ?1",
        rusqlite::params![volume_id], |row| row.get(0),
    ).ok();
    // List all peers that have this volume
    let mut stmt = db.prepare(
        "SELECT DISTINCT node_id FROM chunk_replicas cr
         JOIN file_chunks fc ON cr.chunk_id = fc.chunk_id
         JOIN file_map fm ON fc.file_id = fm.file_id AND fc.volume_id = fm.volume_id
         WHERE fc.volume_id = ?1"
    ).unwrap();
    let tpgs: Vec<serde_json::Value> = stmt.query_map(rusqlite::params![volume_id], |row| {
        let node_id: String = row.get(0)?;
        let is_leader = leader.as_deref() == Some(&node_id);
        let state = if is_leader { "active_optimized" } else { "active_non_optimized" };
        Ok(serde_json::json!({"tpg_id": node_id, "state": state}))
    }).unwrap().filter_map(|r| r.ok()).collect();
    let body = serde_json::to_vec(&tpgs).unwrap_or_default();
    send_response(&mut stream, MgmtResponseHeader::ok(body.len() as u64, 0), &[], &body).await;
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/cmoeller/Development/corevm && cargo check -p vmm-san -p vmm-core`
Expected: compiles without errors

- [ ] **Step 4: Commit**

```bash
git add libs/vmm-core/src/san_mgmt.rs apps/vmm-san/src/engine/mgmt_server.rs
git commit -m "feat(san): extend management socket with iSCSI ACL and ALUA commands"
```

---

## Task 5: Cluster Proxy Routes for iSCSI

**Files:**
- Modify: `apps/vmm-cluster/src/san_client.rs:210`
- Modify: `apps/vmm-cluster/src/api/san.rs:748`
- Modify: `apps/vmm-cluster/src/api/mod.rs:187`

- [ ] **Step 1: Add iSCSI client methods to `san_client.rs`**

In `apps/vmm-cluster/src/san_client.rs`, add after line 209 (after `delete_s3_credential`), before the `// ── Internal HTTP helpers` comment:

```rust
    // ���─ iSCSI ACLs ───────────────────────────────────────────

    pub async fn list_iscsi_acls(&self, volume_id: Option<&str>) -> Result<Value, String> {
        let path = match volume_id {
            Some(vid) => format!("/api/iscsi/acls?volume_id={}", vid),
            None => "/api/iscsi/acls".to_string(),
        };
        self.get(&path).await
    }

    pub async fn create_iscsi_acl(&self, body: &Value) -> Result<Value, String> {
        self.post("/api/iscsi/acls", body).await
    }

    pub async fn delete_iscsi_acl(&self, id: &str) -> Result<(), String> {
        let url = format!("{}/api/iscsi/acls/{}", self.base_url, id);
        let resp = self.http.delete(&url).send().await.map_err(|e| format!("SAN request failed ({}): {}", url, e))?;
        if resp.status().is_success() { Ok(()) } else { Err(format!("Delete failed: {}", resp.status())) }
    }

    pub async fn list_iscsi_targets(&self) -> Result<Value, String> {
        self.get("/api/iscsi/targets").await
    }
```

- [ ] **Step 2: Add proxy handlers to `api/san.rs`**

In `apps/vmm-cluster/src/api/san.rs`, add after line 748 (after `delete_s3_credential` function):

```rust

// ── iSCSI ACLs ───────────────────────────────────────────

/// GET /api/san/iscsi/acls
pub async fn list_iscsi_acls(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    let volume_id = query.get("volume_id").map(|s| s.as_str());
    client.list_iscsi_acls(volume_id).await.map(Json).map_err(san_err)
}

/// POST /api/san/iscsi/acls
pub async fn create_iscsi_acl(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let (client, host_id) = any_san_client(&state)?;
    let iqn = body.get("initiator_iqn").and_then(|v| v.as_str()).unwrap_or("unknown");
    let result = client.create_iscsi_acl(&body).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("iSCSI ACL created for initiator '{}'", iqn),
        Some("iscsi_acl"), None, Some(&host_id));

    Ok((StatusCode::CREATED, Json(result)))
}

/// DELETE /api/san/iscsi/acls/{id}
pub async fn delete_iscsi_acl(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let (client, host_id) = any_san_client(&state)?;
    client.delete_iscsi_acl(&id).await.map_err(san_err)?;

    let db = state.db.lock().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    EventService::log(&db, "info", "san",
        &format!("iSCSI ACL deleted (id={})", id),
        Some("iscsi_acl"), Some(&id), Some(&host_id));

    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/san/iscsi/targets
pub async fn list_iscsi_targets(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (client, _) = any_san_client(&state)?;
    client.list_iscsi_targets().await.map(Json).map_err(san_err)
}
```

Ensure `HashMap` is imported at the top of the file (add `use std::collections::HashMap;` if missing).

- [ ] **Step 3: Register proxy routes in `api/mod.rs`**

In `apps/vmm-cluster/src/api/mod.rs`, add after line 187 (after S3 credential routes):

```rust

        // ── iSCSI ACLs (proxied) ────────────────────────
        .route("/api/san/iscsi/acls", get(san::list_iscsi_acls).post(san::create_iscsi_acl))
        .route("/api/san/iscsi/acls/{id}", delete(san::delete_iscsi_acl))
        .route("/api/san/iscsi/targets", get(san::list_iscsi_targets))
```

- [ ] **Step 4: Verify it compiles**

Run: `cd /home/cmoeller/Development/corevm && cargo check -p vmm-cluster`
Expected: compiles without errors

- [ ] **Step 5: Commit**

```bash
git add apps/vmm-cluster/src/san_client.rs apps/vmm-cluster/src/api/san.rs apps/vmm-cluster/src/api/mod.rs
git commit -m "feat(cluster): add iSCSI ACL proxy routes to cluster API"
```

---

## Task 6: vmm-iscsi Service — Crate Setup, Config, Socket Pool

**Files:**
- Create: `apps/vmm-iscsi/Cargo.toml`
- Create: `apps/vmm-iscsi/src/main.rs`
- Create: `apps/vmm-iscsi/src/config.rs`
- Create: `apps/vmm-iscsi/src/socket.rs`

- [ ] **Step 1: Create `Cargo.toml`**

Create `apps/vmm-iscsi/Cargo.toml`:

```toml
[package]
name = "vmm-iscsi"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
vmm-core = { path = "../../libs/vmm-core" }
```

- [ ] **Step 2: Create `config.rs`**

Create `apps/vmm-iscsi/src/config.rs`:

```rust
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct IscsiConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub san: SanSection,
    #[serde(default)]
    pub logging: LoggingSection,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerSection {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_node_name")]
    pub node_name: String,
}

fn default_listen() -> String { "0.0.0.0:3260".to_string() }
fn default_node_name() -> String { "iqn.2026-04.io.corevm".to_string() }

impl Default for ServerSection {
    fn default() -> Self {
        Self { listen: default_listen(), node_name: default_node_name() }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SanSection {
    #[serde(default = "default_mgmt_socket")]
    pub mgmt_socket: String,
    #[serde(default = "default_block_socket_dir")]
    pub block_socket_dir: String,
}

fn default_mgmt_socket() -> String { "/run/vmm-san/mgmt.sock".to_string() }
fn default_block_socket_dir() -> String { "/run/vmm-san".to_string() }

impl Default for SanSection {
    fn default() -> Self {
        Self { mgmt_socket: default_mgmt_socket(), block_socket_dir: default_block_socket_dir() }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingSection {
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String { "info".to_string() }

impl Default for LoggingSection {
    fn default() -> Self { Self { level: default_log_level() } }
}

impl IscsiConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            tracing::warn!("Config file {} not found, using defaults", path.display());
            let config: IscsiConfig = toml::from_str("").map_err(|e| format!("default config error: {}", e))?;
            return Ok(config);
        }
        let contents = std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
        let config: IscsiConfig = toml::from_str(&contents).map_err(|e| format!("parse {}: {}", path.display(), e))?;
        Ok(config)
    }
}
```

- [ ] **Step 3: Create `socket.rs`**

Create `apps/vmm-iscsi/src/socket.rs`:

```rust
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use vmm_core::san_mgmt::*;
use vmm_core::san_iscsi::*;

pub struct MgmtResponse {
    pub status: u32,
    pub metadata: Vec<u8>,
    pub body: Vec<u8>,
}

impl MgmtResponse {
    pub fn is_ok(&self) -> bool { self.status == MgmtStatus::Ok as u32 }

    pub fn body_json(&self) -> serde_json::Value {
        if self.body.is_empty() { serde_json::Value::Null }
        else { serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null) }
    }
}

pub struct BlockResponse {
    pub status: u32,
    pub data: Vec<u8>,
}

impl BlockResponse {
    pub fn is_ok(&self) -> bool { self.status == IscsiStatus::Ok as u32 }
}

pub struct SocketPool {
    mgmt_path: String,
    block_socket_dir: String,
    mgmt_conn: Mutex<Option<UnixStream>>,
    blk_conns: Mutex<HashMap<String, UnixStream>>,
}

impl SocketPool {
    pub fn new(config: &crate::config::SanSection) -> Self {
        Self {
            mgmt_path: config.mgmt_socket.clone(),
            block_socket_dir: config.block_socket_dir.clone(),
            mgmt_conn: Mutex::new(None),
            blk_conns: Mutex::new(HashMap::new()),
        }
    }

    pub async fn mgmt_request(
        &self, cmd: MgmtCommand, key: &[u8], body: &[u8],
    ) -> Result<MgmtResponse, String> {
        let mut guard = self.mgmt_conn.lock().await;
        if guard.is_none() {
            let stream = UnixStream::connect(&self.mgmt_path).await
                .map_err(|e| format!("connect {}: {}", self.mgmt_path, e))?;
            *guard = Some(stream);
        }
        let result = Self::do_mgmt_request(guard.as_mut().unwrap(), cmd, key, body).await;
        if result.is_err() { *guard = None; }
        result
    }

    async fn do_mgmt_request(
        stream: &mut UnixStream, cmd: MgmtCommand, key: &[u8], body: &[u8],
    ) -> Result<MgmtResponse, String> {
        let header = MgmtRequestHeader::new(cmd, key.len() as u32, body.len() as u64);
        stream.write_all(&header.to_bytes()).await.map_err(|e| format!("write header: {}", e))?;
        if !key.is_empty() { stream.write_all(key).await.map_err(|e| format!("write key: {}", e))?; }
        if !body.is_empty() { stream.write_all(body).await.map_err(|e| format!("write body: {}", e))?; }
        stream.flush().await.map_err(|e| format!("flush: {}", e))?;

        let mut resp_buf = [0u8; MgmtResponseHeader::SIZE];
        stream.read_exact(&mut resp_buf).await.map_err(|e| format!("read resp header: {}", e))?;
        let resp_header = MgmtResponseHeader::from_bytes(&resp_buf);
        if resp_header.magic != MGMT_RESPONSE_MAGIC { return Err("invalid mgmt response magic".into()); }

        let mut metadata = vec![0u8; resp_header.metadata_len as usize];
        if !metadata.is_empty() { stream.read_exact(&mut metadata).await.map_err(|e| format!("read metadata: {}", e))?; }
        let mut resp_body = vec![0u8; resp_header.body_len as usize];
        if !resp_body.is_empty() { stream.read_exact(&mut resp_body).await.map_err(|e| format!("read body: {}", e))?; }

        Ok(MgmtResponse { status: resp_header.status, metadata, body: resp_body })
    }

    pub async fn block_request(
        &self, volume_id: &str, cmd: IscsiCommand, lba: u64, data: &[u8],
    ) -> Result<BlockResponse, String> {
        let mut guard = self.blk_conns.lock().await;
        let sock_path = format!("{}/blk-{}.sock", self.block_socket_dir, volume_id);

        if !guard.contains_key(volume_id) {
            let stream = UnixStream::connect(&sock_path).await
                .map_err(|e| format!("connect {}: {}", sock_path, e))?;
            guard.insert(volume_id.to_string(), stream);
        }

        let stream = guard.get_mut(volume_id).unwrap();
        let result = Self::do_block_request(stream, cmd, lba, data).await;
        if result.is_err() { guard.remove(volume_id); }
        result
    }

    async fn do_block_request(
        stream: &mut UnixStream, cmd: IscsiCommand, lba: u64, data: &[u8],
    ) -> Result<BlockResponse, String> {
        let header = IscsiRequestHeader::new(cmd, lba, data.len() as u32);
        stream.write_all(&header.to_bytes()).await.map_err(|e| format!("write header: {}", e))?;
        if !data.is_empty() { stream.write_all(data).await.map_err(|e| format!("write data: {}", e))?; }
        stream.flush().await.map_err(|e| format!("flush: {}", e))?;

        let mut resp_buf = [0u8; IscsiResponseHeader::SIZE];
        stream.read_exact(&mut resp_buf).await.map_err(|e| format!("read resp header: {}", e))?;
        let resp_header = IscsiResponseHeader::from_bytes(&resp_buf);
        if resp_header.magic != ISCSI_RESPONSE_MAGIC { return Err("invalid block response magic".into()); }

        let mut resp_data = vec![0u8; resp_header.length as usize];
        if !resp_data.is_empty() { stream.read_exact(&mut resp_data).await.map_err(|e| format!("read data: {}", e))?; }

        Ok(BlockResponse { status: resp_header.status, data: resp_data })
    }
}
```

- [ ] **Step 4: Create `main.rs`**

Create `apps/vmm-iscsi/src/main.rs`:

```rust
mod config;
mod socket;
pub mod pdu;
pub mod session;
pub mod scsi;
pub mod alua;
pub mod discovery;

use config::IscsiConfig;
use std::sync::Arc;

pub struct AppState {
    pub config: IscsiConfig,
    pub socket: socket::SocketPool,
}

#[tokio::main]
async fn main() {
    let config_path = std::env::args()
        .skip_while(|a| a != "--config")
        .nth(1)
        .unwrap_or_else(|| "/etc/vmm/iscsi.toml".to_string());

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = IscsiConfig::load(std::path::Path::new(&config_path))
        .unwrap_or_else(|e| {
            eprintln!("Config error: {}", e);
            std::process::exit(1);
        });

    tracing::info!("vmm-iscsi starting on {}", config.server.listen);

    let listen_addr = config.server.listen.clone();
    let socket_pool = socket::SocketPool::new(&config.san);
    let state = Arc::new(AppState { config, socket: socket_pool });

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Cannot bind {}: {}", listen_addr, e);
            std::process::exit(1);
        });

    tracing::info!("iSCSI target listening on {}", listen_addr);

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                tracing::debug!("iSCSI connection from {}", addr);
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(e) = session::handle_connection(stream, state).await {
                        tracing::debug!("iSCSI session ended: {}", e);
                    }
                });
            }
            Err(e) => {
                tracing::error!("Accept error: {}", e);
            }
        }
    }
}
```

- [ ] **Step 5: Add crate to workspace**

Check the workspace `Cargo.toml` at the repo root and add `"apps/vmm-iscsi"` to the `members` list.

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-iscsi/
git commit -m "feat(iscsi): scaffold vmm-iscsi service with config, socket pool, and main entry point"
```

---

## Task 7: iSCSI PDU Parser (`pdu.rs`)

**Files:**
- Create: `apps/vmm-iscsi/src/pdu.rs`

- [ ] **Step 1: Create `pdu.rs` with iSCSI PDU definitions and parsing**

Create `apps/vmm-iscsi/src/pdu.rs`:

```rust
//! iSCSI PDU (Protocol Data Unit) parsing and serialization.
//!
//! Implements RFC 3720 wire format for the minimal PDU set:
//! Login, Text, SCSI Command/Response, Data-In/Out, NOP, Logout.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// iSCSI opcodes (initiator to target)
pub const OPCODE_NOP_OUT: u8 = 0x00;
pub const OPCODE_SCSI_CMD: u8 = 0x01;
pub const OPCODE_LOGIN_REQ: u8 = 0x03;
pub const OPCODE_TEXT_REQ: u8 = 0x04;
pub const OPCODE_DATA_OUT: u8 = 0x05;
pub const OPCODE_LOGOUT_REQ: u8 = 0x06;

// iSCSI opcodes (target to initiator)
pub const OPCODE_NOP_IN: u8 = 0x20;
pub const OPCODE_SCSI_RESP: u8 = 0x21;
pub const OPCODE_LOGIN_RESP: u8 = 0x23;
pub const OPCODE_TEXT_RESP: u8 = 0x24;
pub const OPCODE_DATA_IN: u8 = 0x25;
pub const OPCODE_LOGOUT_RESP: u8 = 0x26;
pub const OPCODE_R2T: u8 = 0x31;

/// Basic Header Segment (BHS) — 48 bytes, common to all iSCSI PDUs.
#[derive(Debug, Clone)]
pub struct Bhs {
    pub opcode: u8,
    pub flags: u8,
    pub total_ahs_len: u8,
    pub data_segment_length: u32, // 24-bit in wire, stored as u32
    pub lun: u64,
    pub initiator_task_tag: u32,
    // Opcode-specific fields (bytes 20-47)
    pub specific: [u8; 28],
}

impl Bhs {
    pub const SIZE: usize = 48;

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        let opcode = buf[0] & 0x3F;
        let flags = buf[1];
        let total_ahs_len = buf[4];
        let data_segment_length =
            ((buf[5] as u32) << 16) | ((buf[6] as u32) << 8) | (buf[7] as u32);
        let lun = u64::from_be_bytes([buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]]);
        let initiator_task_tag = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
        let mut specific = [0u8; 28];
        specific.copy_from_slice(&buf[20..48]);

        Self { opcode, flags, total_ahs_len, data_segment_length, lun, initiator_task_tag, specific }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0] = self.opcode;
        buf[1] = self.flags;
        buf[4] = self.total_ahs_len;
        buf[5] = ((self.data_segment_length >> 16) & 0xFF) as u8;
        buf[6] = ((self.data_segment_length >> 8) & 0xFF) as u8;
        buf[7] = (self.data_segment_length & 0xFF) as u8;
        buf[8..16].copy_from_slice(&self.lun.to_be_bytes());
        buf[16..20].copy_from_slice(&self.initiator_task_tag.to_be_bytes());
        buf[20..48].copy_from_slice(&self.specific);
        buf
    }

    /// Is the Final bit set? (bit 7 of byte 1 for most PDUs)
    pub fn is_final(&self) -> bool { self.flags & 0x80 != 0 }
}

/// A complete iSCSI PDU: BHS + optional data segment.
#[derive(Debug, Clone)]
pub struct Pdu {
    pub bhs: Bhs,
    pub data: Vec<u8>,
}

impl Pdu {
    /// Read one PDU from TCP stream.
    pub async fn read_from(stream: &mut TcpStream) -> Result<Self, String> {
        let mut hdr_buf = [0u8; Bhs::SIZE];
        stream.read_exact(&mut hdr_buf).await.map_err(|e| format!("read BHS: {}", e))?;
        let bhs = Bhs::from_bytes(&hdr_buf);

        // Read data segment (padded to 4-byte boundary)
        let data_len = bhs.data_segment_length as usize;
        let padded_len = (data_len + 3) & !3;
        let mut data = vec![0u8; padded_len];
        if padded_len > 0 {
            stream.read_exact(&mut data).await.map_err(|e| format!("read data segment: {}", e))?;
        }
        data.truncate(data_len);

        Ok(Pdu { bhs, data })
    }

    /// Write one PDU to TCP stream.
    pub async fn write_to(&self, stream: &mut TcpStream) -> Result<(), String> {
        stream.write_all(&self.bhs.to_bytes()).await.map_err(|e| format!("write BHS: {}", e))?;

        if !self.data.is_empty() {
            stream.write_all(&self.data).await.map_err(|e| format!("write data: {}", e))?;
            // Pad to 4-byte boundary
            let pad = (4 - (self.data.len() % 4)) % 4;
            if pad > 0 {
                stream.write_all(&vec![0u8; pad]).await.map_err(|e| format!("write pad: {}", e))?;
            }
        }

        stream.flush().await.map_err(|e| format!("flush: {}", e))?;
        Ok(())
    }
}

// ── Helper constructors for common response PDUs ──────────────

/// Build a Login Response PDU.
pub fn login_response(
    initiator_task_tag: u32,
    status_class: u8,
    status_detail: u8,
    isid: &[u8; 6],
    tsih: u16,
    stat_sn: u32,
    exp_cmd_sn: u32,
    max_cmd_sn: u32,
    csg: u8,
    nsg: u8,
    transit: bool,
    data: Vec<u8>,
) -> Pdu {
    let mut bhs = Bhs {
        opcode: OPCODE_LOGIN_RESP,
        flags: (csg << 2) | nsg | if transit { 0x80 } else { 0 },
        total_ahs_len: 0,
        data_segment_length: data.len() as u32,
        lun: 0,
        initiator_task_tag,
        specific: [0u8; 28],
    };
    // ISID (bytes 8-13 of BHS, but we store target fields in specific)
    // StatusClass + StatusDetail at specific[24..26] relative offsets
    // StatSN at specific[4..8], ExpCmdSN at specific[8..12], MaxCmdSN at specific[12..16]
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());
    bhs.specific[16] = status_class;
    bhs.specific[17] = status_detail;

    Pdu { bhs, data }
}

/// Build a Text Response PDU.
pub fn text_response(
    initiator_task_tag: u32,
    stat_sn: u32,
    exp_cmd_sn: u32,
    max_cmd_sn: u32,
    data: Vec<u8>,
) -> Pdu {
    let mut bhs = Bhs {
        opcode: OPCODE_TEXT_RESP,
        flags: 0x80, // Final
        total_ahs_len: 0,
        data_segment_length: data.len() as u32,
        lun: 0,
        initiator_task_tag,
        specific: [0u8; 28],
    };
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());

    Pdu { bhs, data }
}

/// Build a SCSI Response PDU (no data).
pub fn scsi_response(
    initiator_task_tag: u32,
    stat_sn: u32,
    exp_cmd_sn: u32,
    max_cmd_sn: u32,
    scsi_status: u8,
) -> Pdu {
    let mut bhs = Bhs {
        opcode: OPCODE_SCSI_RESP,
        flags: 0x80, // Final
        total_ahs_len: 0,
        data_segment_length: 0,
        lun: 0,
        initiator_task_tag,
        specific: [0u8; 28],
    };
    // Response byte
    bhs.specific[0] = 0x00; // Command completed at target
    bhs.specific[1] = scsi_status;
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());

    Pdu { bhs, data: vec![] }
}

/// Build a Data-In PDU (target sending read data to initiator).
pub fn data_in(
    initiator_task_tag: u32,
    stat_sn: u32,
    data_sn: u32,
    buffer_offset: u32,
    data: Vec<u8>,
    is_final: bool,
    scsi_status: Option<u8>,
) -> Pdu {
    let mut flags: u8 = 0;
    if is_final { flags |= 0x80; } // F bit
    if let Some(status) = scsi_status { flags |= 0x01; } // S bit (status included)

    let mut bhs = Bhs {
        opcode: OPCODE_DATA_IN,
        flags,
        total_ahs_len: 0,
        data_segment_length: data.len() as u32,
        lun: 0,
        initiator_task_tag,
        specific: [0u8; 28],
    };
    if let Some(status) = scsi_status {
        bhs.specific[1] = status;
    }
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[16..20].copy_from_slice(&data_sn.to_be_bytes());
    bhs.specific[20..24].copy_from_slice(&buffer_offset.to_be_bytes());

    Pdu { bhs, data }
}

/// Build a NOP-In PDU (response to NOP-Out / keepalive).
pub fn nop_in(
    initiator_task_tag: u32,
    target_transfer_tag: u32,
    stat_sn: u32,
    exp_cmd_sn: u32,
    max_cmd_sn: u32,
) -> Pdu {
    let mut bhs = Bhs {
        opcode: OPCODE_NOP_IN,
        flags: 0x80,
        total_ahs_len: 0,
        data_segment_length: 0,
        lun: 0,
        initiator_task_tag,
        specific: [0u8; 28],
    };
    bhs.specific[0..4].copy_from_slice(&target_transfer_tag.to_be_bytes());
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());

    Pdu { bhs, data: vec![] }
}

/// Build a Logout Response PDU.
pub fn logout_response(
    initiator_task_tag: u32,
    stat_sn: u32,
    exp_cmd_sn: u32,
    max_cmd_sn: u32,
) -> Pdu {
    let mut bhs = Bhs {
        opcode: OPCODE_LOGOUT_RESP,
        flags: 0x80,
        total_ahs_len: 0,
        data_segment_length: 0,
        lun: 0,
        initiator_task_tag,
        specific: [0u8; 28],
    };
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());

    Pdu { bhs, data: vec![] }
}

/// Parse iSCSI key=value text parameters from data segment.
pub fn parse_text_params(data: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(data);
    text.split('\0')
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let mut parts = s.splitn(2, '=');
            let key = parts.next()?.to_string();
            let val = parts.next()?.to_string();
            Some((key, val))
        })
        .collect()
}

/// Serialize key=value parameters to iSCSI text format (null-separated).
pub fn encode_text_params(params: &[(&str, &str)]) -> Vec<u8> {
    let mut data = Vec::new();
    for (k, v) in params {
        data.extend_from_slice(format!("{}={}", k, v).as_bytes());
        data.push(0);
    }
    data
}
```

- [ ] **Step 2: Commit**

```bash
git add apps/vmm-iscsi/src/pdu.rs
git commit -m "feat(iscsi): implement iSCSI PDU parser and serializer (RFC 3720)"
```

---

## Task 8: iSCSI Session State Machine (`session.rs`, `discovery.rs`)

**Files:**
- Create: `apps/vmm-iscsi/src/session.rs`
- Create: `apps/vmm-iscsi/src/discovery.rs`

- [ ] **Step 1: Create `discovery.rs`**

Create `apps/vmm-iscsi/src/discovery.rs`:

```rust
//! SendTargets discovery — queries mgmt.sock for iSCSI-enabled volumes
//! and builds the target list with portal addresses.

use crate::socket::SocketPool;
use vmm_core::san_mgmt::MgmtCommand;

/// Query vmm-san for all iSCSI-enabled volumes and build SendTargets response text.
pub async fn build_send_targets(socket: &SocketPool, node_name: &str) -> Result<Vec<u8>, String> {
    let resp = socket.mgmt_request(MgmtCommand::ListIscsiVolumes, &[], &[]).await?;
    if !resp.is_ok() {
        return Err("ListIscsiVolumes failed".into());
    }

    let volumes: Vec<serde_json::Value> = serde_json::from_slice(&resp.body).unwrap_or_default();

    let mut params: Vec<(&str, String)> = Vec::new();
    // We need owned strings but encode_text_params takes &str, so build manually
    let mut data = Vec::new();
    for vol in &volumes {
        let name = vol["name"].as_str().unwrap_or("unknown");
        let iqn = format!("{}:{}", node_name, name);
        // TargetName=iqn.2026-04.io.corevm:vol-name
        data.extend_from_slice(format!("TargetName={}", iqn).as_bytes());
        data.push(0);
        // TargetAddress=<ip>:3260,1 (portal group tag = 1)
        // The actual address will be filled by the caller based on connection info
        // For now, we report 0.0.0.0:3260 and let the initiator use the discovery address
        data.extend_from_slice(b"TargetAddress=0.0.0.0:3260,1");
        data.push(0);
    }

    Ok(data)
}
```

- [ ] **Step 2: Create `session.rs`**

Create `apps/vmm-iscsi/src/session.rs`:

```rust
//! iSCSI session state machine.
//!
//! Handles login phase (discovery or normal), parameter negotiation,
//! and dispatches to SCSI command handler in full-feature phase.

use std::sync::Arc;
use tokio::net::TcpStream;
use vmm_core::san_mgmt::MgmtCommand;
use crate::AppState;
use crate::pdu::*;

/// Session parameters negotiated during login.
struct SessionParams {
    max_recv_data_segment_length: u32,
    max_burst_length: u32,
    first_burst_length: u32,
    initial_r2t: bool,
    immediate_data: bool,
}

impl Default for SessionParams {
    fn default() -> Self {
        Self {
            max_recv_data_segment_length: 65536,
            max_burst_length: 262144,
            first_burst_length: 65536,
            initial_r2t: true,
            immediate_data: true,
        }
    }
}

/// Per-session state.
struct Session {
    initiator_name: String,
    target_name: String,
    volume_id: String,
    is_discovery: bool,
    params: SessionParams,
    cmd_sn: u32,
    exp_cmd_sn: u32,
    max_cmd_sn: u32,
    stat_sn: u32,
}

/// Entry point for a new TCP connection.
pub async fn handle_connection(mut stream: TcpStream, state: Arc<AppState>) -> Result<(), String> {
    // Read the first PDU — must be a Login Request
    let pdu = Pdu::read_from(&mut stream).await?;

    if pdu.bhs.opcode != OPCODE_LOGIN_REQ {
        return Err(format!("expected Login Request, got opcode 0x{:02X}", pdu.bhs.opcode));
    }

    // Parse login parameters
    let text_params = parse_text_params(&pdu.data);
    let initiator_name = text_params.iter()
        .find(|(k, _)| k == "InitiatorName")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    let target_name = text_params.iter()
        .find(|(k, _)| k == "TargetName")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    let session_type = text_params.iter()
        .find(|(k, _)| k == "SessionType")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "Normal".to_string());

    let is_discovery = session_type == "Discovery";

    tracing::info!("iSCSI login: initiator={} target={} type={}", initiator_name, target_name, session_type);

    let mut session = Session {
        initiator_name: initiator_name.clone(),
        target_name: target_name.clone(),
        volume_id: String::new(),
        is_discovery,
        params: SessionParams::default(),
        cmd_sn: 1,
        exp_cmd_sn: pdu.bhs.specific[8..12].try_into().map(u32::from_be_bytes).unwrap_or(1),
        max_cmd_sn: 1,
        stat_sn: 1,
    };

    if !is_discovery {
        // Resolve target name to volume ID
        let vol_name = target_name.rsplit(':').next().unwrap_or("");
        let resolve_resp = state.socket.mgmt_request(
            MgmtCommand::ResolveVolume, vol_name.as_bytes(), &[],
        ).await?;

        if !resolve_resp.is_ok() {
            // Send login reject
            let resp = login_response(
                pdu.bhs.initiator_task_tag, 0x02, 0x00, // class=initiator error
                &[0; 6], 0, session.stat_sn, session.exp_cmd_sn, session.max_cmd_sn,
                0, 0, false, vec![],
            );
            resp.write_to(&mut stream).await?;
            return Err(format!("volume '{}' not found", vol_name));
        }

        let meta = resolve_resp.body_json();
        session.volume_id = meta["id"].as_str().unwrap_or("").to_string();

        // Check ACL
        let acl_resp = state.socket.mgmt_request(
            MgmtCommand::ListIscsiAcls, session.volume_id.as_bytes(), &[],
        ).await?;

        if acl_resp.is_ok() {
            let acls: Vec<serde_json::Value> = serde_json::from_slice(&acl_resp.body).unwrap_or_default();
            let allowed = acls.iter().any(|a| {
                a["initiator_iqn"].as_str() == Some(&initiator_name)
            });
            if !allowed && !acls.is_empty() {
                let resp = login_response(
                    pdu.bhs.initiator_task_tag, 0x02, 0x01, // class=initiator, detail=not authorized
                    &[0; 6], 0, session.stat_sn, session.exp_cmd_sn, session.max_cmd_sn,
                    0, 0, false, vec![],
                );
                resp.write_to(&mut stream).await?;
                return Err(format!("initiator '{}' not in ACL for volume '{}'", initiator_name, vol_name));
            }
        }
    }

    // Negotiate parameters from initiator's login data
    let mut response_params: Vec<(&str, &str)> = vec![
        ("HeaderDigest", "None"),
        ("DataDigest", "None"),
        ("MaxRecvDataSegmentLength", "65536"),
        ("MaxBurstLength", "262144"),
        ("FirstBurstLength", "65536"),
        ("DefaultTime2Wait", "2"),
        ("DefaultTime2Retain", "20"),
        ("MaxOutstandingR2T", "1"),
        ("InitialR2T", "Yes"),
        ("ImmediateData", "Yes"),
        ("MaxConnections", "1"),
        ("ErrorRecoveryLevel", "0"),
    ];

    if is_discovery {
        response_params.push(("TargetAlias", "CoreSAN"));
    }

    let resp_data = encode_text_params(&response_params);

    // Send Login Response (success)
    let resp = login_response(
        pdu.bhs.initiator_task_tag, 0x00, 0x00, // success
        &[0; 6], 1, // TSIH=1
        session.stat_sn, session.exp_cmd_sn, session.max_cmd_sn,
        1, 3, true, // CSG=operational, NSG=full-feature, transit=true
        resp_data,
    );
    resp.write_to(&mut stream).await?;
    session.stat_sn += 1;

    tracing::info!("iSCSI login successful: initiator={} volume={}", initiator_name, session.volume_id);

    // Full-feature phase loop
    loop {
        let pdu = match Pdu::read_from(&mut stream).await {
            Ok(p) => p,
            Err(_) => break,
        };

        match pdu.bhs.opcode {
            OPCODE_SCSI_CMD => {
                crate::scsi::handle_scsi_command(&pdu, &mut session.stat_sn,
                    session.exp_cmd_sn, session.max_cmd_sn,
                    &session.volume_id, &state, &mut stream).await?;
            }
            OPCODE_TEXT_REQ => {
                // SendTargets in full-feature phase (or discovery session)
                let targets_data = crate::discovery::build_send_targets(
                    &state.socket, &state.config.server.node_name,
                ).await.unwrap_or_default();
                let resp = text_response(
                    pdu.bhs.initiator_task_tag,
                    session.stat_sn, session.exp_cmd_sn, session.max_cmd_sn,
                    targets_data,
                );
                resp.write_to(&mut stream).await?;
                session.stat_sn += 1;
            }
            OPCODE_NOP_OUT => {
                let resp = nop_in(
                    pdu.bhs.initiator_task_tag, 0xFFFFFFFF,
                    session.stat_sn, session.exp_cmd_sn, session.max_cmd_sn,
                );
                resp.write_to(&mut stream).await?;
                session.stat_sn += 1;
            }
            OPCODE_LOGOUT_REQ => {
                let resp = logout_response(
                    pdu.bhs.initiator_task_tag,
                    session.stat_sn, session.exp_cmd_sn, session.max_cmd_sn,
                );
                resp.write_to(&mut stream).await?;
                tracing::info!("iSCSI logout: initiator={}", session.initiator_name);
                break;
            }
            other => {
                tracing::warn!("Unhandled iSCSI opcode: 0x{:02X}", other);
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Commit**

```bash
git add apps/vmm-iscsi/src/session.rs apps/vmm-iscsi/src/discovery.rs
git commit -m "feat(iscsi): implement session state machine with login, discovery, and full-feature dispatch"
```

---

## Task 9: SCSI Command Handler (`scsi.rs`) + ALUA (`alua.rs`)

**Files:**
- Create: `apps/vmm-iscsi/src/scsi.rs`
- Create: `apps/vmm-iscsi/src/alua.rs`

- [ ] **Step 1: Create `alua.rs`**

Create `apps/vmm-iscsi/src/alua.rs`:

```rust
//! ALUA (Asymmetric Logical Unit Access) support.
//!
//! Builds REPORT TARGET PORT GROUPS response data for multipath-aware initiators.

use crate::socket::SocketPool;
use vmm_core::san_mgmt::MgmtCommand;

// ALUA states (SPC-4)
pub const ALUA_ACTIVE_OPTIMIZED: u8 = 0x00;
pub const ALUA_ACTIVE_NON_OPTIMIZED: u8 = 0x01;
pub const ALUA_STANDBY: u8 = 0x02;
pub const ALUA_UNAVAILABLE: u8 = 0x03;

/// Build REPORT TARGET PORT GROUPS response data (SPC-4).
pub async fn report_target_port_groups(socket: &SocketPool, volume_id: &str) -> Result<Vec<u8>, String> {
    let resp = socket.mgmt_request(MgmtCommand::GetTargetPortGroups, volume_id.as_bytes(), &[]).await?;
    if !resp.is_ok() {
        return Err("GetTargetPortGroups failed".into());
    }

    let tpgs: Vec<serde_json::Value> = serde_json::from_slice(&resp.body).unwrap_or_default();

    // Build parameter data per SPC-4 section 7.6.3.5
    let mut data = Vec::new();
    // Return data length placeholder (4 bytes, filled at end)
    data.extend_from_slice(&[0u8; 4]);

    for (i, tpg) in tpgs.iter().enumerate() {
        let state_str = tpg["state"].as_str().unwrap_or("active_non_optimized");
        let alua_state = match state_str {
            "active_optimized" => ALUA_ACTIVE_OPTIMIZED,
            "active_non_optimized" => ALUA_ACTIVE_NON_OPTIMIZED,
            "standby" => ALUA_STANDBY,
            _ => ALUA_UNAVAILABLE,
        };

        let tpg_id = (i + 1) as u16;

        // Target port group descriptor (8 bytes min)
        data.push(alua_state); // byte 0: asymmetric access state
        data.push(0x8F); // byte 1: supported states (A/O, A/NO, Standby, Unavail + explicit transition)
        data.extend_from_slice(&tpg_id.to_be_bytes()); // bytes 2-3: target port group
        data.push(0); // byte 4: reserved
        data.push(0); // byte 5: status code
        data.push(0); // byte 6: vendor specific
        data.push(1); // byte 7: target port count

        // Target port descriptor (4 bytes)
        data.extend_from_slice(&[0, 0]); // relative target port identifier (high)
        let port_id = (i + 1) as u16;
        data.extend_from_slice(&port_id.to_be_bytes()); // relative target port identifier (low)
    }

    // Fill in return data length (total - 4 bytes for the length field itself)
    let len = (data.len() - 4) as u32;
    data[0..4].copy_from_slice(&len.to_be_bytes());

    Ok(data)
}

/// Get ALUA state for this node / volume.
pub async fn get_local_alua_state(socket: &SocketPool, volume_id: &str) -> Result<u8, String> {
    let resp = socket.mgmt_request(MgmtCommand::GetAluaState, volume_id.as_bytes(), &[]).await?;
    if !resp.is_ok() {
        return Ok(ALUA_UNAVAILABLE);
    }
    let json: serde_json::Value = serde_json::from_slice(&resp.body).unwrap_or_default();
    let state = json["state"].as_str().unwrap_or("unavailable");
    Ok(match state {
        "active_optimized" => ALUA_ACTIVE_OPTIMIZED,
        "active_non_optimized" => ALUA_ACTIVE_NON_OPTIMIZED,
        "standby" => ALUA_STANDBY,
        _ => ALUA_UNAVAILABLE,
    })
}
```

- [ ] **Step 2: Create `scsi.rs`**

Create `apps/vmm-iscsi/src/scsi.rs`:

```rust
//! SCSI command handler — minimal SBC set for block storage.
//!
//! Translates SCSI CDBs from iSCSI PDUs into block I/O via CoreSAN sockets.

use std::sync::Arc;
use tokio::net::TcpStream;
use vmm_core::san_iscsi::IscsiCommand;
use crate::AppState;
use crate::pdu::*;

// SCSI status codes
const SCSI_STATUS_GOOD: u8 = 0x00;
const SCSI_STATUS_CHECK_CONDITION: u8 = 0x02;

// SCSI opcodes
const OP_TEST_UNIT_READY: u8 = 0x00;
const OP_INQUIRY: u8 = 0x12;
const OP_MODE_SENSE_6: u8 = 0x1A;
const OP_READ_CAPACITY_10: u8 = 0x25;
const OP_READ_10: u8 = 0x28;
const OP_WRITE_10: u8 = 0x2A;
const OP_MODE_SENSE_10: u8 = 0x5A;
const OP_READ_16: u8 = 0x88;
const OP_WRITE_16: u8 = 0x8A;
const OP_SERVICE_ACTION_IN: u8 = 0x9E; // READ CAPACITY(16) = SA 0x10
const OP_REPORT_LUNS: u8 = 0xA0;
const OP_MAINTENANCE_IN: u8 = 0xA3; // REPORT TARGET PORT GROUPS = SA 0x0A

/// Handle a SCSI command PDU. Reads CDB from PDU, dispatches, sends response.
pub async fn handle_scsi_command(
    pdu: &Pdu,
    stat_sn: &mut u32,
    exp_cmd_sn: u32,
    max_cmd_sn: u32,
    volume_id: &str,
    state: &Arc<AppState>,
    stream: &mut TcpStream,
) -> Result<(), String> {
    // CDB is in BHS specific bytes (bytes 32-47 of BHS = specific[12..28])
    let cdb = &pdu.bhs.specific[12..28];
    let opcode = cdb[0];
    let tag = pdu.bhs.initiator_task_tag;

    match opcode {
        OP_TEST_UNIT_READY => {
            let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_GOOD);
            resp.write_to(stream).await?;
            *stat_sn += 1;
        }

        OP_INQUIRY => {
            let evpd = cdb[1] & 0x01;
            let page_code = cdb[2];
            let alloc_len = u16::from_be_bytes([cdb[3], cdb[4]]) as usize;

            let data = if evpd == 0 {
                // Standard inquiry
                build_standard_inquiry()
            } else {
                match page_code {
                    0x00 => build_vpd_supported_pages(),
                    0x83 => build_vpd_device_identification(volume_id, state),
                    0x80 => build_vpd_unit_serial(volume_id),
                    _ => build_sense_illegal_request(),
                }
            };

            let send_len = data.len().min(alloc_len);
            let resp = data_in(tag, *stat_sn, 0, 0, data[..send_len].to_vec(), true, Some(SCSI_STATUS_GOOD));
            resp.write_to(stream).await?;
            *stat_sn += 1;
        }

        OP_READ_CAPACITY_10 => {
            let cap = get_capacity(volume_id, state).await;
            let blocks = (cap.0 / cap.1) - 1; // last LBA
            let block_size = cap.1 as u32;
            let mut data = vec![0u8; 8];
            // If > 2TB, return 0xFFFFFFFF to signal use READ CAPACITY(16)
            let last_lba = if blocks > 0xFFFFFFFF { 0xFFFFFFFF_u32 } else { blocks as u32 };
            data[0..4].copy_from_slice(&last_lba.to_be_bytes());
            data[4..8].copy_from_slice(&block_size.to_be_bytes());

            let resp = data_in(tag, *stat_sn, 0, 0, data, true, Some(SCSI_STATUS_GOOD));
            resp.write_to(stream).await?;
            *stat_sn += 1;
        }

        OP_SERVICE_ACTION_IN => {
            let sa = cdb[1] & 0x1F;
            if sa == 0x10 {
                // READ CAPACITY(16)
                let cap = get_capacity(volume_id, state).await;
                let last_lba = (cap.0 / cap.1) - 1;
                let block_size = cap.1 as u32;
                let mut data = vec![0u8; 32];
                data[0..8].copy_from_slice(&last_lba.to_be_bytes());
                data[8..12].copy_from_slice(&block_size.to_be_bytes());

                let alloc_len = u32::from_be_bytes([cdb[10], cdb[11], cdb[12], cdb[13]]) as usize;
                let send_len = data.len().min(alloc_len);
                let resp = data_in(tag, *stat_sn, 0, 0, data[..send_len].to_vec(), true, Some(SCSI_STATUS_GOOD));
                resp.write_to(stream).await?;
                *stat_sn += 1;
            } else {
                let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION);
                resp.write_to(stream).await?;
                *stat_sn += 1;
            }
        }

        OP_MODE_SENSE_6 | OP_MODE_SENSE_10 => {
            // Return minimal mode parameter header (no pages)
            let data = if opcode == OP_MODE_SENSE_6 {
                vec![3, 0, 0, 0] // 4-byte header: mode data length=3, medium type=0, device specific=0, block descriptor length=0
            } else {
                vec![0, 6, 0, 0, 0, 0, 0, 0] // 8-byte header
            };
            let resp = data_in(tag, *stat_sn, 0, 0, data, true, Some(SCSI_STATUS_GOOD));
            resp.write_to(stream).await?;
            *stat_sn += 1;
        }

        OP_REPORT_LUNS => {
            // Always report LUN 0
            let mut data = vec![0u8; 16];
            data[0..4].copy_from_slice(&8u32.to_be_bytes()); // LUN list length = 8
            // LUN 0 at bytes 8-15 (all zeros = LUN 0)
            let resp = data_in(tag, *stat_sn, 0, 0, data, true, Some(SCSI_STATUS_GOOD));
            resp.write_to(stream).await?;
            *stat_sn += 1;
        }

        OP_READ_10 => {
            let lba = u32::from_be_bytes([cdb[2], cdb[3], cdb[4], cdb[5]]) as u64;
            let transfer_length = u16::from_be_bytes([cdb[7], cdb[8]]) as u32;
            let byte_count = transfer_length * 512;

            let result = state.socket.block_request(volume_id, IscsiCommand::ReadBlocks, lba, &[]).await;
            // We need to pass the length — encode it in the data field as a length hint
            // Actually: the block socket uses header.length for read size. We pass empty data but set lba.
            // Re-read: IscsiRequestHeader has lba and length fields. block_request sends data.len() as length.
            // For reads, we need to send the byte_count differently. Let's use a direct approach:
            let read_data = read_via_socket(state, volume_id, lba, byte_count).await;

            match read_data {
                Ok(data) => {
                    let resp = data_in(tag, *stat_sn, 0, 0, data, true, Some(SCSI_STATUS_GOOD));
                    resp.write_to(stream).await?;
                }
                Err(_) => {
                    let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION);
                    resp.write_to(stream).await?;
                }
            }
            *stat_sn += 1;
        }

        OP_READ_16 => {
            let lba = u64::from_be_bytes([cdb[2], cdb[3], cdb[4], cdb[5], cdb[6], cdb[7], cdb[8], cdb[9]]);
            let transfer_length = u32::from_be_bytes([cdb[10], cdb[11], cdb[12], cdb[13]]);
            let byte_count = transfer_length * 512;

            match read_via_socket(state, volume_id, lba, byte_count).await {
                Ok(data) => {
                    let resp = data_in(tag, *stat_sn, 0, 0, data, true, Some(SCSI_STATUS_GOOD));
                    resp.write_to(stream).await?;
                }
                Err(_) => {
                    let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION);
                    resp.write_to(stream).await?;
                }
            }
            *stat_sn += 1;
        }

        OP_WRITE_10 => {
            let lba = u32::from_be_bytes([cdb[2], cdb[3], cdb[4], cdb[5]]) as u64;
            let transfer_length = u16::from_be_bytes([cdb[7], cdb[8]]) as u32;
            let byte_count = transfer_length * 512;

            // Write data comes in the PDU data segment (immediate data)
            // or via Data-Out PDUs. For simplicity with ImmediateData=Yes:
            let write_data = if pdu.data.len() >= byte_count as usize {
                pdu.data[..byte_count as usize].to_vec()
            } else {
                // Need to read additional Data-Out PDUs
                let mut buf = pdu.data.clone();
                while buf.len() < byte_count as usize {
                    let data_pdu = Pdu::read_from(stream).await?;
                    if data_pdu.bhs.opcode != OPCODE_DATA_OUT { break; }
                    buf.extend_from_slice(&data_pdu.data);
                }
                buf
            };

            match state.socket.block_request(volume_id, IscsiCommand::WriteBlocks, lba, &write_data).await {
                Ok(resp) if resp.is_ok() => {
                    let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_GOOD);
                    resp.write_to(stream).await?;
                }
                _ => {
                    let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION);
                    resp.write_to(stream).await?;
                }
            }
            *stat_sn += 1;
        }

        OP_WRITE_16 => {
            let lba = u64::from_be_bytes([cdb[2], cdb[3], cdb[4], cdb[5], cdb[6], cdb[7], cdb[8], cdb[9]]);
            let transfer_length = u32::from_be_bytes([cdb[10], cdb[11], cdb[12], cdb[13]]);
            let byte_count = transfer_length * 512;

            let write_data = if pdu.data.len() >= byte_count as usize {
                pdu.data[..byte_count as usize].to_vec()
            } else {
                let mut buf = pdu.data.clone();
                while buf.len() < byte_count as usize {
                    let data_pdu = Pdu::read_from(stream).await?;
                    if data_pdu.bhs.opcode != OPCODE_DATA_OUT { break; }
                    buf.extend_from_slice(&data_pdu.data);
                }
                buf
            };

            match state.socket.block_request(volume_id, IscsiCommand::WriteBlocks, lba, &write_data).await {
                Ok(resp) if resp.is_ok() => {
                    let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_GOOD);
                    resp.write_to(stream).await?;
                }
                _ => {
                    let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION);
                    resp.write_to(stream).await?;
                }
            }
            *stat_sn += 1;
        }

        OP_MAINTENANCE_IN => {
            let sa = cdb[1] & 0x1F;
            if sa == 0x0A {
                // REPORT TARGET PORT GROUPS
                let data = crate::alua::report_target_port_groups(&state.socket, volume_id)
                    .await.unwrap_or_default();
                let alloc_len = u32::from_be_bytes([cdb[6], cdb[7], cdb[8], cdb[9]]) as usize;
                let send_len = data.len().min(alloc_len);
                let resp = data_in(tag, *stat_sn, 0, 0, data[..send_len].to_vec(), true, Some(SCSI_STATUS_GOOD));
                resp.write_to(stream).await?;
                *stat_sn += 1;
            } else {
                let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION);
                resp.write_to(stream).await?;
                *stat_sn += 1;
            }
        }

        _ => {
            tracing::warn!("Unsupported SCSI opcode: 0x{:02X}", opcode);
            let resp = scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION);
            resp.write_to(stream).await?;
            *stat_sn += 1;
        }
    }

    Ok(())
}

// ── Helper functions ──────────────────────────────────────────

async fn get_capacity(volume_id: &str, state: &Arc<AppState>) -> (u64, u64) {
    match state.socket.block_request(volume_id, IscsiCommand::GetCapacity, 0, &[]).await {
        Ok(resp) if resp.is_ok() => {
            let json: serde_json::Value = serde_json::from_slice(&resp.data).unwrap_or_default();
            let size = json["size_bytes"].as_u64().unwrap_or(0);
            let block_size = json["block_size"].as_u64().unwrap_or(512);
            (size, block_size)
        }
        _ => (0, 512),
    }
}

async fn read_via_socket(state: &Arc<AppState>, volume_id: &str, lba: u64, byte_count: u32) -> Result<Vec<u8>, String> {
    // For reads, we send a "read" request with the length encoded
    // The block socket handler uses header.length to know how much to read
    // We pass a dummy buffer of the right length to trigger the correct header.length
    let placeholder = vec![0u8; byte_count as usize];
    let resp = state.socket.block_request(volume_id, IscsiCommand::ReadBlocks, lba, &placeholder).await?;
    if resp.is_ok() { Ok(resp.data) } else { Err("read failed".into()) }
}

fn build_standard_inquiry() -> Vec<u8> {
    let mut data = vec![0u8; 96];
    data[0] = 0x00; // Peripheral device type: SBC (block device)
    data[1] = 0x00; // Not removable
    data[2] = 0x06; // SPC-4 version
    data[3] = 0x02; // Response data format = 2
    data[4] = 91;   // Additional length
    data[5] = 0x10; // TPGS=01 (implicit ALUA)
    // Vendor (8 bytes, ASCII padded)
    data[8..16].copy_from_slice(b"CoreVM  ");
    // Product (16 bytes, ASCII padded)
    data[16..32].copy_from_slice(b"CoreSAN         ");
    // Revision (4 bytes)
    data[32..36].copy_from_slice(b"0001");
    data
}

fn build_vpd_supported_pages() -> Vec<u8> {
    vec![
        0x00, // Peripheral qualifier + device type
        0x00, // Page code: Supported VPD pages
        0x00, 0x03, // Page length
        0x00, // Supported pages: 0x00
        0x80, // 0x80 (Unit Serial Number)
        0x83, // 0x83 (Device Identification)
    ]
}

fn build_vpd_device_identification(volume_id: &str, state: &Arc<AppState>) -> Vec<u8> {
    // NAA identifier based on volume UUID — same across all nodes for multipath
    let naa_id = format!("naa.6001405{}", &volume_id.replace('-', "")[..25]);
    let naa_bytes = naa_id.as_bytes();

    let mut data = vec![0u8; 4]; // header
    data[0] = 0x00; // Peripheral qualifier + device type
    data[1] = 0x83; // Page code
    // Designation descriptor 1: NAA identifier (for multipath device correlation)
    let mut desc1 = vec![
        0x01, // Protocol identifier: iSCSI + Code set: Binary
        0x03, // PIV=0, Association=0 (LUN), Designator type=3 (NAA)
        0x00, // Reserved
        naa_bytes.len() as u8,
    ];
    desc1.extend_from_slice(naa_bytes);
    data.extend_from_slice(&desc1);

    // Designation descriptor 2: Target port group
    let tpg_id: u16 = 1; // TODO: derive from node index
    let mut desc2 = vec![
        0x01, 0x05, // Code set: binary, Designator type: target port group
        0x00, 0x04, // Length = 4
    ];
    desc2.extend_from_slice(&[0, 0]); // reserved
    desc2.extend_from_slice(&tpg_id.to_be_bytes());
    data.extend_from_slice(&desc2);

    // Fill page length
    let page_len = (data.len() - 4) as u16;
    data[2..4].copy_from_slice(&page_len.to_be_bytes());

    data
}

fn build_vpd_unit_serial(volume_id: &str) -> Vec<u8> {
    let serial = volume_id.as_bytes();
    let mut data = vec![0u8; 4];
    data[0] = 0x00;
    data[1] = 0x80; // Page code
    data[2] = 0;
    data[3] = serial.len() as u8;
    data.extend_from_slice(serial);
    data
}

fn build_sense_illegal_request() -> Vec<u8> {
    // Fixed format sense data: ILLEGAL REQUEST
    vec![0x70, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x0A,
         0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00]
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/cmoeller/Development/corevm && cargo check -p vmm-iscsi`
Expected: compiles without errors

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-iscsi/src/scsi.rs apps/vmm-iscsi/src/alua.rs
git commit -m "feat(iscsi): implement SCSI command handler (SBC minimal set) and ALUA support"
```

---

## Task 10: Frontend — TypeScript Types + StorageBlockStorage Page

**Files:**
- Modify: `apps/vmm-ui/src/api/types.ts:527`
- Create: `apps/vmm-ui/src/pages/StorageBlockStorage.tsx`

- [ ] **Step 1: Add iSCSI types to `types.ts`**

In `apps/vmm-ui/src/api/types.ts`, add after line 527 (after `S3CredentialCreateResponse`):

```typescript

// ── iSCSI Block Storage ──────────────────────────────────────────────

export interface IscsiAcl {
  id: string
  volume_id: string
  volume_name: string
  initiator_iqn: string
  comment: string
  created_at: string
}

export interface IscsiTarget {
  volume_id: string
  volume_name: string
  iqn: string
  portals: string[]
  alua_state: string
  status: string
}
```

- [ ] **Step 2: Create `StorageBlockStorage.tsx`**

Create `apps/vmm-ui/src/pages/StorageBlockStorage.tsx`:

```tsx
import { useState, useEffect } from 'react'
import { Trash2, Plus } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import api from '../api/client'
import type { CoreSanVolume, IscsiAcl, IscsiTarget } from '../api/types'
import { formatBytes } from '../utils/format'
import Button from '../components/Button'
import Card from '../components/Card'
import CreateIscsiAclDialog from '../components/coresan/CreateIscsiAclDialog'
import ConfirmDialog from '../components/ConfirmDialog'

export default function StorageBlockStorage() {
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'

  const [volumes, setVolumes] = useState<CoreSanVolume[]>([])
  const [acls, setAcls] = useState<IscsiAcl[]>([])
  const [targets, setTargets] = useState<IscsiTarget[]>([])
  const [tab, setTab] = useState<'volumes' | 'acls' | 'connect'>('volumes')
  const [showCreateAcl, setShowCreateAcl] = useState(false)
  const [deleteAclId, setDeleteAclId] = useState<string | null>(null)
  const [error, setError] = useState('')

  const sanBase = 'http://localhost:7443'

  const fetchData = async () => {
    try {
      let vols: CoreSanVolume[] = []
      try {
        if (isCluster) {
          const { data } = await api.get<CoreSanVolume[]>('/api/san/volumes')
          vols = Array.isArray(data) ? data : []
        } else {
          const resp = await fetch(`${sanBase}/api/volumes`)
          if (resp.ok) { const d = await resp.json(); vols = Array.isArray(d) ? d : [] }
        }
      } catch {}
      setVolumes(vols.filter(v => v.access_protocols?.includes('iscsi')))

      let aclList: IscsiAcl[] = []
      try {
        if (isCluster) {
          const { data } = await api.get<IscsiAcl[]>('/api/san/iscsi/acls')
          aclList = Array.isArray(data) ? data : []
        } else {
          const resp = await fetch(`${sanBase}/api/iscsi/acls`)
          if (resp.ok) { const d = await resp.json(); aclList = Array.isArray(d) ? d : [] }
        }
      } catch {}
      setAcls(aclList)

      let tgtList: IscsiTarget[] = []
      try {
        if (isCluster) {
          const { data } = await api.get<IscsiTarget[]>('/api/san/iscsi/targets')
          tgtList = Array.isArray(data) ? data : []
        } else {
          const resp = await fetch(`${sanBase}/api/iscsi/targets`)
          if (resp.ok) { const d = await resp.json(); tgtList = Array.isArray(d) ? d : [] }
        }
      } catch {}
      setTargets(tgtList)
    } catch (e: any) {
      setError(e.message || 'Failed to load data')
    }
  }

  useEffect(() => { fetchData() }, [isCluster])

  const handleDeleteAcl = async () => {
    if (!deleteAclId) return
    try {
      if (isCluster) {
        await api.delete(`/api/san/iscsi/acls/${deleteAclId}`)
      } else {
        await fetch(`${sanBase}/api/iscsi/acls/${deleteAclId}`, { method: 'DELETE' })
      }
      setDeleteAclId(null)
      fetchData()
    } catch (e: any) {
      setError(e.message || 'Failed to delete ACL')
    }
  }

  const aluaLabel = (state: string) => {
    switch (state) {
      case 'active_optimized': return 'Active/Optimized'
      case 'active_non_optimized': return 'Active/Non-Optimized'
      case 'standby': return 'Standby'
      default: return state
    }
  }

  const aluaColor = (state: string) => {
    switch (state) {
      case 'active_optimized': return 'bg-green-500/10 text-green-400'
      case 'active_non_optimized': return 'bg-yellow-500/10 text-yellow-400'
      case 'standby': return 'bg-blue-500/10 text-blue-400'
      default: return 'bg-red-500/10 text-red-400'
    }
  }

  const tabs = [
    { key: 'volumes' as const, label: 'iSCSI Volumes' },
    { key: 'acls' as const, label: 'Access Control (ACLs)' },
    { key: 'connect' as const, label: 'Connection Info' },
  ]

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-vmm-text">Block Storage</h1>
      </div>

      {error && (
        <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{error}</div>
      )}

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

      {tab === 'volumes' && (
        <Card>
          <div className="p-4">
            <p className="text-sm text-vmm-muted mb-4">
              Volumes with iSCSI access protocol enabled. Manage volumes in the CoreSAN page.
            </p>
            {volumes.length === 0 ? (
              <p className="text-vmm-muted text-sm py-8 text-center">
                No iSCSI-enabled volumes. Create a volume with iSCSI protocol in CoreSAN, or enable iSCSI on an existing volume.
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
                    <th className="pb-2 font-medium">ALUA</th>
                  </tr>
                </thead>
                <tbody>
                  {volumes.map(v => {
                    const target = targets.find(t => t.volume_id === v.id)
                    return (
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
                        <td className="py-2">
                          {target && (
                            <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${aluaColor(target.alua_state)}`}>
                              {aluaLabel(target.alua_state)}
                            </span>
                          )}
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            )}
          </div>
        </Card>
      )}

      {tab === 'acls' && (
        <Card>
          <div className="p-4">
            <div className="flex items-center justify-between mb-4">
              <p className="text-sm text-vmm-muted">iSCSI initiator access control per volume.</p>
              <Button size="sm" onClick={() => setShowCreateAcl(true)}>
                <Plus size={14} className="mr-1" /> Add Initiator
              </Button>
            </div>
            {acls.length === 0 ? (
              <p className="text-vmm-muted text-sm py-8 text-center">
                No ACLs configured. Add an initiator IQN to allow iSCSI access to a volume.
              </p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-vmm-muted border-b border-vmm-border">
                    <th className="pb-2 font-medium">Volume</th>
                    <th className="pb-2 font-medium">Initiator IQN</th>
                    <th className="pb-2 font-medium">Comment</th>
                    <th className="pb-2 font-medium">Created</th>
                    <th className="pb-2 font-medium w-16"></th>
                  </tr>
                </thead>
                <tbody>
                  {acls.map(a => (
                    <tr key={a.id} className="border-b border-vmm-border/50 hover:bg-vmm-hover">
                      <td className="py-2 font-medium text-vmm-text">{a.volume_name}</td>
                      <td className="py-2 font-mono text-xs text-vmm-text">{a.initiator_iqn}</td>
                      <td className="py-2 text-vmm-muted">{a.comment || '\u2014'}</td>
                      <td className="py-2 text-vmm-muted text-xs">{a.created_at}</td>
                      <td className="py-2">
                        <button onClick={() => setDeleteAclId(a.id)}
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

      {tab === 'connect' && (
        <Card>
          <div className="p-4 space-y-4">
            <p className="text-sm text-vmm-muted">
              Use standard iSCSI initiators to connect to block storage volumes.
            </p>
            <div className="space-y-3">
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">Target Portal</h3>
                <code className="block bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text">
                  &lt;host&gt;:3260
                </code>
              </div>
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">Linux Initiator (iscsiadm)</h3>
                <pre className="bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text whitespace-pre overflow-x-auto">{`# Discovery
iscsiadm -m discovery -t sendtargets -p <host>:3260

# Login to a target
iscsiadm -m node -T iqn.2026-04.io.corevm:<volume> -p <host>:3260 --login

# Verify block device
lsblk | grep sd`}</pre>
              </div>
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">Multipath Setup</h3>
                <pre className="bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text whitespace-pre overflow-x-auto">{`apt install multipath-tools
cat >> /etc/multipath.conf <<EOF
devices {
    device {
        vendor  "CoreVM"
        product "CoreSAN"
        path_grouping_policy  group_by_prio
        prio    alua
        failback immediate
    }
}
EOF
systemctl restart multipathd
multipath -ll`}</pre>
              </div>
            </div>
          </div>
        </Card>
      )}

      <CreateIscsiAclDialog
        open={showCreateAcl}
        onClose={() => setShowCreateAcl(false)}
        onCreated={() => { setShowCreateAcl(false); fetchData() }}
        isCluster={isCluster}
        sanBase={sanBase}
        volumes={volumes}
      />

      <ConfirmDialog
        open={!!deleteAclId}
        title="Delete iSCSI ACL"
        message="This will immediately revoke iSCSI access for this initiator. Active sessions may be disconnected."
        confirmLabel="Delete"
        danger
        onConfirm={handleDeleteAcl}
        onCancel={() => setDeleteAclId(null)}
      />
    </div>
  )
}
```

- [ ] **Step 3: Commit**

```bash
git add apps/vmm-ui/src/api/types.ts apps/vmm-ui/src/pages/StorageBlockStorage.tsx
git commit -m "feat(ui): add Block Storage page with iSCSI volumes, ACLs, and connection info"
```

---

## Task 11: Frontend — CreateIscsiAclDialog + Navigation + Volume Dialog

**Files:**
- Create: `apps/vmm-ui/src/components/coresan/CreateIscsiAclDialog.tsx`
- Modify: `apps/vmm-ui/src/App.tsx:22,111`
- Modify: `apps/vmm-ui/src/components/Sidebar.tsx:6,32,91`
- Modify: `apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx:139`

- [ ] **Step 1: Create `CreateIscsiAclDialog.tsx`**

Create `apps/vmm-ui/src/components/coresan/CreateIscsiAclDialog.tsx`:

```tsx
import { useState } from 'react'
import type { CoreSanVolume } from '../../api/types'
import api from '../../api/client'
import Dialog from '../Dialog'
import FormField from '../FormField'
import TextInput from '../TextInput'
import Select from '../Select'
import Button from '../Button'

interface Props {
  open: boolean
  onClose: () => void
  onCreated: () => void
  isCluster: boolean
  sanBase: string
  volumes: CoreSanVolume[]
}

export default function CreateIscsiAclDialog({ open, onClose, onCreated, isCluster, sanBase, volumes }: Props) {
  const [volumeId, setVolumeId] = useState('')
  const [iqn, setIqn] = useState('')
  const [comment, setComment] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)

  const handleSubmit = async () => {
    if (!volumeId || !iqn) return
    if (!iqn.startsWith('iqn.')) {
      setError('Initiator IQN must start with "iqn."')
      return
    }
    setLoading(true)
    setError('')
    try {
      const body = { volume_id: volumeId, initiator_iqn: iqn, comment }
      if (isCluster) {
        await api.post('/api/san/iscsi/acls', body)
      } else {
        const resp = await fetch(`${sanBase}/api/iscsi/acls`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        })
        if (!resp.ok) throw new Error(await resp.text())
      }
      setVolumeId('')
      setIqn('')
      setComment('')
      onCreated()
    } catch (e: any) {
      setError(e.message || 'Failed to create ACL')
    } finally {
      setLoading(false)
    }
  }

  return (
    <Dialog open={open} title="Add iSCSI Initiator" onClose={onClose} width="max-w-md">
      <div className="space-y-4">
        {error && (
          <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{error}</div>
        )}
        <FormField label="Volume">
          <Select value={volumeId} onChange={e => setVolumeId(e.target.value)}
            options={[
              { value: '', label: 'Select a volume...' },
              ...volumes.map(v => ({ value: v.id, label: v.name })),
            ]} />
        </FormField>
        <FormField label="Initiator IQN">
          <TextInput value={iqn} onChange={e => setIqn(e.target.value)}
            placeholder="iqn.2024-01.com.example:initiator01" />
          <p className="text-[10px] text-vmm-muted mt-1">
            Find your initiator IQN: <code>cat /etc/iscsi/initiatorname.iscsi</code>
          </p>
        </FormField>
        <FormField label="Comment (optional)">
          <TextInput value={comment} onChange={e => setComment(e.target.value)}
            placeholder="e.g. Production DB server" />
        </FormField>
        <div className="flex justify-end gap-2 pt-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSubmit} disabled={!volumeId || !iqn || loading}>
            {loading ? 'Adding...' : 'Add Initiator'}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
```

- [ ] **Step 2: Add route in `App.tsx`**

In `apps/vmm-ui/src/App.tsx`, add import after line 22 (`import StorageObjectStorage ...`):

```tsx
import StorageBlockStorage from './pages/StorageBlockStorage'
```

And add route after line 111 (`<Route path="object-storage" ...>`):

```tsx
            <Route path="block-storage" element={<StorageBlockStorage />} />
```

- [ ] **Step 3: Add nav item in `Sidebar.tsx`**

In `apps/vmm-ui/src/components/Sidebar.tsx`, add to the `Shield` import (line 6) — add `Server` if not already imported (it is).

In the standalone storage children (around line 32), add after the object-storage entry:

```tsx
      { to: '/storage/block-storage', icon: Server, label: 'Block Storage' },
```

And in the cluster storage children (around line 91), add after the object-storage entry:

```tsx
      { to: '/storage/block-storage', icon: Server, label: 'Block Storage' },
```

- [ ] **Step 4: Add iSCSI checkbox in `CreateVolumeDialog.tsx`**

In `apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx`, add after line 139 (after the S3 `</label>` closing tag), before the closing `</div>` of the protocol flex container:

```tsx
            <label className="flex items-center gap-2 text-sm text-vmm-text cursor-pointer">
              <input type="checkbox" checked={newVolProtocols.includes('iscsi')}
                onChange={e => {
                  if (e.target.checked) setNewVolProtocols([...newVolProtocols, 'iscsi'])
                  else setNewVolProtocols(newVolProtocols.filter(p => p !== 'iscsi'))
                }}
                className="rounded border-vmm-border" />
              iSCSI Block Storage
            </label>
```

And add a hint after the existing S3 hint (after line 144), a new conditional hint:

```tsx
          {newVolProtocols.includes('iscsi') && (
            <p className="text-xs text-vmm-muted mt-1">
              iSCSI access requires vmm-iscsi running on the host. Manage ACLs in Block Storage page.
            </p>
          )}
```

- [ ] **Step 5: Verify frontend builds**

Run: `cd /home/cmoeller/Development/corevm/apps/vmm-ui && npm run build`
Expected: builds without errors

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-ui/src/components/coresan/CreateIscsiAclDialog.tsx apps/vmm-ui/src/App.tsx apps/vmm-ui/src/components/Sidebar.tsx apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx
git commit -m "feat(ui): add CreateIscsiAclDialog, nav entry, route, and iSCSI protocol checkbox"
```

---

## Task 12: Final Integration Check

- [ ] **Step 1: Full workspace build**

Run: `cd /home/cmoeller/Development/corevm && cargo build`
Expected: all crates compile

- [ ] **Step 2: Frontend build**

Run: `cd /home/cmoeller/Development/corevm/apps/vmm-ui && npm run build`
Expected: builds without errors

- [ ] **Step 3: Review complete — commit any fixups**

If any compilation or build issues were found, fix them and commit:

```bash
git add -A
git commit -m "fix: resolve compilation issues from iSCSI integration"
```
