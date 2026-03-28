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
    default_viswitch_id INTEGER,                    -- auto-created viSwitch for new hosts
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS hosts (
    id              TEXT PRIMARY KEY,
    hostname        TEXT NOT NULL,
    display_name    TEXT NOT NULL DEFAULT '',  -- custom name (editable), falls back to hostname if empty
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
    registered_at   TEXT NOT NULL DEFAULT (datetime('now')),
    -- CoreSAN status (updated via heartbeat, auto-discovered)
    san_enabled     INTEGER NOT NULL DEFAULT 0,      -- 1 if vmm-san is running on this host
    san_node_id     TEXT NOT NULL DEFAULT '',         -- CoreSAN node UUID
    san_address     TEXT NOT NULL DEFAULT '',         -- CoreSAN API address (e.g. http://host:7443)
    san_volumes     INTEGER NOT NULL DEFAULT 0,       -- number of CoreSAN volumes
    san_peers       INTEGER NOT NULL DEFAULT 0        -- number of CoreSAN peers
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
    drs_excluded    INTEGER NOT NULL DEFAULT 0,
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
    cluster_id      TEXT REFERENCES clusters(id) ON DELETE SET NULL,  -- NULL = global
    drs_excluded    INTEGER NOT NULL DEFAULT 0,  -- 1 = VMs in this group excluded from DRS
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

-- ═══════════════════════════════════════════════════════════════
-- CLUSTER SETTINGS (key-value store for global configuration)
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS cluster_settings (
    key             TEXT PRIMARY KEY,
    value           TEXT NOT NULL DEFAULT '',
    category        TEXT NOT NULL DEFAULT 'general'  -- smtp, ldap, dhcp, dns, general
);

-- ═══════════════════════════════════════════════════════════════
-- DRS EXCLUSIONS (VMs or resource groups excluded from DRS)
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS drs_exclusions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id      TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    exclusion_type  TEXT NOT NULL,           -- vm, resource_group
    target_id       TEXT NOT NULL,           -- VM ID or resource group ID
    reason          TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(cluster_id, exclusion_type, target_id)
);

-- ═══════════════════════════════════════════════════════════════
-- NETWORK SERVICES (optional DHCP + DNS per cluster)
-- ═══════════════════════════════════════════════════════════════

-- Virtual networks (cluster-managed networks with optional services)
CREATE TABLE IF NOT EXISTS virtual_networks (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id      TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    vlan_id         INTEGER,                    -- NULL = untagged
    subnet          TEXT NOT NULL DEFAULT '',    -- e.g. "10.0.0.0/24"
    gateway         TEXT NOT NULL DEFAULT '',    -- e.g. "10.0.0.1"
    -- Services
    dhcp_enabled    INTEGER NOT NULL DEFAULT 0,
    dhcp_range_start TEXT NOT NULL DEFAULT '',   -- e.g. "10.0.0.100"
    dhcp_range_end  TEXT NOT NULL DEFAULT '',    -- e.g. "10.0.0.200"
    dhcp_lease_secs INTEGER NOT NULL DEFAULT 3600,
    dns_enabled     INTEGER NOT NULL DEFAULT 0,
    dns_domain      TEXT NOT NULL DEFAULT '',    -- e.g. "vm.local"
    dns_upstream    TEXT NOT NULL DEFAULT '',    -- e.g. "8.8.8.8,8.8.4.4"
    pxe_enabled     INTEGER NOT NULL DEFAULT 0,
    pxe_boot_file   TEXT NOT NULL DEFAULT '',    -- e.g. "pxelinux.0" or "ipxe.efi"
    pxe_tftp_root   TEXT NOT NULL DEFAULT '',    -- e.g. "/vmm/tftp"
    pxe_next_server TEXT NOT NULL DEFAULT '',    -- TFTP server IP (usually cluster IP)
    auto_register_dns INTEGER NOT NULL DEFAULT 1, -- auto-create DNS records for VM names
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(cluster_id, name)
);

-- Kept for backward compat — but virtual_networks is the primary model
CREATE TABLE IF NOT EXISTS network_services (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id      TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    service_type    TEXT NOT NULL,           -- dhcp, dns, pxe
    enabled         INTEGER NOT NULL DEFAULT 0,
    config_json     TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(cluster_id, service_type)
);

-- DHCP leases (managed by the built-in DHCP server)
CREATE TABLE IF NOT EXISTS dhcp_leases (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    network_service_id INTEGER NOT NULL REFERENCES network_services(id) ON DELETE CASCADE,
    mac_address     TEXT NOT NULL,
    ip_address      TEXT NOT NULL,
    hostname        TEXT,
    vm_id           TEXT REFERENCES vms(id) ON DELETE SET NULL,
    lease_start     TEXT NOT NULL DEFAULT (datetime('now')),
    lease_end       TEXT,
    UNIQUE(network_service_id, mac_address)
);

-- DNS records (managed by the built-in DNS server)
CREATE TABLE IF NOT EXISTS dns_records (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    network_service_id INTEGER NOT NULL REFERENCES network_services(id) ON DELETE CASCADE,
    record_type     TEXT NOT NULL DEFAULT 'A',  -- A, AAAA, CNAME, PTR
    name            TEXT NOT NULL,
    value           TEXT NOT NULL,
    ttl             INTEGER NOT NULL DEFAULT 3600,
    auto_registered INTEGER NOT NULL DEFAULT 0,  -- 1 = auto-created from VM name
    UNIQUE(network_service_id, record_type, name)
);

-- PXE boot entries (ISOs linked to PXE boot menu per network)
CREATE TABLE IF NOT EXISTS pxe_boot_entries (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    network_id      INTEGER NOT NULL REFERENCES virtual_networks(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,           -- Menu label (e.g. "Ubuntu 24.04 Server")
    iso_id          TEXT REFERENCES isos(id) ON DELETE SET NULL,
    iso_path        TEXT NOT NULL DEFAULT '', -- Direct path if no ISO record
    boot_args       TEXT NOT NULL DEFAULT '', -- Kernel args (e.g. "auto=true")
    sort_order      INTEGER NOT NULL DEFAULT 0,
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ═══════════════════════════════════════════════════════════════
-- viSWITCH (VIRTUAL SWITCH)
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS viswitches (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id      TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    max_ports       INTEGER NOT NULL DEFAULT 1024,   -- max VM ports (1–1024)
    max_uplinks     INTEGER NOT NULL DEFAULT 128,    -- max uplink ports (1–128)
    mtu             INTEGER NOT NULL DEFAULT 1500,
    uplink_policy   TEXT NOT NULL DEFAULT 'failover', -- roundrobin, failover, rulebased
    uplink_rules    TEXT NOT NULL DEFAULT '[]',       -- JSON rules for rulebased
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(cluster_id, name)
);

CREATE TABLE IF NOT EXISTS viswitch_uplinks (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    viswitch_id     INTEGER NOT NULL REFERENCES viswitches(id) ON DELETE CASCADE,
    uplink_index    INTEGER NOT NULL,                 -- 0–127
    uplink_type     TEXT NOT NULL,                     -- 'physical' or 'virtual'
    physical_nic    TEXT NOT NULL DEFAULT '',           -- NIC name for physical uplinks
    network_id      INTEGER REFERENCES virtual_networks(id) ON DELETE SET NULL,
    active          INTEGER NOT NULL DEFAULT 1,        -- 1=active, 0=standby (failover)
    traffic_types   TEXT NOT NULL DEFAULT 'vm',        -- comma-separated: vm, san, vm,san
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(viswitch_id, uplink_index)
);

CREATE TABLE IF NOT EXISTS viswitch_ports (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    viswitch_id     INTEGER NOT NULL REFERENCES viswitches(id) ON DELETE CASCADE,
    port_index      INTEGER NOT NULL,                 -- 0–1023
    vm_id           TEXT REFERENCES vms(id) ON DELETE SET NULL,
    vlan_id         INTEGER,                           -- port VLAN tag (NULL = untagged)
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(viswitch_id, port_index)
);

-- ═══════════════════════════════════════════════════════════════
-- LDAP / ACTIVE DIRECTORY INTEGRATION
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS ldap_configs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    enabled         INTEGER NOT NULL DEFAULT 0,
    server_url      TEXT NOT NULL DEFAULT '',        -- ldap://dc.example.com:389
    bind_dn         TEXT NOT NULL DEFAULT '',        -- CN=svc_vmm,OU=ServiceAccounts,DC=example,DC=com
    bind_password   TEXT NOT NULL DEFAULT '',
    base_dn         TEXT NOT NULL DEFAULT '',        -- DC=example,DC=com
    user_search_dn  TEXT NOT NULL DEFAULT '',        -- OU=Users,DC=example,DC=com
    user_filter     TEXT NOT NULL DEFAULT '(&(objectClass=user)(sAMAccountName={username}))',
    group_search_dn TEXT NOT NULL DEFAULT '',        -- OU=Groups,DC=example,DC=com
    group_filter    TEXT NOT NULL DEFAULT '(&(objectClass=group)(member={user_dn}))',
    -- Attribute mappings
    attr_username   TEXT NOT NULL DEFAULT 'sAMAccountName',
    attr_email      TEXT NOT NULL DEFAULT 'mail',
    attr_display    TEXT NOT NULL DEFAULT 'displayName',
    -- Role mapping (JSON): { "VMM-Admins": "admin", "VMM-Operators": "operator", "VMM-Viewers": "viewer" }
    role_mapping    TEXT NOT NULL DEFAULT '{}',
    -- TLS
    use_tls         INTEGER NOT NULL DEFAULT 0,
    skip_tls_verify INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;

/// Initialize the database: create tables and seed default data.
pub fn init(db: &Connection) -> Result<(), String> {
    // Migrate existing databases before creating schema
    migrate(db);

    db.execute_batch(SCHEMA)
        .map_err(|e| format!("Failed to create schema: {}", e))?;

    seed_defaults(db)?;
    Ok(())
}

/// Apply schema migrations for existing databases.
fn migrate(db: &Connection) {
    // CoreSAN host fields (added in CoreSAN integration)
    db.execute_batch("ALTER TABLE hosts ADD COLUMN san_enabled INTEGER NOT NULL DEFAULT 0;").ok();
    db.execute_batch("ALTER TABLE hosts ADD COLUMN san_node_id TEXT NOT NULL DEFAULT '';").ok();
    db.execute_batch("ALTER TABLE hosts ADD COLUMN san_address TEXT NOT NULL DEFAULT '';").ok();
    db.execute_batch("ALTER TABLE hosts ADD COLUMN san_volumes INTEGER NOT NULL DEFAULT 0;").ok();
    db.execute_batch("ALTER TABLE hosts ADD COLUMN san_peers INTEGER NOT NULL DEFAULT 0;").ok();
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
