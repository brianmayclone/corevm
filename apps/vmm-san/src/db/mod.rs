//! Database schema, migrations, and seed data for CoreSAN.

use rusqlite::Connection;

/// Full database schema for CoreSAN metadata.
const SCHEMA: &str = r#"
-- ═══════════════════════════════════════════════════════════════
-- VOLUMES: logical storage pools with per-volume resilience policy
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS volumes (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    -- Resilience policy (Software-Defined RAID)
    resilience_mode TEXT NOT NULL DEFAULT 'mirror',  -- 'none', 'mirror', 'erasure'
    replica_count   INTEGER NOT NULL DEFAULT 2,       -- copies (for mirror mode)
    -- none:    1 copy, no protection (RAID-0 like, performance mode)
    -- mirror:  N copies on different nodes (RAID-1/10 like)
    -- erasure: data+parity chunks (RAID-5/6 like, future Phase 2)
    stripe_width    INTEGER NOT NULL DEFAULT 0,       -- 0=no striping, >0=stripe across N backends
    sync_mode       TEXT NOT NULL DEFAULT 'async',    -- 'sync' or 'async'
    status          TEXT NOT NULL DEFAULT 'creating',  -- creating, online, degraded, offline
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- BACKENDS: local mountpoints contributing storage to a volume
-- Multiple backends on the same node can belong to the same volume
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS backends (
    id              TEXT PRIMARY KEY,
    volume_id       TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    node_id         TEXT NOT NULL,
    path            TEXT NOT NULL,              -- local mountpoint, e.g. /mnt/data1
    total_bytes     INTEGER NOT NULL DEFAULT 0,
    free_bytes      INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'online',  -- online, degraded, offline, draining
    last_check      TEXT,
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
    rel_path        TEXT NOT NULL,              -- path relative to volume root, e.g. "my-vm/disk.raw"
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    sha256          TEXT NOT NULL DEFAULT '',
    -- Write ownership: which node currently holds the write lease
    write_owner     TEXT NOT NULL DEFAULT '',    -- node_id of current writer, '' = no owner
    write_lease_until TEXT NOT NULL DEFAULT '',  -- ISO timestamp when lease expires
    -- Monotonic version counter — incremented on every write, used for conflict detection
    version         INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(volume_id, rel_path)
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
-- INDEXES
-- ═══════════════════════════════════════════════════════════════

CREATE INDEX IF NOT EXISTS idx_file_map_vol_path ON file_map(volume_id, rel_path);
CREATE INDEX IF NOT EXISTS idx_file_map_write_owner ON file_map(write_owner);
CREATE INDEX IF NOT EXISTS idx_file_replicas_state ON file_replicas(state);
CREATE INDEX IF NOT EXISTS idx_backends_volume ON backends(volume_id);
CREATE INDEX IF NOT EXISTS idx_benchmark_nodes ON benchmark_results(from_node_id, to_node_id);
CREATE INDEX IF NOT EXISTS idx_write_log_seq ON write_log(seq);
CREATE INDEX IF NOT EXISTS idx_write_log_volume ON write_log(volume_id);
"#;

/// Initialize the database: create tables and indexes.
pub fn init(db: &Connection) -> Result<(), String> {
    // Enable WAL mode for better concurrent read/write performance
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .map_err(|e| format!("Failed to set PRAGMA: {}", e))?;

    // Migrate existing databases BEFORE creating schema (so new columns exist for indexes)
    migrate(db);

    db.execute_batch(SCHEMA)
        .map_err(|e| format!("Failed to create schema: {}", e))?;

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
}
