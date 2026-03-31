# S3-Compatible Object Storage Gateway for CoreSAN

**Date:** 2026-03-31
**Status:** Approved

## Overview

Add S3-compatible object storage access to CoreSAN volumes. Volumes can declare supported access protocols via a new `access_protocols` field (JSON array). When `"s3"` is included, vmm-san starts a dedicated Object Socket for that volume, and the new `vmm-s3gw` binary translates S3 HTTP requests into Object Socket calls.

This replaces the need for MinIO (no longer open source) with a native, fully integrated solution. The architecture is designed to support additional protocols (NFS, iSCSI) in the future via the same pattern.

## Architecture

```
S3-Client ──HTTP:9000──▶ vmm-s3gw ──UDS──▶ vmm-san ──▶ Chunks/DB/Replikation

vmm-s3gw connects to:
  /run/vmm-san/mgmt.sock              # Auth, ListBuckets, CreateBucket
  /run/vmm-san/obj-{volume_id}.sock   # Object I/O per volume (lazy-connect)
```

- **vmm-s3gw** is a pure frontend — no database, no chunk logic, no replication awareness.
- **vmm-san** owns all state. The Object Socket reuses the existing chunk engine, file_map, write leases, and replication.
- No cluster routing needed — SAN replication handles data distribution transparently.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Gateway deployment | Separate binary (`vmm-s3gw`) | Clean separation, independent lifecycle |
| Communication | Unix Domain Sockets | Max performance, no HTTP overhead, co-located with vmm-san |
| Protocol marking | `access_protocols` JSON array | Extensible for future protocols (nfs, iscsi) |
| Bucket mapping | Bucket name = Volume name (1:1) | Simple, no extra mapping layer |
| Auth | AWS Signature V4, credentials derived from existing users | Consistent with user management, S3-client compatible |
| Socket separation | Separate Object Socket (not extending Disk Server) | No risk to VM block I/O performance |

## Data Model Changes

### volumes table — new field

```sql
access_protocols TEXT DEFAULT '["fuse"]'  -- JSON array: "fuse", "s3", future: "nfs", "iscsi"
```

No migration needed (no production installations exist). Added directly to schema.

### New table: s3_credentials

```sql
CREATE TABLE s3_credentials (
    id TEXT PRIMARY KEY,                -- UUID
    access_key TEXT NOT NULL UNIQUE,     -- 20 chars, AWS-compatible format
    secret_key_enc TEXT NOT NULL,        -- AES-256-GCM encrypted (needed for SigV4 verification)
    user_id TEXT NOT NULL,              -- Reference to vmm-server user
    display_name TEXT DEFAULT '',
    status TEXT DEFAULT 'active',       -- active, disabled
    created_at TEXT,
    expires_at TEXT                      -- optional, NULL = no expiry
);
```

Secret key is encrypted (not hashed) because AWS Signature V4 requires the plaintext secret to compute the expected signature. Encryption key is a node-local key.

### New table: multipart_uploads

```sql
CREATE TABLE multipart_uploads (
    upload_id TEXT PRIMARY KEY,
    volume_id TEXT NOT NULL REFERENCES volumes(id),
    object_key TEXT NOT NULL,
    created_by TEXT NOT NULL,           -- access_key
    created_at TEXT,
    status TEXT DEFAULT 'active'        -- active, completed, aborted
);
```

### New table: multipart_parts

```sql
CREATE TABLE multipart_parts (
    upload_id TEXT NOT NULL REFERENCES multipart_uploads(upload_id),
    part_number INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    etag TEXT NOT NULL,                 -- MD5 of part data
    backend_path TEXT NOT NULL,         -- Temp chunk path
    PRIMARY KEY (upload_id, part_number)
);
```

On `CompleteMultipartUpload`, parts are concatenated and written as a normal object into `file_map`.

## Object Socket Protocol

New binary protocol in `libs/vmm-core/src/san_object.rs`, analogous to `san_disk.rs`.

### Commands

```rust
enum ObjectCommand: u32 {
    Put                = 1,   // key + data → store object
    Get                = 2,   // key → data
    Head               = 3,   // key → metadata (size, sha256, updated_at)
    Delete             = 4,   // key → ok
    List               = 5,   // prefix + marker + max_keys → key list
    Copy               = 6,   // src_key + dst_key → ok
    InitMultipart      = 7,   // key → upload_id
    UploadPart         = 8,   // upload_id + part_num + data → etag
    CompleteMultipart  = 9,   // upload_id + part_list → ok
    AbortMultipart     = 10,  // upload_id → ok
}
```

### Request Header

```rust
struct ObjectRequestHeader {
    magic: u32,       // 0x4F424A53 ("OBJS")
    cmd: u32,         // ObjectCommand
    key_len: u32,     // Length of key string
    body_len: u64,    // Length of body data (object data for Put, 0 for Get)
    flags: u32,       // Reserved for future use
}
// Followed by: key_bytes (key_len), then body_bytes (body_len)
```

### Response Header

```rust
struct ObjectResponseHeader {
    magic: u32,          // 0x4F424A52 ("OBJR")
    status: u32,         // 0 = Ok, 1+ = error codes
    body_len: u64,       // Length of response data
    metadata_len: u32,   // Optional metadata (JSON)
}
// Followed by: metadata_bytes (metadata_len), then body_bytes (body_len)
```

## Management Socket Protocol

`/run/vmm-san/mgmt.sock` — handles operations that are not volume-specific.

### Commands

```rust
enum MgmtCommand: u32 {
    ListVolumes          = 1,   // → list of volumes with s3 in access_protocols
    CreateVolume         = 2,   // name + config → volume_id
    DeleteVolume         = 3,   // volume_id → ok
    CreateCredential     = 20,  // user_id + display_name → access_key + secret_key
    ValidateCredential   = 21,  // access_key + string_to_sign + signature → ok/denied
    ListCredentials      = 22,  // → list of credentials
    DeleteCredential     = 23,  // credential_id → ok
}
```

Uses the same header format as Object Socket (magic: `0x4D474D54` / "MGMT").

## vmm-s3gw — S3 Gateway Binary

### New workspace member: `apps/vmm-s3gw/`

Axum-based HTTP server implementing the S3-compatible API.

### S3 API Endpoints

Supports both path-style (`host:9000/bucket/key`) and virtual-host-style (`bucket.host:9000/key`).

```
GET    /                                          → ListBuckets
PUT    /{bucket}                                  → CreateBucket
DELETE /{bucket}                                  → DeleteBucket
HEAD   /{bucket}                                  → HeadBucket
GET    /{bucket}?list-type=2                      → ListObjectsV2
PUT    /{bucket}/{key...}                         → PutObject
GET    /{bucket}/{key...}                         → GetObject
HEAD   /{bucket}/{key...}                         → HeadObject
DELETE /{bucket}/{key...}                         → DeleteObject
PUT    /{bucket}/{key...} + x-amz-copy-source     → CopyObject
POST   /{bucket}/{key...}?uploads                 → InitiateMultipartUpload
PUT    /{bucket}/{key...}?partNumber=N&uploadId=X → UploadPart
POST   /{bucket}/{key...}?uploadId=X              → CompleteMultipartUpload
DELETE /{bucket}/{key...}?uploadId=X              → AbortMultipartUpload
```

Presigned URLs supported via `X-Amz-Signature`, `X-Amz-Credential`, `X-Amz-Expires` query parameters.

### Authentication

Full AWS Signature V4 implementation:

1. Client sends `Authorization: AWS4-HMAC-SHA256 Credential=AKID/date/region/s3/aws4_request, SignedHeaders=..., Signature=...`
2. Gateway extracts access key, sends `ValidateCredential` to mgmt socket
3. vmm-san decrypts secret key, computes expected signature, compares
4. On success: request proceeds via volume-specific object socket

### Configuration (`/etc/vmm/s3gw.toml`)

```toml
[server]
listen = "0.0.0.0:9000"
region = "us-east-1"

[san]
mgmt_socket = "/run/vmm-san/mgmt.sock"
object_socket_dir = "/run/vmm-san"

[tls]
cert = "/etc/vmm/certs/s3.crt"
key = "/etc/vmm/certs/s3.key"
```

### Socket Connection Management

- Lazy-connect to object sockets on first request per volume
- Connection pool with keep-alive per volume socket
- Automatic reconnect on socket errors

## Error Handling

Object Socket errors map to standard S3 XML error responses:

| Object Socket Status | S3 Error Code            | HTTP Status |
|---------------------|--------------------------|-------------|
| Ok                  | —                        | 200         |
| NotFound            | NoSuchKey / NoSuchBucket | 404         |
| AccessDenied        | AccessDenied             | 403         |
| AlreadyExists       | BucketAlreadyOwnedByYou  | 409         |
| InvalidKey          | InvalidArgument          | 400         |
| NoSpace             | InsufficientStorage      | 507         |
| LeaseDenied         | SlowDown                 | 503         |

All errors returned as XML:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>NoSuchKey</Code>
  <Message>The specified key does not exist.</Message>
  <Key>photos/2024/test.jpg</Key>
  <RequestId>uuid</RequestId>
</Error>
```

## Testing Strategy

- **Unit tests** in vmm-s3gw: AWS SigV4 parsing, request routing, XML serialization/deserialization
- **Integration tests**: vmm-s3gw + vmm-san together, using `aws-sdk-rust` or `aws` CLI as client
- **Compatibility tests**: MinIO `mc` CLI — if `mc ls`, `mc cp`, `mc mb` work, compatibility is confirmed

## File Inventory

### New files

| File | Purpose |
|------|---------|
| `apps/vmm-s3gw/` | S3 Gateway binary (Axum, AWS SigV4, UDS client) |
| `apps/vmm-san/src/engine/object_server.rs` | Object Socket listener per volume |
| `apps/vmm-san/src/engine/mgmt_server.rs` | Management Socket (auth, ListBuckets) |
| `libs/vmm-core/src/san_object.rs` | Shared protocol structs (ObjectCommand, headers) |

### Changed files

| File | Change |
|------|--------|
| `apps/vmm-san/src/db/mod.rs` | `access_protocols` field, `s3_credentials`, `multipart_uploads`, `multipart_parts` tables |
| `apps/vmm-san/src/api/volumes.rs` | `access_protocols` in Create/Update/Response |
| `apps/vmm-san/src/main.rs` | Spawn object_server + mgmt_server |
| `Cargo.toml` (workspace) | New member `apps/vmm-s3gw` |

### Untouched

- `engine/disk_server.rs` — zero changes
- `engine/fuse_mount.rs` — zero changes
- `storage/chunk.rs` — zero changes
- Replication engine — zero changes
- Write lease system — zero changes
- All existing REST APIs — zero changes

## vmm-cluster Integration

The cluster already proxies all SAN operations via `/api/san/*` routes (in `apps/vmm-cluster/src/api/san.rs`) using `SanClient` HTTP forwarding. S3 object storage extends this pattern:

### New Cluster API Routes

```
GET    /api/san/s3/credentials              → List S3 credentials (forward to any SAN host mgmt API)
POST   /api/san/s3/credentials              → Create S3 credential
DELETE /api/san/s3/credentials/{id}          → Delete S3 credential
GET    /api/san/s3/status                    → S3 gateway status per host
```

### Volume access_protocols in Cluster

- `POST /api/san/volumes` already forwards to any SAN host — `access_protocols` is simply a new field in the request body, no special cluster handling needed
- `GET /api/san/volumes` response now includes `access_protocols` — cluster passes through transparently
- Volume update (`PUT /api/san/volumes/{id}`) supports changing `access_protocols` — cluster logs event

### S3 Credential Management via Cluster

The cluster acts as proxy for S3 credential CRUD. Credentials are stored in vmm-san's DB and synced via the existing peer replication. The cluster routes credential requests to any online SAN host via a new REST API on vmm-san:

```
GET    /api/s3/credentials                  → List all credentials
POST   /api/s3/credentials                  → Create credential (returns access_key + secret_key)
DELETE /api/s3/credentials/{id}             → Delete credential
```

### S3 Gateway Health in SAN Health Engine

The existing `engine/san_health.rs` health polling (30s interval) is extended to report S3 gateway status per host. Each SAN host reports whether `vmm-s3gw` is running and accessible.

## vmm-ui Integration

### Navigation

Add "Object Storage" as a new item under the Storage sidebar section (both standalone and cluster mode):

```
Storage (expandable)
  ├─ Overview
  ├─ Local Storage
  ├─ Shared Storage
  ├─ CoreSAN
  ├─ Object Storage    ← NEW
  ├─ Disk Management
  └─ QoS Policies
```

### New Page: StorageObjectStorage.tsx

Main object storage management page with three tabs:

**Tab 1: Volumes**
- Table of all volumes that have `"s3"` in `access_protocols`
- Columns: Name, Status, Size, Used, Protocols, FTT, RAID
- Actions: Enable/Disable S3 on existing volumes (PATCH access_protocols)
- Link to CoreSAN page for full volume management

**Tab 2: Credentials**
- Table of all S3 credentials
- Columns: Access Key (visible), User, Display Name, Status, Created
- Actions: Create new, Delete, Disable/Enable
- "Create Credential" button opens `CreateS3CredentialDialog`

**Tab 3: Connection Info**
- S3 endpoint URL per host (shows all hosts with vmm-s3gw running)
- Region name from config
- Quick-start examples: `aws s3 ls --endpoint-url http://host:9000`
- MinIO mc configuration example

### New Dialog: CreateS3CredentialDialog.tsx

Located in `src/components/coresan/CreateS3CredentialDialog.tsx`:

- Form fields: User (dropdown of existing users), Display Name (text)
- On submit: POST to `/api/san/s3/credentials` (cluster) or direct to SAN
- **Critical UX**: Shows the secret key exactly ONCE after creation in a copyable field with warning that it cannot be retrieved again

### Volume Creation Dialog Changes

Modify existing `CreateVolumeDialog.tsx`:

- Add "Access Protocols" multi-select field (checkboxes: FUSE, S3)
- Default: FUSE checked (backward compatible)
- When S3 is selected, show info banner: "S3 access requires vmm-s3gw running on this host"

### API Communication Pattern

- **Standalone mode**: `sanFetch()` to `http://localhost:7443/api/s3/credentials`
- **Cluster mode**: `api.get('/api/san/s3/credentials')` through cluster proxy
- Follows the existing `sanFetch()` / cluster API pattern used for all CoreSAN operations

## File Inventory (Updated)

### New files

| File | Purpose |
|------|---------|
| `apps/vmm-s3gw/` | S3 Gateway binary (Axum, AWS SigV4, UDS client) |
| `apps/vmm-san/src/engine/object_server.rs` | Object Socket listener per volume |
| `apps/vmm-san/src/engine/mgmt_server.rs` | Management Socket (auth, ListBuckets) |
| `apps/vmm-san/src/api/s3.rs` | REST endpoints for S3 credential CRUD (used by cluster proxy) |
| `libs/vmm-core/src/san_object.rs` | Shared protocol structs (ObjectCommand, headers) |
| `libs/vmm-core/src/san_mgmt.rs` | Shared mgmt protocol structs (MgmtCommand, headers) |
| `apps/vmm-cluster/src/api/san_s3.rs` | Cluster proxy routes for S3 credential management |
| `apps/vmm-ui/src/pages/StorageObjectStorage.tsx` | Object Storage management page |
| `apps/vmm-ui/src/components/coresan/CreateS3CredentialDialog.tsx` | S3 credential creation dialog |

### Changed files

| File | Change |
|------|--------|
| `apps/vmm-san/src/db/mod.rs` | `access_protocols` field, `s3_credentials`, `multipart_uploads`, `multipart_parts` tables |
| `apps/vmm-san/src/api/volumes.rs` | `access_protocols` in Create/Update/Response |
| `apps/vmm-san/src/api/mod.rs` | Add S3 credential routes |
| `apps/vmm-san/src/main.rs` | Spawn object_server + mgmt_server |
| `apps/vmm-cluster/src/api/mod.rs` | Add `/api/san/s3/*` routes |
| `apps/vmm-cluster/src/api/san.rs` | Pass through `access_protocols` in volume operations |
| `apps/vmm-ui/src/App.tsx` | Add `/storage/object-storage` route |
| `apps/vmm-ui/src/components/Sidebar.tsx` | Add "Object Storage" nav item |
| `apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx` | Add access_protocols multi-select |
| `apps/vmm-ui/src/api/types.ts` | Add S3Credential interface, update Volume type |
| `Cargo.toml` (workspace) | New member `apps/vmm-s3gw` |

### Untouched

- `engine/disk_server.rs` — zero changes
- `engine/fuse_mount.rs` — zero changes
- `storage/chunk.rs` — zero changes
- Replication engine — zero changes
- Write lease system — zero changes

## Extensibility Pattern

For future protocols (NFS, iSCSI, etc.):

1. Add protocol string to `access_protocols` array (e.g., `"nfs"`)
2. New socket type in vmm-san (`/run/vmm-san/nfs-{vol_id}.sock`)
3. New gateway binary (`apps/vmm-nfsgw/`)
4. vmm-san starts sockets based on `access_protocols` per volume
