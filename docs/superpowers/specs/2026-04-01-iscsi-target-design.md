# CoreSAN iSCSI Target Design

## Summary

Add iSCSI block storage as a new access protocol to CoreSAN, following the same architectural pattern as the S3 object storage gateway. A new `vmm-iscsi` service implements a full iSCSI target in Rust, communicating with vmm-san via Unix Domain Sockets. Cluster-multipathing (ALUA) allows every node holding a volume replica to serve as an iSCSI target, with automatic failover via standard Linux `dm-multipath` on the initiator side.

**Key decisions:**
- Pure Rust iSCSI target (no kernel dependencies like LIO/TCM)
- Cluster-multipathing via ALUA (Asymmetric Logical Unit Access)
- Minimal SBC SCSI command set + ALUA commands
- No authentication (IQN-based ACLs only)
- 1:1 mapping: one volume = one target = one LUN
- Monolithic service with internal worker tasks per target

## Architecture Overview

```
                          +------------------------------------------+
                          |              CoreSAN Cluster              |
                          |                                          |
  iSCSI Initiator --TCP-->|  vmm-iscsi --UDS--> vmm-san             |
  (Linux/Windows)  :3260  |  (Target)    |       (Storage Engine)    |
                          |              |                           |
                          |      mgmt.sock (ACLs, Volume-Lookup)    |
                          |      blk-{vol}.sock (Block I/O)         |
                          +------------------------------------------+
```

### New Components

| Component | Description | Analogue |
|---|---|---|
| `apps/vmm-iscsi/` | New Rust service, own iSCSI target | `apps/vmm-s3gw/` |
| `libs/vmm-core/src/san_iscsi.rs` | iSCSI block socket protocol | `san_object.rs` |
| `apps/vmm-san/src/engine/iscsi_server.rs` | Block I/O socket handler in vmm-san | `object_server.rs` |
| `apps/vmm-san/src/api/iscsi.rs` | REST API for ACL management | `api/s3.rs` |
| `apps/vmm-ui/.../StorageBlockStorage.tsx` | UI page for iSCSI targets | `StorageObjectStorage.tsx` |

### Sockets

- **Management:** `/run/vmm-san/mgmt.sock` (existing, extended with iSCSI commands)
- **Block I/O:** `/run/vmm-san/blk-{volume_id}.sock` (new, per volume)

The existing `{volume_id}.sock` (disk_server) sockets are not reused because they have file-level Open/Close semantics for VM disk I/O. iSCSI needs flat block I/O at LBA offsets without filesystem concepts.

## iSCSI Protocol Stack (vmm-iscsi)

```
  TCP :3260
    |
    v
+---------------------------------------------+
|  vmm-iscsi                                   |
|                                              |
|  +------------+    +----------------------+  |
|  | Discovery   |    | Target Worker Pool   |  |
|  | Service     |    |                      |  |
|  |             |    |  Worker vol-A --UDS--+--+--> blk-{vol-A}.sock
|  | SendTargets |    |  Worker vol-B --UDS--+--+--> blk-{vol-B}.sock
|  | Login       |    |  Worker vol-C --UDS--+--+--> blk-{vol-C}.sock
|  | ACL-Check   |    |                      |  |
|  +------+------+    +----------------------+  |
|         |                                     |
|         +-------- mgmt.sock ------------------+--> vmm-san
+---------------------------------------------+
```

### Login Flow

1. Initiator connects on TCP :3260
2. **Discovery Phase:** Initiator sends `SendTargets`. vmm-iscsi queries mgmt.sock for all volumes with `"iscsi"` in access_protocols. Responds with target IQNs + portal addresses (all cluster nodes holding the volume).
3. **Login Phase:** Initiator sends Login PDU with target IQN. vmm-iscsi:
   - Extracts initiator IQN from Login PDU
   - Checks ACL via mgmt.sock (`ValidateIscsiAcl` command)
   - Negotiates session parameters (MaxBurstLength, etc.)
   - Sends Login Response
4. **Full Feature Phase:** TCP connection is handed to a target worker task. Worker has its own connection to `blk-{volume_id}.sock`.

### iSCSI PDU Types (Minimal Set)

| PDU Type | Direction | Description |
|---|---|---|
| Login Request/Response | I->T, T->I | Session setup, parameter negotiation |
| Text Request/Response | I->T, T->I | SendTargets discovery |
| SCSI Command / Response | I->T, T->I | SCSI commands (Read/Write/Inquiry etc.) |
| Data-Out / Data-In | I->T, T->I | Bulk data (write payload, read data) |
| NOP-Out / NOP-In | I<->T | Keepalive / ping |
| Logout Request/Response | I->T, T->I | Session teardown |

### SCSI Commands (Minimal SBC Set)

| Command | OpCode | Description |
|---|---|---|
| `TEST UNIT READY` | 0x00 | Is the LUN ready? |
| `INQUIRY` | 0x12 | Device info (Vendor: "CoreVM", Product: "CoreSAN") |
| `READ CAPACITY(10)` | 0x25 | Volume size in blocks (512 byte block size) |
| `READ CAPACITY(16)` | 0x9E | For volumes > 2TB |
| `MODE SENSE(6/10)` | 0x1A/0x5A | Device parameters |
| `READ(10/16)` | 0x28/0x88 | Read block data |
| `WRITE(10/16)` | 0x2A/0x8A | Write block data |
| `REPORT LUNS` | 0xA0 | LUN list (always LUN 0) |
| `REPORT TARGET PORT GROUPS` | 0xA3/0x0A | ALUA port status |
| `SET TARGET PORT GROUPS` | 0xA4/0x0A | ALUA switching |

### Session Parameters (Defaults)

| Parameter | Value |
|---|---|
| MaxBurstLength | 262144 (256 KB) |
| MaxRecvDataSegmentLength | 65536 (64 KB) |
| FirstBurstLength | 65536 |
| MaxOutstandingR2T | 1 |
| InitialR2T | Yes |
| ImmediateData | Yes |
| DefaultTime2Wait | 2 |
| DefaultTime2Retain | 20 |

## Cluster Multipathing (ALUA)

Every CoreSAN node holding a volume replica runs as an iSCSI target for that volume. The initiator sees multiple paths and uses `dm-multipath` for failover.

```
                        +--------------+
                        |  Initiator   |
                        | dm-multipath |
                        +---+------+---+
                            |      |
                     TCP :3260   TCP :3260
                            |      |
                     +------v-+ +--v------+
                     | Node A | | Node B  |
                     |vmm-iscsi| |vmm-iscsi|
                     | ALUA:  | | ALUA:   |
                     | A/O    | | Standby |
                     +----+---+ +----+----+
                          |          |
                     +----v----------v----+
                     |   CoreSAN Cluster  |
                     | (replicated data)  |
                     +--------------------+
```

### ALUA States

Each node reports as its own Target Port Group (TPG):

| ALUA State | Meaning | When |
|---|---|---|
| Active/Optimized (A/O) | Preferred path, full performance | Node is leader for the volume |
| Active/Non-Optimized (A/NO) | Works, but not optimal | Node has volume, not leader |
| Standby | Path available, I/O rerouted | Node has replica, no local access |
| Unavailable | Path unreachable | Node offline |

### Leader Determination

CoreSAN's existing quorum-based leader election is reused:
- Leader node for a volume gets ALUA status `Active/Optimized`
- Other participating nodes get `Active/Non-Optimized`
- On leader change (node failure), ALUA status updates automatically

### ALUA Mechanism

1. **On start / volume activation:** vmm-iscsi queries mgmt.sock for ALUA state per iSCSI volume
2. **REPORT TARGET PORT GROUPS:** Reports all known port group states (own + peers) to initiator
3. **State changes:** vmm-san notifies vmm-iscsi on leader change (new mgmt command `NotifyAluaChange`). vmm-iscsi updates reported status. Initiator polls via `REPORT TARGET PORT GROUPS` or gets Unit Attention async event.
4. **I/O on non-optimized nodes:** Still works — vmm-san routes reads/writes internally via peer replication, just with higher latency.

### IQN Schema

```
iqn.2026-04.io.corevm:<volume-name>
```

All nodes report the same target IQN but with different portal addresses (IP:3260). The initiator recognizes multipath via VPD page 0x83 (Device Identification):
- **NAA Identifier:** Based on volume UUID, identical across all nodes
- **Target Port Group Identifier:** Unique per node

### Initiator Reference Configuration

```
# /etc/multipath.conf
devices {
    device {
        vendor  "CoreVM"
        product "CoreSAN"
        path_grouping_policy  group_by_prio
        prio                  alua
        failback              immediate
        no_path_retry         queue
    }
}
```

## Socket Protocol & vmm-san Integration

### Block I/O Protocol (`san_iscsi.rs` in vmm-core)

```
Magic: 0x49534353 ("ISCS")
Response Magic: 0x49534352 ("ISCR")
Socket: /run/vmm-san/blk-{volume_id}.sock
```

| Command | Description |
|---|---|
| `ReadBlocks` | Read N bytes from LBA offset. Response: data |
| `WriteBlocks` | Write data at LBA offset |
| `Flush` | Sync pending writes to disk |
| `GetCapacity` | Volume size in bytes + block size (512) |
| `GetAluaState` | Current ALUA state for this volume on this node |

### Request Header (32 bytes)

```rust
pub struct IscsiRequestHeader {
    pub magic: u32,        // 0x49534353
    pub cmd: u32,          // IscsiCommand enum
    pub lba: u64,          // Logical Block Address (in 512-byte blocks)
    pub length: u32,       // Bytes to read/write
    pub flags: u32,        // Reserved
    pub _reserved: u64,
}
```

### Response Header (16 bytes)

```rust
pub struct IscsiResponseHeader {
    pub magic: u32,        // 0x49534352
    pub status: u32,       // IscsiStatus enum
    pub length: u32,       // Response data length
    pub _reserved: u32,
}
```

### Socket Handler (`iscsi_server.rs` in vmm-san)

Analogous to `object_server.rs` and `disk_server.rs`:
- `spawn_all()` on startup: spawns listener for all volumes with `"iscsi"` in access_protocols
- Socket path: `/run/vmm-san/blk-{volume_id}.sock`
- Translates LBA offsets to chunk_index + chunk_offset, uses existing `ChunkService` for I/O
- Block size: 512 bytes (standard SCSI block device)

### Management Socket Extensions (`san_mgmt.rs`)

New commands (starting at offset 40 to avoid S3 range 20-23):

| Command | Value | Description |
|---|---|---|
| `ListIscsiVolumes` | 40 | All volumes with "iscsi" in access_protocols |
| `ListIscsiAcls` | 41 | ACL entries for a volume (key=volume_id) |
| `CreateIscsiAcl` | 42 | Add initiator IQN to volume ACL |
| `DeleteIscsiAcl` | 43 | Remove initiator IQN from ACL |
| `GetAluaState` | 44 | ALUA state of current node for a volume |
| `GetTargetPortGroups` | 45 | All TPGs with ALUA state for a volume (all nodes) |

### Database Table

```sql
iscsi_acls (
    id TEXT PRIMARY KEY,
    volume_id TEXT REFERENCES volumes(id) ON DELETE CASCADE,
    initiator_iqn TEXT NOT NULL,
    comment TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(volume_id, initiator_iqn)
)
```

### REST API (`api/iscsi.rs`)

| Endpoint | Description |
|---|---|
| `GET /api/iscsi/acls?volume_id=X` | ACLs for a volume |
| `POST /api/iscsi/acls` | Add ACL `{volume_id, initiator_iqn, comment}` |
| `DELETE /api/iscsi/acls/{id}` | Remove ACL |
| `GET /api/iscsi/targets` | All iSCSI targets with status/ALUA info |

## vmm-iscsi Service Structure

### Configuration (`/etc/vmm/iscsi.toml`)

```toml
[server]
listen = "0.0.0.0:3260"
node_name = "iqn.2026-04.io.corevm"

[san]
mgmt_socket = "/run/vmm-san/mgmt.sock"
block_socket_dir = "/run/vmm-san"

[logging]
level = "info"
```

### Module Structure

```
apps/vmm-iscsi/
+-- Cargo.toml
+-- src/
    +-- main.rs              # Entry point, config, TCP listener
    +-- config.rs            # IscsiConfig (analogous to s3gw/config.rs)
    +-- socket.rs            # SocketPool for mgmt + blk sockets (analogous to s3gw/socket.rs)
    +-- pdu.rs               # iSCSI PDU parsing/serialization
    |                        #   Login, Text, SCSI Command, Data-In/Out, NOP, Logout
    +-- session.rs           # Session state machine
    |                        #   Discovery vs Normal session
    |                        #   Parameter negotiation
    |                        #   CmdSN/StatSN sequencing
    +-- scsi.rs              # SCSI command handler
    |                        #   Inquiry, ReadCapacity, Read, Write, ModeSense
    |                        #   ReportLuns, TestUnitReady
    +-- alua.rs              # ALUA logic
    |                        #   ReportTargetPortGroups
    |                        #   Status tracking per volume
    |                        #   Unit Attention on state change
    +-- discovery.rs         # SendTargets response builder
                             #   Queries mgmt.sock for iSCSI volumes
                             #   Builds target list with portal addresses of all nodes
```

### Code Flow

```
main.rs:
  1. Load config
  2. Create SocketPool (mgmt + blk connections)
  3. TCP listener on :3260
  4. Per incoming connection -> tokio::spawn(session::handle_connection)

session.rs:
  handle_connection(tcp_stream, socket_pool):
    1. Login phase:
       - Read PDU -> Login Request
       - If discovery session -> only allow Text/SendTargets
       - If normal session:
         a. Extract target IQN -> resolve volume ID via mgmt.sock
         b. Extract initiator IQN -> check ACL via mgmt.sock
         c. Negotiate parameters (MaxBurstLength etc.)
         d. Send Login Response
    2. Full feature phase:
       - Connect to blk-{volume_id}.sock
       - Loop: read PDU -> dispatch:
         - SCSI Command -> scsi.rs
         - NOP-Out -> reply NOP-In
         - Logout -> end session

scsi.rs:
  handle_scsi_command(cdb, session, socket):
    - Parse CDB (Command Descriptor Block)
    - Match on OpCode:
      - READ(10/16) -> socket.read_blocks(lba, length)
      - WRITE(10/16) -> socket.write_blocks(lba, data)
      - INQUIRY -> static device info
      - READ CAPACITY -> socket.get_capacity()
      - REPORT TARGET PORT GROUPS -> alua.get_tpgs()
      - etc.
    - Send SCSI Response + Data-In PDU back
```

### Dependencies (Cargo.toml)

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
vmm-core = { path = "../../libs/vmm-core" }
```

No external iSCSI libraries. Pure Rust implementation using standard crates only.

## Frontend — iSCSI UI

### New/Changed Files

| File | Description |
|---|---|
| `pages/StorageBlockStorage.tsx` | New page (analogous to `StorageObjectStorage.tsx`) |
| `components/coresan/CreateIscsiAclDialog.tsx` | Dialog to add initiator IQN |
| `components/coresan/CreateVolumeDialog.tsx` | Extended: third checkbox "iSCSI Block Storage" |
| `api/types.ts` | New types `IscsiAcl`, `IscsiTarget` |
| Navigation/Routing | New menu item "Block Storage" under Storage |

### StorageBlockStorage.tsx — Three Tabs

**Tab 1: "iSCSI Volumes"**
- Table of all volumes with `"iscsi"` in access_protocols
- Columns: Name, Status, Size, Used, Protocols, FTT, ALUA State
- Note: "Manage volumes in the CoreSAN page."

**Tab 2: "Access Control (ACLs)"**
- Table of all ACL entries, filterable by volume
- Columns: Volume, Initiator IQN, Comment, Created
- Button "Add Initiator" -> opens CreateIscsiAclDialog
- Delete button per entry with confirm dialog

**Tab 3: "Connection Info"**
- Target portal: `<host>:3260`
- IQN format: `iqn.2026-04.io.corevm:<volume-name>`
- Example commands for Linux initiator:

```bash
# Discovery
iscsiadm -m discovery -t sendtargets -p <host>:3260

# Login
iscsiadm -m node -T iqn.2026-04.io.corevm:<volume> -p <host>:3260 --login

# Multipath Setup
apt install multipath-tools
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
multipath -ll
```

### CreateIscsiAclDialog.tsx

- Fields: Volume (dropdown, iSCSI volumes only), Initiator IQN (text), Comment (optional)
- Validation: IQN must start with `iqn.`
- POST to `/api/iscsi/acls`

### CreateVolumeDialog.tsx Extension

Third checkbox alongside FUSE and S3:

```tsx
<label>
  <input type="checkbox" checked={newVolProtocols.includes('iscsi')} />
  iSCSI Block Storage
</label>
// Hint when checked:
// "iSCSI access requires vmm-iscsi running on the host. Manage ACLs in Block Storage page."
```

### TypeScript Types (`api/types.ts`)

```typescript
export interface IscsiAcl {
  id: string
  volume_id: string
  volume_name?: string
  initiator_iqn: string
  comment?: string
  created_at: string
}

export interface IscsiTarget {
  volume_id: string
  volume_name: string
  iqn: string
  portals: string[]        // IP:port of all nodes
  alua_state: string       // "active_optimized" | "active_non_optimized" | "standby"
  status: string
}
```
