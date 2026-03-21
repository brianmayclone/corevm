//! Database schema, migrations, and seed data.
//!
//! vmm-cluster owns ALL state: VMs, hosts, datastores, users, permissions.
//! This is the single source of truth — nodes are execution agents.

use rusqlite::Connection;

/// Full database schema — all tables the cluster authority needs.
const SCHEMA: &str = r#"
-- ═══════════════════════════════════════════════════════════════
-- CLUSTER TOPOLOGY
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS clusters (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    description     TEXT NOT NULL DEFAULT '',
    -- DRS settings
    drs_enabled     INTEGER NOT NULL DEFAULT 1,
    -- HA settings
    ha_enabled      INTEGER NOT NULL DEFAULT 1,
    ha_host_monitoring  INTEGER NOT NULL DEFAULT 1,
    ha_vm_restart_priority TEXT NOT NULL DEFAULT 'medium',
    ha_admission_control INTEGER NOT NULL DEFAULT 1,
    ha_failover_hosts INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS hosts (
    id              TEXT PRIMARY KEY,
    hostname        TEXT NOT NULL,
    address         TEXT NOT NULL UNIQUE,
    cluster_id      TEXT NOT NULL REFERENCES clusters(id),
    agent_token     TEXT NOT NULL,
    -- Hardware (updated via heartbeat)
    cpu_model       TEXT NOT NULL DEFAULT '',
    cpu_cores       INTEGER NOT NULL DEFAULT 0,
    cpu_threads     INTEGER NOT NULL DEFAULT 0,
    total_ram_mb    INTEGER NOT NULL DEFAULT 0,
    hw_virtualization INTEGER NOT NULL DEFAULT 0,
    -- Live status (updated via heartbeat)
    free_ram_mb     INTEGER NOT NULL DEFAULT 0,
    cpu_usage_pct   REAL NOT NULL DEFAULT 0.0,
    -- State
    status          TEXT NOT NULL DEFAULT 'connecting',
    maintenance_mode INTEGER NOT NULL DEFAULT 0,
    connection_state TEXT NOT NULL DEFAULT 'disconnected',
    last_heartbeat  TEXT,
    version         TEXT NOT NULL DEFAULT '',
    registered_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- VIRTUAL MACHINES (Single Source of Truth — NOT on the nodes!)
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS vms (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    -- Placement
    cluster_id      TEXT NOT NULL REFERENCES clusters(id),
    host_id         TEXT REFERENCES hosts(id),
    -- Configuration (lives HERE, pushed to nodes)
    config_json     TEXT NOT NULL,
    -- State
    state           TEXT NOT NULL DEFAULT 'stopped',
    -- HA settings per VM
    ha_protected    INTEGER NOT NULL DEFAULT 1,
    ha_restart_priority TEXT NOT NULL DEFAULT 'medium',
    -- DRS
    drs_automation  TEXT NOT NULL DEFAULT 'manual',
    -- Resource group
    resource_group_id INTEGER REFERENCES resource_groups(id),
    -- Owner
    owner_id        INTEGER REFERENCES users(id),
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- STORAGE (Cluster-wide Datastores)
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS datastores (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    store_type      TEXT NOT NULL,
    mount_source    TEXT NOT NULL DEFAULT '',
    mount_opts      TEXT NOT NULL DEFAULT '',
    mount_path      TEXT NOT NULL,
    cluster_id      TEXT NOT NULL REFERENCES clusters(id),
    total_bytes     INTEGER NOT NULL DEFAULT 0,
    free_bytes      INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'creating',
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS datastore_hosts (
    datastore_id    TEXT NOT NULL REFERENCES datastores(id) ON DELETE CASCADE,
    host_id         TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    mounted         INTEGER NOT NULL DEFAULT 0,
    mount_status    TEXT NOT NULL DEFAULT 'pending',
    total_bytes     INTEGER NOT NULL DEFAULT 0,
    free_bytes      INTEGER NOT NULL DEFAULT 0,
    last_check      TEXT,
    PRIMARY KEY (datastore_id, host_id)
);

CREATE TABLE IF NOT EXISTS disk_images (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    datastore_id    TEXT NOT NULL REFERENCES datastores(id),
    path            TEXT NOT NULL,
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    format          TEXT NOT NULL DEFAULT 'raw',
    vm_id           TEXT REFERENCES vms(id) ON DELETE SET NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS isos (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    datastore_id    TEXT NOT NULL REFERENCES datastores(id),
    path            TEXT NOT NULL,
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    uploaded_at     TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- USERS & PERMISSIONS (centralized, not per-node)
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS users (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    username        TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    role            TEXT NOT NULL DEFAULT 'operator',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS groups (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    role            TEXT NOT NULL DEFAULT 'viewer',
    description     TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS group_members (
    group_id        INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);

CREATE TABLE IF NOT EXISTS resource_groups (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    description     TEXT NOT NULL DEFAULT '',
    is_default      INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS resource_group_permissions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    resource_group_id INTEGER NOT NULL REFERENCES resource_groups(id) ON DELETE CASCADE,
    group_id        INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    permissions     TEXT NOT NULL DEFAULT '',
    UNIQUE(resource_group_id, group_id)
);

-- ═══════════════════════════════════════════════════════════════
-- OPERATIONS & EVENTS
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    task_type       TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'queued',
    progress_pct    INTEGER NOT NULL DEFAULT 0,
    target_type     TEXT NOT NULL,
    target_id       TEXT NOT NULL,
    initiated_by    INTEGER REFERENCES users(id),
    details_json    TEXT,
    error           TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    started_at      TEXT,
    completed_at    TEXT
);

CREATE TABLE IF NOT EXISTS audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id         INTEGER REFERENCES users(id),
    action          TEXT NOT NULL,
    target_type     TEXT,
    target_id       TEXT,
    details         TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    severity        TEXT NOT NULL DEFAULT 'info',
    category        TEXT NOT NULL,
    message         TEXT NOT NULL,
    target_type     TEXT,
    target_id       TEXT,
    host_id         TEXT REFERENCES hosts(id),
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS drs_recommendations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id      TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    vm_id           TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    source_host_id  TEXT NOT NULL REFERENCES hosts(id),
    target_host_id  TEXT NOT NULL REFERENCES hosts(id),
    reason          TEXT NOT NULL,
    priority        TEXT NOT NULL DEFAULT 'medium',
    status          TEXT NOT NULL DEFAULT 'pending',
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS migrations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    vm_id           TEXT NOT NULL,
    vm_name         TEXT NOT NULL,
    source_host_id  TEXT NOT NULL REFERENCES hosts(id),
    target_host_id  TEXT NOT NULL REFERENCES hosts(id),
    migration_type  TEXT NOT NULL DEFAULT 'cold',
    reason          TEXT NOT NULL DEFAULT 'manual',
    status          TEXT NOT NULL DEFAULT 'pending',
    initiated_by    INTEGER REFERENCES users(id),
    started_at      TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at    TEXT,
    error           TEXT
);

CREATE TABLE IF NOT EXISTS ha_vm_overrides (
    vm_id           TEXT PRIMARY KEY REFERENCES vms(id) ON DELETE CASCADE,
    restart_priority TEXT,
    isolation_response TEXT
);

CREATE TABLE IF NOT EXISTS alarms (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    target_type     TEXT NOT NULL,
    target_id       TEXT NOT NULL,
    condition_type  TEXT NOT NULL,
    threshold       REAL,
    severity        TEXT NOT NULL DEFAULT 'warning',
    triggered       INTEGER NOT NULL DEFAULT 0,
    acknowledged    INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    triggered_at    TEXT
);

-- DRS rules — configurable thresholds and behavior for the DRS engine
CREATE TABLE IF NOT EXISTS drs_rules (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id      TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    enabled         INTEGER NOT NULL DEFAULT 1,
    -- Trigger condition
    metric          TEXT NOT NULL DEFAULT 'cpu_usage',
                    -- cpu_usage, ram_usage, vm_count_imbalance
    threshold       REAL NOT NULL DEFAULT 80.0,
                    -- Percentage threshold to trigger (e.g. 80 = 80%)
    -- Action
    action          TEXT NOT NULL DEFAULT 'recommend',
                    -- recommend (manual), auto_migrate
    -- Cooldown: minimum seconds between recommendations for the same VM
    cooldown_secs   INTEGER NOT NULL DEFAULT 3600,
    -- Priority of generated recommendations
    priority        TEXT NOT NULL DEFAULT 'medium',
                    -- low, medium, high, critical
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- NOTIFICATION SYSTEM
-- ═══════════════════════════════════════════════════════════════

-- Notification channels — delivery targets (email, webhook, log)
CREATE TABLE IF NOT EXISTS notification_channels (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    channel_type    TEXT NOT NULL,           -- email, webhook, log
    enabled         INTEGER NOT NULL DEFAULT 1,
    -- Config (JSON) — type-specific settings:
    --   email:   { "smtp_host", "smtp_port", "smtp_user", "smtp_pass", "from", "to" }
    --   webhook: { "url", "method", "headers", "secret" }
    --   log:     { "level" }  (writes to cluster event log)
    config_json     TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Notification rules — which events trigger which channels
CREATE TABLE IF NOT EXISTS notification_rules (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    enabled         INTEGER NOT NULL DEFAULT 1,
    -- Trigger conditions
    event_category  TEXT NOT NULL DEFAULT '*',   -- ha, drs, host, vm, datastore, alarm, task, * (all)
    min_severity    TEXT NOT NULL DEFAULT 'warning', -- info, warning, error, critical
    -- Target channel
    channel_id      INTEGER NOT NULL REFERENCES notification_channels(id) ON DELETE CASCADE,
    -- Throttle: minimum seconds between notifications for the same event source
    cooldown_secs   INTEGER NOT NULL DEFAULT 300,
    -- Optional: filter to specific cluster
    cluster_id      TEXT REFERENCES clusters(id) ON DELETE CASCADE,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Notification log — history of sent notifications
CREATE TABLE IF NOT EXISTS notification_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id         INTEGER REFERENCES notification_rules(id),
    channel_id      INTEGER REFERENCES notification_channels(id),
    event_id        INTEGER,
    status          TEXT NOT NULL DEFAULT 'sent',  -- sent, failed, throttled
    error           TEXT,
    sent_at         TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;

/// Initialize the database: create tables and seed default data.
pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(SCHEMA)
        .map_err(|e| format!("Failed to create schema: {}", e))?;

    seed_defaults(db)?;
    Ok(())
}

/// Seed default admin user and default resource group.
fn seed_defaults(db: &Connection) -> Result<(), String> {
    // Default admin user (admin/admin)
    let user_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM users", [], |row| row.get(0),
    ).unwrap_or(0);

    if user_count == 0 {
        use argon2::PasswordHasher;
        let salt = argon2::password_hash::SaltString::generate(&mut rand::rngs::OsRng);
        let hash = argon2::Argon2::default()
            .hash_password(b"admin", &salt)
            .map_err(|e| format!("Failed to hash password: {}", e))?
            .to_string();

        db.execute(
            "INSERT INTO users (username, password_hash, role) VALUES ('admin', ?1, 'admin')",
            rusqlite::params![&hash],
        ).map_err(|e| format!("Failed to seed admin user: {}", e))?;
        tracing::info!("Seeded default admin user (admin/admin)");
    }

    // Default resource group
    let rg_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM resource_groups", [], |row| row.get(0),
    ).unwrap_or(0);

    if rg_count == 0 {
        db.execute(
            "INSERT INTO resource_groups (name, description, is_default) VALUES ('All Machines', 'Default resource group for all VMs', 1)",
            [],
        ).map_err(|e| format!("Failed to seed resource group: {}", e))?;
    }

    Ok(())
}
