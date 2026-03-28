//! Database schema, migrations, helpers, and seed data for CoreSAN.

pub mod helpers;

pub use helpers::{DbResult, DbError, DbContext, db_transaction, db_exec};

use rusqlite::Connection;

/// Full database schema for CoreSAN metadata.
const SCHEMA: &str = r#"
-- ═══════════════════════════════════════════════════════════════
-- VOLUMES: logical storage pools with per-volume resilience policy
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS volumes (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    -- Legacy fields (kept for migration compatibility)
    resilience_mode TEXT NOT NULL DEFAULT 'mirror',
    replica_count   INTEGER NOT NULL DEFAULT 2,
    stripe_width    INTEGER NOT NULL DEFAULT 0,
    sync_mode       TEXT NOT NULL DEFAULT 'async',
    -- FTT-based resilience (new model)
    ftt             INTEGER NOT NULL DEFAULT 1,       -- Failures To Tolerate: 0, 1, 2
    chunk_size_bytes INTEGER NOT NULL DEFAULT 67108864, -- 64MB default
    local_raid      TEXT NOT NULL DEFAULT 'stripe',   -- stripe, mirror, stripe_mirror
    max_size_bytes  INTEGER NOT NULL DEFAULT 0,       -- 0 = unlimited (legacy)
    -- Status
    status          TEXT NOT NULL DEFAULT 'creating',
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- BACKENDS: local mountpoints contributing storage to a volume
-- Multiple backends on the same node can belong to the same volume
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS backends (
    id              TEXT PRIMARY KEY,
    node_id         TEXT NOT NULL,
    path            TEXT NOT NULL,              -- mount point of a claimed disk
    total_bytes     INTEGER NOT NULL DEFAULT 0,
    free_bytes      INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'online',  -- online, degraded, offline, draining
    last_check      TEXT,
    claimed_disk_id TEXT NOT NULL DEFAULT '',    -- link to claimed_disks table
    UNIQUE(node_id, path)
);

-- ═══════════════════════════════════════════════════════════════
-- PEERS: other CoreSAN nodes in this storage cluster
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS peers (
    node_id         TEXT PRIMARY KEY,
    address         TEXT NOT NULL,              -- e.g. "http://192.168.1.10:7443"
    peer_port       INTEGER NOT NULL DEFAULT 7444,
    hostname        TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'connecting',  -- connecting, online, offline
    last_heartbeat  TEXT,
    joined_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- FILE_MAP: metadata index of every file in the SAN
-- Tracks path, size, checksum, write ownership, and version
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS file_map (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    volume_id       TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    rel_path        TEXT NOT NULL,
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    sha256          TEXT NOT NULL DEFAULT '',
    -- Write ownership
    write_owner     TEXT NOT NULL DEFAULT '',
    write_lease_until TEXT NOT NULL DEFAULT '',
    -- Version tracking
    version         INTEGER NOT NULL DEFAULT 0,
    -- Chunk tracking
    chunk_count     INTEGER NOT NULL DEFAULT 0,
    protection_status TEXT NOT NULL DEFAULT 'unprotected', -- protected, degraded, unprotected
    -- Split-brain conflict resolution
    ownership_epoch INTEGER NOT NULL DEFAULT 0,
    ownership_tick  INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(volume_id, rel_path)
);

-- ═══════════════════════════════════════════════════════════════
-- FILE_CHUNKS: individual chunks of a file, distributed across backends
-- Each file is split into fixed-size chunks (default 64MB)
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS file_chunks (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id         INTEGER NOT NULL REFERENCES file_map(id) ON DELETE CASCADE,
    chunk_index     INTEGER NOT NULL,           -- 0, 1, 2, ...
    offset_bytes    INTEGER NOT NULL,           -- byte offset in the file
    size_bytes      INTEGER NOT NULL,           -- actual size (last chunk may be smaller)
    sha256          TEXT NOT NULL DEFAULT '',    -- per-chunk checksum
    UNIQUE(file_id, chunk_index)
);

-- ═══════════════════════════════════════════════════════════════
-- CHUNK_REPLICAS: where each chunk physically lives
-- Tracks both local disk placement and cross-node replication
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS chunk_replicas (
    chunk_id        INTEGER NOT NULL REFERENCES file_chunks(id) ON DELETE CASCADE,
    backend_id      TEXT NOT NULL DEFAULT '',   -- local backend id (empty for remote-tracked replicas)
    node_id         TEXT NOT NULL,              -- which host holds this replica
    state           TEXT NOT NULL DEFAULT 'syncing', -- syncing, synced, stale, error
    synced_at       TEXT,
    PRIMARY KEY (chunk_id, backend_id, node_id)
);

-- ═══════════════════════════════════════════════════════════════
-- FILE_REPLICAS: which backend holds which copy of each file
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS file_replicas (
    file_id         INTEGER NOT NULL REFERENCES file_map(id) ON DELETE CASCADE,
    backend_id      TEXT NOT NULL REFERENCES backends(id) ON DELETE CASCADE,
    state           TEXT NOT NULL DEFAULT 'syncing',  -- syncing, synced, stale, error
    -- Version of the data this replica holds (matches file_map.version when synced)
    replica_version INTEGER NOT NULL DEFAULT 0,
    synced_at       TEXT,
    PRIMARY KEY (file_id, backend_id)
);

-- ═══════════════════════════════════════════════════════════════
-- WRITE_LOG: ordered log of write events for fast push replication
-- Peers poll this log to catch up on missed writes
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS write_log (
    seq             INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id         INTEGER NOT NULL REFERENCES file_map(id) ON DELETE CASCADE,
    volume_id       TEXT NOT NULL,
    rel_path        TEXT NOT NULL,
    version         INTEGER NOT NULL,
    writer_node_id  TEXT NOT NULL,
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    sha256          TEXT NOT NULL DEFAULT '',
    ownership_epoch INTEGER NOT NULL DEFAULT 0,
    ownership_tick  INTEGER NOT NULL DEFAULT 0,
    written_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- BENCHMARK_RESULTS: network performance data between peers
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS benchmark_results (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    from_node_id    TEXT NOT NULL,
    to_node_id      TEXT NOT NULL,
    bandwidth_mbps  REAL NOT NULL DEFAULT 0,    -- throughput in Mbit/s
    latency_us      REAL NOT NULL DEFAULT 0,    -- latency in microseconds
    jitter_us       REAL NOT NULL DEFAULT 0,    -- jitter in microseconds
    packet_loss_pct REAL NOT NULL DEFAULT 0,    -- packet loss percentage
    test_size_bytes INTEGER NOT NULL DEFAULT 0,
    measured_at     TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- INTEGRITY_LOG: checksum verification audit trail
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS integrity_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id         INTEGER NOT NULL REFERENCES file_map(id),
    backend_id      TEXT NOT NULL,
    expected_sha256 TEXT NOT NULL,
    actual_sha256   TEXT NOT NULL,
    passed          INTEGER NOT NULL,           -- 1=OK, 0=corrupt
    checked_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- CLAIMED_DISKS: physical disks managed by CoreSAN
-- CoreSAN claims whole disks, formats them, and uses them as backends
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS claimed_disks (
    id              TEXT PRIMARY KEY,
    device_path     TEXT NOT NULL UNIQUE,    -- /dev/sdb, /dev/nvme1n1
    device_uuid     TEXT NOT NULL DEFAULT '',-- filesystem UUID after formatting
    mount_path      TEXT NOT NULL UNIQUE,    -- /vmm/san-disks/<uuid>
    fs_type         TEXT NOT NULL DEFAULT 'ext4',
    model           TEXT NOT NULL DEFAULT '',
    serial          TEXT NOT NULL DEFAULT '',
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'formatting', -- formatting, mounted, error, released
    backend_id      TEXT NOT NULL DEFAULT '', -- linked backend in backends table
    -- Disks belong to the NODE POOL, not to a specific volume.
    -- All volumes share all disks on a node.
    claimed_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- INDEXES
-- ═══════════════════════════════════════════════════════════════

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

-- ═══════════════════════════════════════════════════════════════
-- SMART_DATA: S.M.A.R.T. disk health metrics per physical device
-- Collected every 5 minutes by the smart_monitor engine
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS smart_data (
    device_path         TEXT PRIMARY KEY,
    supported           INTEGER NOT NULL DEFAULT 0,
    health_passed       INTEGER,            -- 1=PASSED, 0=FAILED, NULL=unknown
    transport           TEXT NOT NULL DEFAULT 'unknown',
    power_on_hours      INTEGER,
    temperature_c       INTEGER,
    reallocated_sectors INTEGER,
    pending_sectors     INTEGER,
    uncorrectable_sectors INTEGER,
    wear_leveling_pct   INTEGER,
    media_errors        INTEGER,
    percentage_used     INTEGER,
    model               TEXT NOT NULL DEFAULT '',
    serial              TEXT NOT NULL DEFAULT '',
    firmware            TEXT NOT NULL DEFAULT '',
    raw_json            TEXT,
    collected_at        TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;

/// Initialize the database: create tables and indexes.
pub fn init(db: &Connection) -> Result<(), String> {
    // Enable WAL mode for better concurrent read/write performance
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .map_err(|e| format!("Failed to set PRAGMA: {}", e))?;

    // Create schema first (CREATE TABLE IF NOT EXISTS), then migrate existing DBs
    db.execute_batch(SCHEMA)
        .map_err(|e| format!("Failed to create schema: {}", e))?;

    // Migrate existing databases (adds columns that may not exist in older schemas)
    migrate(db);

    Ok(())
}

/// Apply schema migrations for existing databases.
fn migrate(db: &Connection) {
    // Add write_owner column if missing
    db.execute_batch(
        "ALTER TABLE file_map ADD COLUMN write_owner TEXT NOT NULL DEFAULT '';"
    ).ok();
    db.execute_batch(
        "ALTER TABLE file_map ADD COLUMN write_lease_until TEXT NOT NULL DEFAULT '';"
    ).ok();
    db.execute_batch(
        "ALTER TABLE file_map ADD COLUMN version INTEGER NOT NULL DEFAULT 0;"
    ).ok();
    db.execute_batch(
        "ALTER TABLE file_replicas ADD COLUMN replica_version INTEGER NOT NULL DEFAULT 0;"
    ).ok();
    // Claimed disk reference on backends
    db.execute_batch(
        "ALTER TABLE backends ADD COLUMN claimed_disk_id TEXT NOT NULL DEFAULT '';"
    ).ok();
    // FTT + chunk fields on volumes
    db.execute_batch("ALTER TABLE volumes ADD COLUMN ftt INTEGER NOT NULL DEFAULT 1;").ok();
    db.execute_batch("ALTER TABLE volumes ADD COLUMN chunk_size_bytes INTEGER NOT NULL DEFAULT 67108864;").ok();
    db.execute_batch("ALTER TABLE volumes ADD COLUMN local_raid TEXT NOT NULL DEFAULT 'stripe';").ok();
    // Chunk tracking on file_map
    db.execute_batch("ALTER TABLE file_map ADD COLUMN chunk_count INTEGER NOT NULL DEFAULT 0;").ok();
    db.execute_batch("ALTER TABLE file_map ADD COLUMN protection_status TEXT NOT NULL DEFAULT 'unprotected';").ok();

    // Ownership ticks for split-brain conflict resolution
    db.execute_batch("ALTER TABLE file_map ADD COLUMN ownership_epoch INTEGER NOT NULL DEFAULT 0;").ok();
    db.execute_batch("ALTER TABLE file_map ADD COLUMN ownership_tick INTEGER NOT NULL DEFAULT 0;").ok();
    db.execute_batch("ALTER TABLE write_log ADD COLUMN ownership_epoch INTEGER NOT NULL DEFAULT 0;").ok();
    db.execute_batch("ALTER TABLE write_log ADD COLUMN ownership_tick INTEGER NOT NULL DEFAULT 0;").ok();

    // Volume sync mode policy
    db.execute_batch("ALTER TABLE volumes ADD COLUMN sync_mode TEXT NOT NULL DEFAULT 'async';").ok();
    // Volume size limit
    db.execute_batch("ALTER TABLE volumes ADD COLUMN max_size_bytes INTEGER NOT NULL DEFAULT 0;").ok();

    // Migrate chunk_replicas: remove FK on backend_id, change PK to (chunk_id, node_id)
    // SQLite can't ALTER constraints, so we recreate the table if the old schema has
    // backend_id as part of the PK. We detect this by trying to insert a test row
    // with empty backend_id — if it fails with FK error, we need to migrate.
    migrate_chunk_replicas(db);
}

/// Recreate chunk_replicas: remove FK on backend_id, use PK (chunk_id, backend_id, node_id).
/// This allows: multiple local backends per node (mirror) AND remote-tracking with empty backend_id.
fn migrate_chunk_replicas(db: &Connection) {
    let needs_migrate = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='chunk_replicas'",
        [], |row| row.get::<_, String>(0),
    ).map(|sql| {
        // Migrate if old schema has FK on backend_id or wrong PK
        sql.contains("REFERENCES backends(id)") ||
        sql.contains("PRIMARY KEY (chunk_id, backend_id)") && !sql.contains("PRIMARY KEY (chunk_id, backend_id, node_id)") ||
        sql.contains("PRIMARY KEY (chunk_id, node_id)")
    }).unwrap_or(false);

    if !needs_migrate {
        return;
    }

    tracing::info!("Migrating chunk_replicas → PK (chunk_id, backend_id, node_id), no FK on backend_id");

    db.execute_batch("
        CREATE TABLE IF NOT EXISTS chunk_replicas_new (
            chunk_id        INTEGER NOT NULL REFERENCES file_chunks(id) ON DELETE CASCADE,
            backend_id      TEXT NOT NULL DEFAULT '',
            node_id         TEXT NOT NULL,
            state           TEXT NOT NULL DEFAULT 'syncing',
            synced_at       TEXT,
            PRIMARY KEY (chunk_id, backend_id, node_id)
        );
        INSERT OR IGNORE INTO chunk_replicas_new (chunk_id, backend_id, node_id, state, synced_at)
            SELECT chunk_id, backend_id, node_id, state, synced_at FROM chunk_replicas;
        DROP TABLE chunk_replicas;
        ALTER TABLE chunk_replicas_new RENAME TO chunk_replicas;
    ").ok();

    db.execute_batch("
        CREATE INDEX IF NOT EXISTS idx_chunk_replicas_chunk ON chunk_replicas(chunk_id);
        CREATE INDEX IF NOT EXISTS idx_chunk_replicas_backend ON chunk_replicas(backend_id);
        CREATE INDEX IF NOT EXISTS idx_chunk_replicas_node ON chunk_replicas(node_id);
    ").ok();
}
