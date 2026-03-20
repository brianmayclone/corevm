//! SQLite database initialization and migrations.

use rusqlite::Connection;

/// Initialize the database: create tables if they don't exist, seed admin user.
pub fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(SCHEMA).map_err(|e| format!("DB init failed: {}", e))?;
    seed_admin(conn)?;
    Ok(())
}

/// Seed the default admin user if no users exist.
fn seed_admin(conn: &Connection) -> Result<(), String> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
        .map_err(|e| format!("DB query failed: {}", e))?;
    if count == 0 {
        // Hash "admin" password with argon2
        let salt = argon2::password_hash::SaltString::generate(&mut rand::rngs::OsRng);
        let hash = argon2::Argon2::default()
            .hash_password(b"admin", &salt)
            .map_err(|e| format!("Password hash failed: {}", e))?
            .to_string();
        conn.execute(
            "INSERT INTO users (username, password_hash, role) VALUES (?1, ?2, 'admin')",
            rusqlite::params![&"admin", &hash],
        ).map_err(|e| format!("Seed admin failed: {}", e))?;
        tracing::info!("Seeded default admin user (username: admin, password: admin)");
    }
    Ok(())
}

use argon2::PasswordHasher;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    username    TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'operator',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS vms (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT DEFAULT '',
    config_json TEXT NOT NULL,
    owner_id    INTEGER NOT NULL REFERENCES users(id),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS storage_pools (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    path        TEXT NOT NULL UNIQUE,
    pool_type   TEXT NOT NULL DEFAULT 'local',  -- local, nfs, cephfs, glusterfs
    shared      INTEGER NOT NULL DEFAULT 0,     -- 0=local, 1=shared (accessible from multiple hosts)
    mount_source TEXT,                          -- NFS: "server:/export", CephFS: "mon1,mon2:/path"
    mount_opts  TEXT,                           -- mount options (e.g. "vers=4,noatime")
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS disk_images (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    path        TEXT NOT NULL UNIQUE,
    size_bytes  INTEGER NOT NULL,
    format      TEXT NOT NULL DEFAULT 'raw',
    pool_id     INTEGER REFERENCES storage_pools(id),
    vm_id       TEXT REFERENCES vms(id) ON DELETE SET NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS isos (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    path        TEXT NOT NULL UNIQUE,
    size_bytes  INTEGER NOT NULL,
    pool_id     INTEGER REFERENCES storage_pools(id),
    uploaded_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS snapshots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    vm_id       TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    disk_snapshot_path TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS port_forwards (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    vm_id       TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    protocol    TEXT NOT NULL DEFAULT 'tcp',
    host_port   INTEGER NOT NULL,
    guest_port  INTEGER NOT NULL,
    host_ip     TEXT NOT NULL DEFAULT '0.0.0.0'
);

CREATE TABLE IF NOT EXISTS audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER REFERENCES users(id),
    action      TEXT NOT NULL,
    target_type TEXT,
    target_id   TEXT,
    details     TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;
