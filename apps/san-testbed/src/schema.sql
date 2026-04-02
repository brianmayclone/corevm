-- CoreSAN schema — replicated from vmm-san/src/db/mod.rs
-- Must be kept in sync manually (testbed only, not production).

CREATE TABLE IF NOT EXISTS volumes (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    resilience_mode TEXT NOT NULL DEFAULT 'mirror',
    replica_count   INTEGER NOT NULL DEFAULT 2,
    stripe_width    INTEGER NOT NULL DEFAULT 0,
    sync_mode       TEXT NOT NULL DEFAULT 'async',
    ftt             INTEGER NOT NULL DEFAULT 1,
    chunk_size_bytes INTEGER NOT NULL DEFAULT 4194304,
    local_raid      TEXT NOT NULL DEFAULT 'stripe',
    max_size_bytes  INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'creating',
    access_protocols TEXT NOT NULL DEFAULT '["fuse"]',
    dedup           INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS backends (
    id              TEXT PRIMARY KEY,
    node_id         TEXT NOT NULL,
    path            TEXT NOT NULL,
    total_bytes     INTEGER NOT NULL DEFAULT 0,
    free_bytes      INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'online',
    last_check      TEXT,
    claimed_disk_id TEXT NOT NULL DEFAULT '',
    UNIQUE(node_id, path)
);

CREATE TABLE IF NOT EXISTS peers (
    node_id         TEXT PRIMARY KEY,
    address         TEXT NOT NULL,
    peer_port       INTEGER NOT NULL DEFAULT 7444,
    hostname        TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'connecting',
    last_heartbeat  TEXT,
    joined_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS file_map (
    id              INTEGER PRIMARY KEY,
    volume_id       TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    rel_path        TEXT NOT NULL,
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    sha256          TEXT NOT NULL DEFAULT '',
    write_owner     TEXT NOT NULL DEFAULT '',
    write_lease_until TEXT NOT NULL DEFAULT '',
    version         INTEGER NOT NULL DEFAULT 0,
    chunk_count     INTEGER NOT NULL DEFAULT 0,
    protection_status TEXT NOT NULL DEFAULT 'unprotected',
    ownership_epoch INTEGER NOT NULL DEFAULT 0,
    ownership_tick  INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(volume_id, rel_path)
);

CREATE TABLE IF NOT EXISTS file_chunks (
    id              INTEGER PRIMARY KEY,
    file_id         INTEGER NOT NULL REFERENCES file_map(id) ON DELETE CASCADE,
    chunk_index     INTEGER NOT NULL,
    offset_bytes    INTEGER NOT NULL,
    size_bytes      INTEGER NOT NULL,
    sha256          TEXT NOT NULL DEFAULT '',
    dedup_sha256    TEXT,
    UNIQUE(file_id, chunk_index)
);

CREATE TABLE IF NOT EXISTS chunk_replicas (
    chunk_id        INTEGER NOT NULL REFERENCES file_chunks(id) ON DELETE CASCADE,
    backend_id      TEXT NOT NULL DEFAULT '',
    node_id         TEXT NOT NULL,
    state           TEXT NOT NULL DEFAULT 'syncing',
    synced_at       TEXT,
    PRIMARY KEY (chunk_id, backend_id, node_id)
);

CREATE TABLE IF NOT EXISTS file_replicas (
    file_id         INTEGER NOT NULL REFERENCES file_map(id) ON DELETE CASCADE,
    backend_id      TEXT NOT NULL REFERENCES backends(id) ON DELETE CASCADE,
    state           TEXT NOT NULL DEFAULT 'syncing',
    replica_version INTEGER NOT NULL DEFAULT 0,
    synced_at       TEXT,
    PRIMARY KEY (file_id, backend_id)
);

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

CREATE TABLE IF NOT EXISTS integrity_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id         INTEGER NOT NULL REFERENCES file_map(id),
    backend_id      TEXT NOT NULL,
    expected_sha256 TEXT NOT NULL,
    actual_sha256   TEXT NOT NULL,
    passed          INTEGER NOT NULL,
    checked_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS claimed_disks (
    id              TEXT PRIMARY KEY,
    device_path     TEXT NOT NULL UNIQUE,
    device_uuid     TEXT NOT NULL DEFAULT '',
    mount_path      TEXT NOT NULL UNIQUE,
    fs_type         TEXT NOT NULL DEFAULT 'ext4',
    model           TEXT NOT NULL DEFAULT '',
    serial          TEXT NOT NULL DEFAULT '',
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'formatting',
    backend_id      TEXT NOT NULL DEFAULT '',
    claimed_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS s3_credentials (
    id              TEXT PRIMARY KEY,
    access_key      TEXT NOT NULL UNIQUE,
    secret_key_enc  TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    display_name    TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'active',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at      TEXT
);

CREATE TABLE IF NOT EXISTS multipart_uploads (
    upload_id       TEXT PRIMARY KEY,
    volume_id       TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    object_key      TEXT NOT NULL,
    created_by      TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    status          TEXT NOT NULL DEFAULT 'active'
);

CREATE TABLE IF NOT EXISTS multipart_parts (
    upload_id       TEXT NOT NULL REFERENCES multipart_uploads(upload_id) ON DELETE CASCADE,
    part_number     INTEGER NOT NULL,
    size_bytes      INTEGER NOT NULL,
    etag            TEXT NOT NULL,
    backend_path    TEXT NOT NULL,
    PRIMARY KEY (upload_id, part_number)
);

CREATE TABLE IF NOT EXISTS iscsi_acls (
    id              TEXT PRIMARY KEY,
    volume_id       TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    initiator_iqn   TEXT NOT NULL,
    comment         TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(volume_id, initiator_iqn)
);

CREATE TABLE IF NOT EXISTS smart_data (
    device_path         TEXT PRIMARY KEY,
    supported           INTEGER NOT NULL DEFAULT 0,
    health_passed       INTEGER,
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

CREATE TABLE IF NOT EXISTS dedup_store (
    sha256       TEXT NOT NULL,
    volume_id    TEXT NOT NULL,
    backend_id   TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    ref_count    INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (sha256, volume_id, backend_id)
);

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
CREATE INDEX IF NOT EXISTS idx_s3_credentials_access_key ON s3_credentials(access_key);
CREATE INDEX IF NOT EXISTS idx_s3_credentials_user ON s3_credentials(user_id);
CREATE INDEX IF NOT EXISTS idx_multipart_uploads_volume ON multipart_uploads(volume_id);
CREATE INDEX IF NOT EXISTS idx_multipart_uploads_status ON multipart_uploads(status);
CREATE INDEX IF NOT EXISTS idx_iscsi_acls_volume ON iscsi_acls(volume_id);
CREATE INDEX IF NOT EXISTS idx_iscsi_acls_iqn ON iscsi_acls(initiator_iqn);
CREATE INDEX IF NOT EXISTS idx_dedup_store_volume ON dedup_store(volume_id);
CREATE INDEX IF NOT EXISTS idx_dedup_store_ref ON dedup_store(volume_id, ref_count);
