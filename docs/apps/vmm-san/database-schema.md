# CoreSAN Database Schema

CoreSAN uses SQLite in WAL (Write-Ahead Logging) mode with foreign keys enabled. Each node maintains its own independent database at `<data_dir>/coresan.db`.

## Initialization

On startup:
```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
```

## Tables

### volumes

Volume definitions. Synced across all nodes via peer communication.

```sql
CREATE TABLE IF NOT EXISTS volumes (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL UNIQUE,
    resilience_mode   TEXT NOT NULL DEFAULT 'mirror',      -- legacy field
    replica_count     INTEGER NOT NULL DEFAULT 2,           -- legacy field
    stripe_width      INTEGER NOT NULL DEFAULT 0,           -- legacy field
    sync_mode         TEXT NOT NULL DEFAULT 'async',        -- 'async' or 'quorum'
    ftt               INTEGER NOT NULL DEFAULT 1,           -- 0, 1, or 2
    chunk_size_bytes  INTEGER NOT NULL DEFAULT 67108864,    -- 64 MB
    local_raid        TEXT NOT NULL DEFAULT 'stripe',       -- 'stripe', 'mirror', 'stripe_mirror'
    status            TEXT NOT NULL DEFAULT 'creating',     -- 'creating', 'online', 'degraded', 'offline'
    created_at        TEXT NOT NULL DEFAULT (datetime('now'))
);
```

| Column | Description |
|--------|-------------|
| `id` | UUID, primary key |
| `name` | Human-readable name, unique |
| `sync_mode` | Replication timing ('async' = immediate return, 'quorum' = not yet implemented) |
| `ftt` | Failures to tolerate (0 = no replication, 1 = 2 copies, 2 = 3 copies) |
| `chunk_size_bytes` | Fixed chunk size for this volume (default 64 MB) |
| `local_raid` | How chunks are placed across local backends |
| `dedup_enabled` | (Planned) Whether deduplication is enabled for this volume |
| `status` | Current volume state |

### backends

Storage mountpoints (claimed disks). Each node stores its own backends plus cross-registered remote backends.

```sql
CREATE TABLE IF NOT EXISTS backends (
    id               TEXT PRIMARY KEY,
    node_id          TEXT NOT NULL,
    path             TEXT NOT NULL,
    total_bytes      INTEGER NOT NULL DEFAULT 0,
    free_bytes       INTEGER NOT NULL DEFAULT 0,
    status           TEXT NOT NULL DEFAULT 'online',     -- 'online', 'degraded', 'offline', 'draining'
    last_check       TEXT,
    claimed_disk_id  TEXT NOT NULL DEFAULT '',
    UNIQUE(node_id, path)
);
```

| Column | Description |
|--------|-------------|
| `id` | UUID, primary key |
| `node_id` | Which node this backend is on |
| `path` | Filesystem mount path (e.g., `/vmm/san-disks/abc123`) |
| `total_bytes` | Total disk capacity |
| `free_bytes` | Available space |
| `status` | Current health state |
| `claimed_disk_id` | Reference to claimed_disks entry |

### peers

Known peer nodes. Loaded into DashMap on startup for in-memory access.

```sql
CREATE TABLE IF NOT EXISTS peers (
    node_id         TEXT PRIMARY KEY,
    address         TEXT NOT NULL,
    peer_port       INTEGER NOT NULL DEFAULT 7444,
    hostname        TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'connecting',  -- 'connecting', 'online', 'offline'
    last_heartbeat  TEXT,
    joined_at       TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### file_map

Core file metadata. Every file stored in CoreSAN has one entry per volume.

```sql
CREATE TABLE IF NOT EXISTS file_map (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    volume_id            TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    rel_path             TEXT NOT NULL,
    size_bytes           INTEGER NOT NULL DEFAULT 0,
    sha256               TEXT NOT NULL DEFAULT '',
    write_owner          TEXT NOT NULL DEFAULT '',          -- node_id holding write lease
    write_lease_until    TEXT NOT NULL DEFAULT '',          -- lease expiration timestamp
    version              INTEGER NOT NULL DEFAULT 0,        -- incremented on each write
    chunk_count          INTEGER NOT NULL DEFAULT 0,
    protection_status    TEXT NOT NULL DEFAULT 'unprotected', -- 'protected', 'degraded', 'unprotected'
    ownership_epoch      INTEGER NOT NULL DEFAULT 0,        -- incremented on ownership change
    ownership_tick       INTEGER NOT NULL DEFAULT 0,        -- incremented per write within epoch
    created_at           TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at           TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(volume_id, rel_path)
);
```

| Column | Description |
|--------|-------------|
| `write_owner` | Node currently holding exclusive write lock |
| `write_lease_until` | When the lease expires (30s from acquisition) |
| `version` | Monotonically increasing write counter |
| `protection_status` | Whether FTT is met for all chunks |
| `ownership_epoch` | Detects ownership changes between nodes |
| `ownership_tick` | Write counter within same ownership |

### file_chunks

Chunk metadata for each file. A file is divided into fixed-size chunks.

```sql
CREATE TABLE IF NOT EXISTS file_chunks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id       INTEGER NOT NULL REFERENCES file_map(id) ON DELETE CASCADE,
    chunk_index   INTEGER NOT NULL,
    offset_bytes  INTEGER NOT NULL,
    size_bytes    INTEGER NOT NULL,
    sha256        TEXT NOT NULL DEFAULT '',
    UNIQUE(file_id, chunk_index)
);
```

### chunk_replicas

Tracks where each chunk is physically stored (which backend, which node).

```sql
CREATE TABLE IF NOT EXISTS chunk_replicas (
    chunk_id   INTEGER NOT NULL REFERENCES file_chunks(id) ON DELETE CASCADE,
    backend_id TEXT NOT NULL,
    node_id    TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'syncing',  -- 'syncing', 'synced', 'stale', 'error'
    synced_at  TEXT,
    PRIMARY KEY (chunk_id, backend_id, node_id)
);
```

**Note:** The PRIMARY KEY is `(chunk_id, backend_id, node_id)` — not just `(chunk_id, backend_id)`. There is no foreign key on `backend_id` because remote replicas are tracked with an empty `backend_id` (the sender does not know which backend the remote node used). The `chunk_replicas` table is the authoritative source for replica tracking; `file_replicas` is legacy.

**Planned:** A `content_sha256` column is planned for deduplication support.

| State | Meaning |
|-------|---------|
| `syncing` | Transfer in progress |
| `synced` | Data matches latest version |
| `stale` | Newer version exists elsewhere |
| `error` | Corruption detected or disk failure |

### file_replicas (Legacy)

File-level replica tracking. This table is legacy — `chunk_replicas` is the authoritative source for replica state. Retained for backward compatibility.

```sql
CREATE TABLE IF NOT EXISTS file_replicas (
    file_id         INTEGER NOT NULL REFERENCES file_map(id) ON DELETE CASCADE,
    backend_id      TEXT NOT NULL REFERENCES backends(id) ON DELETE CASCADE,
    state           TEXT NOT NULL DEFAULT 'syncing',  -- 'syncing', 'synced', 'stale', 'error'
    replica_version INTEGER NOT NULL DEFAULT 0,
    synced_at       TEXT,
    PRIMARY KEY (file_id, backend_id)
);
```

### write_log

Ordered history of write operations. Used for catch-up replication and conflict detection.

```sql
CREATE TABLE IF NOT EXISTS write_log (
    seq               INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id           INTEGER NOT NULL REFERENCES file_map(id) ON DELETE CASCADE,
    volume_id         TEXT NOT NULL,
    rel_path          TEXT NOT NULL,
    version           INTEGER NOT NULL,
    writer_node_id    TEXT NOT NULL,
    size_bytes        INTEGER NOT NULL DEFAULT 0,
    sha256            TEXT NOT NULL DEFAULT '',
    ownership_epoch   INTEGER NOT NULL DEFAULT 0,
    ownership_tick    INTEGER NOT NULL DEFAULT 0,
    written_at        TEXT NOT NULL DEFAULT (datetime('now'))
);
```

Entries older than 1 hour are automatically cleaned up every 300 seconds.

### benchmark_results

Network performance measurements between node pairs.

```sql
CREATE TABLE IF NOT EXISTS benchmark_results (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    from_node_id    TEXT NOT NULL,
    to_node_id      TEXT NOT NULL,
    bandwidth_mbps  REAL NOT NULL DEFAULT 0,
    latency_us      REAL NOT NULL DEFAULT 0,
    jitter_us       REAL NOT NULL DEFAULT 0,
    packet_loss_pct REAL NOT NULL DEFAULT 0,
    test_size_bytes INTEGER NOT NULL DEFAULT 0,
    measured_at     TEXT NOT NULL DEFAULT (datetime('now'))
);
```

Results older than 24 hours are automatically cleaned up.

### integrity_log

Checksum verification results.

```sql
CREATE TABLE IF NOT EXISTS integrity_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id         INTEGER NOT NULL REFERENCES file_map(id),
    backend_id      TEXT NOT NULL,
    expected_sha256 TEXT NOT NULL,
    actual_sha256   TEXT NOT NULL,
    passed          INTEGER NOT NULL,  -- 1 = OK, 0 = corruption detected
    checked_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### claimed_disks

Physical disk claim records.

```sql
CREATE TABLE IF NOT EXISTS claimed_disks (
    id           TEXT PRIMARY KEY,
    device_path  TEXT NOT NULL UNIQUE,    -- /dev/sdb
    device_uuid  TEXT NOT NULL DEFAULT '',
    mount_path   TEXT NOT NULL UNIQUE,    -- /vmm/san-disks/<uuid>
    fs_type      TEXT NOT NULL DEFAULT 'ext4',
    model        TEXT NOT NULL DEFAULT '',
    serial       TEXT NOT NULL DEFAULT '',
    size_bytes   INTEGER NOT NULL DEFAULT 0,
    status       TEXT NOT NULL DEFAULT 'formatting',  -- 'formatting', 'mounted', 'error', 'released'
    backend_id   TEXT NOT NULL DEFAULT '',
    claimed_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
```

| Status | Meaning |
|--------|---------|
| `formatting` | Disk is being partitioned and formatted |
| `mounted` | Disk is active and serving as a backend |
| `error` | Disk disappeared or has I/O errors |
| `released` | Disk has been gracefully released (drained) |

### smart_data

S.M.A.R.T. disk health data collected by the `smart_monitor` engine.

```sql
CREATE TABLE IF NOT EXISTS smart_data (
    device_path         TEXT PRIMARY KEY,
    health_passed       INTEGER NOT NULL DEFAULT 1,   -- 1 = passed, 0 = failed
    transport           TEXT NOT NULL DEFAULT '',       -- 'SATA', 'SAS', 'NVMe'
    power_on_hours      INTEGER NOT NULL DEFAULT 0,
    temperature         INTEGER NOT NULL DEFAULT 0,
    reallocated_sectors INTEGER NOT NULL DEFAULT 0,
    pending_sectors     INTEGER NOT NULL DEFAULT 0,
    media_errors        INTEGER NOT NULL DEFAULT 0,
    updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
);
```

| Column | Description |
|--------|-------------|
| `device_path` | Primary key, e.g., `/dev/sdb` |
| `health_passed` | Overall SMART health assessment |
| `transport` | Disk transport type (SATA, SAS, NVMe) |
| `power_on_hours` | Total hours the disk has been powered on |
| `temperature` | Current temperature in Celsius |
| `reallocated_sectors` | Count of reallocated sectors (bad sectors remapped) |
| `pending_sectors` | Sectors pending reallocation |
| `media_errors` | NVMe media/data integrity errors |

### node_settings

Key-value store for persistent node configuration.

```sql
CREATE TABLE IF NOT EXISTS node_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

Currently stores:
- `node_id` — this node's UUID (generated on first startup)

## Indexes

```sql
CREATE INDEX IF NOT EXISTS idx_file_map_vol_path ON file_map(volume_id, rel_path);
CREATE INDEX IF NOT EXISTS idx_file_map_write_owner ON file_map(write_owner);
CREATE INDEX IF NOT EXISTS idx_file_replicas_state ON file_replicas(state);
CREATE INDEX IF NOT EXISTS idx_file_replicas_backend ON file_replicas(backend_id);
CREATE INDEX IF NOT EXISTS idx_backends_node ON backends(node_id);
CREATE INDEX IF NOT EXISTS idx_benchmark_nodes ON benchmark_results(from_node_id, to_node_id);
CREATE INDEX IF NOT EXISTS idx_write_log_seq ON write_log(seq);
CREATE INDEX IF NOT EXISTS idx_write_log_volume ON write_log(volume_id);
CREATE INDEX IF NOT EXISTS idx_claimed_disks_status ON claimed_disks(status);
CREATE INDEX IF NOT EXISTS idx_file_chunks_file ON file_chunks(file_id);
CREATE INDEX IF NOT EXISTS idx_chunk_replicas_chunk ON chunk_replicas(chunk_id);
CREATE INDEX IF NOT EXISTS idx_chunk_replicas_backend ON chunk_replicas(backend_id);
CREATE INDEX IF NOT EXISTS idx_chunk_replicas_node ON chunk_replicas(node_id);
```

## Entity Relationships

```
volumes ──1:N──► backends
volumes ──1:N──► file_map
file_map ──1:N──► file_chunks ──1:N──► chunk_replicas (authoritative)
file_map ──1:N──► file_replicas ──► backends (legacy)
file_map ──1:N──► write_log
file_map ──1:N──► integrity_log
claimed_disks ──1:1──► backends (via backend_id / claimed_disk_id)
claimed_disks ──1:1──► smart_data (via device_path)
```

## Database Backup

The `db_mirror` engine (60-second interval) copies the database to each claimed disk backend:

```
/vmm/san-disks/<uuid>/coresan-backup.db
```

On startup, if the primary database is missing, CoreSAN attempts to restore from the most recent backup found on any claimed disk.
