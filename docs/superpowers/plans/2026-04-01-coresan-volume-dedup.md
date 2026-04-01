# CoreSAN Volume-Level Deduplication — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add transparent, opt-in, post-process deduplication to CoreSAN volumes with UI metrics and chunk-map visualization.

**Architecture:** A new background engine (`dedup.rs`) periodically scans dedup-enabled volumes for duplicate chunks (by SHA256), consolidates them into a content-addressed store (`.dedup/<sha256>`), and updates the DB. The read path is extended to resolve deduplicated chunks. The API exposes dedup stats per volume. The UI shows savings in volume cards, a dedup toggle in volume creation, and purple-colored dedup chunks in the allocation map.

**Tech Stack:** Rust (backend: axum, rusqlite, tokio, sha2), TypeScript/React 19 (frontend: Tailwind CSS, Lucide icons)

**Spec:** `docs/superpowers/specs/2026-04-01-coresan-volume-dedup-design.md`

---

## File Map

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `apps/vmm-san/src/db/mod.rs` | Add `dedup_store` table, `dedup` column on volumes, `dedup_sha256` column on file_chunks |
| Modify | `apps/vmm-san/src/config.rs` | Add `DedupSection` to config |
| Create | `apps/vmm-san/src/engine/dedup.rs` | Background dedup engine |
| Modify | `apps/vmm-san/src/engine/mod.rs` | Register dedup module |
| Modify | `apps/vmm-san/src/main.rs` | Spawn dedup engine |
| Modify | `apps/vmm-san/src/storage/chunk.rs` | Extend `read_chunk_data` to resolve `.dedup/` path |
| Modify | `apps/vmm-san/src/services/volume.rs` | Add `dedup` field to `VolumeInfo` |
| Modify | `apps/vmm-san/src/api/volumes.rs` | Add `dedup` to create/update/response, add `dedup_stats` |
| Modify | `apps/vmm-san/src/api/files.rs` | Add `deduplicated`/`dedup_sha256` to `ChunkMapEntry` |
| Modify | `apps/vmm-san/src/engine/integrity.rs` | Resolve `.dedup/` path for integrity checks |
| Modify | `apps/vmm-san/src/engine/rebalancer.rs` | Handle `.dedup/` chunks during rebalance |
| Modify | `apps/vmm-ui/src/api/types.ts` | Add dedup fields to TypeScript types |
| Modify | `apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx` | Add dedup toggle |
| Modify | `apps/vmm-ui/src/pages/StorageCoresan.tsx` | Show dedup stats in volume detail |
| Modify | `apps/vmm-ui/src/components/coresan/VolumeChunkMap.tsx` | Purple dedup chunk colors, hover highlight |
| Modify | `apps/vmm-ui/src/pages/StorageOverview.tsx` | Dedup savings in aggregate capacity bar |

---

## Task 1: Database Schema — dedup_store table, migrations

**Files:**
- Modify: `apps/vmm-san/src/db/mod.rs`

- [ ] **Step 1: Add `dedup_store` table to SCHEMA**

In `apps/vmm-san/src/db/mod.rs`, add the new table and index after the `smart_data` table (before the closing `"#;`):

```sql
-- ═══════════════════════════════════════════════════════════════
-- DEDUP_STORE: content-addressed chunks for volume deduplication
-- Each row is a unique physical chunk in .dedup/<sha256> on a backend
-- ═══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS dedup_store (
    sha256       TEXT NOT NULL,
    volume_id    TEXT NOT NULL,
    backend_id   TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    ref_count    INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (sha256, volume_id, backend_id)
);

CREATE INDEX IF NOT EXISTS idx_dedup_store_volume ON dedup_store(volume_id);
CREATE INDEX IF NOT EXISTS idx_dedup_store_ref ON dedup_store(volume_id, ref_count);
```

- [ ] **Step 2: Add migration for `dedup` column on volumes**

In the `migrate()` function, add after the `max_size_bytes` migration (around line 345):

```rust
    // Dedup opt-in per volume
    db.execute_batch("ALTER TABLE volumes ADD COLUMN dedup INTEGER NOT NULL DEFAULT 0;").ok();
```

- [ ] **Step 3: Add migration for `dedup_sha256` column on file_chunks**

In the same `migrate()` function, add immediately after:

```rust
    // Dedup: tracks whether a chunk has been moved to the content-addressed store
    db.execute_batch("ALTER TABLE file_chunks ADD COLUMN dedup_sha256 TEXT DEFAULT NULL;").ok();
```

- [ ] **Step 4: Build and verify**

Run: `cargo build -p vmm-san 2>&1 | head -20`
Expected: Clean compile (no errors)

- [ ] **Step 5: Commit**

```bash
git add apps/vmm-san/src/db/mod.rs
git commit -m "feat(coresan): add dedup_store table and dedup columns to schema"
```

---

## Task 2: Config — DedupSection

**Files:**
- Modify: `apps/vmm-san/src/config.rs`

- [ ] **Step 1: Add DedupSection struct**

In `apps/vmm-san/src/config.rs`, add after `IntegritySection` (after line 99):

```rust
#[derive(Debug, Deserialize)]
pub struct DedupSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_dedup_interval")]
    pub interval_secs: u64,
}
```

- [ ] **Step 2: Add default function**

After `default_repair_interval` (line 134):

```rust
fn default_dedup_interval() -> u64 { 300 }
```

- [ ] **Step 3: Add Default impl**

After the `IntegritySection` Default impl (after line 185):

```rust
impl Default for DedupSection {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_dedup_interval(),
        }
    }
}
```

- [ ] **Step 4: Add dedup field to CoreSanConfig**

Add `pub dedup: DedupSection` to the `CoreSanConfig` struct, with `#[serde(default)]` attribute. Also add `dedup: Default::default()` in the `CoreSanConfig` Default impl.

- [ ] **Step 5: Build and verify**

Run: `cargo build -p vmm-san 2>&1 | head -20`
Expected: Clean compile

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-san/src/config.rs
git commit -m "feat(coresan): add [dedup] config section"
```

---

## Task 3: VolumeService — add dedup field

**Files:**
- Modify: `apps/vmm-san/src/services/volume.rs`

- [ ] **Step 1: Add `dedup` to VolumeInfo struct**

In `apps/vmm-san/src/services/volume.rs`, add to `VolumeInfo` (after `chunk_size_bytes`):

```rust
    pub dedup: bool,
```

- [ ] **Step 2: Update all SQL SELECT queries to include dedup**

Update `create()`, `get()`, `list()`, `list_online()` to include the `dedup` column. For the row mappings, read it as `row.get::<_, i32>(N)? != 0` and map to `dedup: row.get::<_, i32>(N)? != 0`.

For `get()` the query becomes:
```rust
"SELECT id, name, ftt, local_raid, chunk_size_bytes, status, created_at, dedup FROM volumes WHERE id = ?1"
```
And the mapping adds: `dedup: row.get::<_, i32>(7)? != 0,`

Apply the same pattern to `list()` and `list_online()` (add `dedup` to SELECT, add field to struct init).

- [ ] **Step 3: Update `create()` to accept dedup parameter**

Change the `create` signature to include `dedup: bool`:

```rust
pub fn create(db: &Connection, id: &str, name: &str, ftt: u32, chunk_size: u64, local_raid: &str, dedup: bool) -> Result<(), String> {
    db.execute(
        "INSERT INTO volumes (id, name, ftt, chunk_size_bytes, local_raid, dedup, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'online')",
        rusqlite::params![id, name, ftt, chunk_size, local_raid, dedup as i32],
    ).map_err(|e| format!("Failed to create volume: {}", e))?;
    Ok(())
}
```

- [ ] **Step 4: Add `update_dedup()` method**

```rust
pub fn update_dedup(db: &Connection, id: &str, dedup: bool) {
    log_err!(db.execute("UPDATE volumes SET dedup = ?1 WHERE id = ?2", rusqlite::params![dedup as i32, id]),
        "VolumeService::update_dedup");
}
```

- [ ] **Step 5: Build and fix any callers of `create()` that break**

The `create()` signature change will break callers in `api/volumes.rs` and possibly `api/peers.rs` (sync_volume). Fix them by adding `false` as the dedup argument for now. The proper integration comes in Task 6.

Run: `cargo build -p vmm-san 2>&1 | head -30`
Expected: Clean compile

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-san/src/services/volume.rs
git add apps/vmm-san/src/api/volumes.rs apps/vmm-san/src/api/peers.rs
git commit -m "feat(coresan): add dedup field to VolumeService"
```

---

## Task 4: Read path — resolve deduplicated chunks

**Files:**
- Modify: `apps/vmm-san/src/storage/chunk.rs`

- [ ] **Step 1: Add `dedup_chunk_path` helper function**

After the existing `chunk_path` function (line 68):

```rust
/// Build the filesystem path for a content-addressed dedup chunk.
pub fn dedup_chunk_path(backend_path: &str, volume_id: &str, sha256: &str) -> PathBuf {
    Path::new(backend_path)
        .join(".coresan")
        .join(volume_id)
        .join(".dedup")
        .join(sha256)
}
```

- [ ] **Step 2: Extend `read_chunk_data` to check `dedup_sha256`**

In the `read_chunk_data` function, modify the replica query (line 185-195) to also fetch `dedup_sha256`:

```rust
let replicas: Vec<(String, String, String, Option<String>)> = {
    let mut stmt = db.prepare(
        "SELECT cr.backend_id, b.path, COALESCE(fc.sha256, ''), fc.dedup_sha256 FROM chunk_replicas cr
         JOIN backends b ON b.id = cr.backend_id
         JOIN file_chunks fc ON fc.id = cr.chunk_id
         WHERE fc.file_id = ?1 AND fc.chunk_index = ?2
           AND cr.node_id = ?3 AND cr.state = 'synced'"
    ).unwrap();
    stmt.query_map(
        rusqlite::params![file_id, range.chunk_index, node_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap().filter_map(|r| r.ok()).collect()
};
```

Then in the loop over replicas (line 201), change the path resolution:

```rust
for (backend_id, bp, expected_sha, dedup_sha) in &replicas {
    let path = if let Some(ref dsha) = dedup_sha {
        dedup_chunk_path(bp, volume_id, dsha)
    } else {
        chunk_path(bp, volume_id, file_id, range.chunk_index)
    };
```

- [ ] **Step 3: Build and verify**

Run: `cargo build -p vmm-san 2>&1 | head -20`
Expected: Clean compile

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/storage/chunk.rs
git commit -m "feat(coresan): read path resolves deduplicated chunks from .dedup/ store"
```

---

## Task 5: Dedup Engine

**Files:**
- Create: `apps/vmm-san/src/engine/dedup.rs`
- Modify: `apps/vmm-san/src/engine/mod.rs`
- Modify: `apps/vmm-san/src/main.rs`

- [ ] **Step 1: Create `dedup.rs`**

Create `apps/vmm-san/src/engine/dedup.rs`:

```rust
//! Dedup engine — periodic post-process deduplication of chunk data.
//!
//! Scans dedup-enabled volumes for chunks with duplicate SHA256 hashes,
//! consolidates them into a content-addressed store (.dedup/<sha256>),
//! and removes the original positional chunk files.

use std::sync::Arc;
use sha2::{Sha256, Digest};
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::storage::chunk;

/// Spawn the dedup engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    if !state.config.dedup.enabled {
        tracing::info!("Dedup engine disabled");
        return;
    }

    let check_interval = state.config.dedup.interval_secs;
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(check_interval));
        loop {
            tick.tick().await;
            run_dedup_cycle(&state).await;
        }
    });
}

/// Run one dedup cycle across all dedup-enabled volumes.
async fn run_dedup_cycle(state: &CoreSanState) {
    // Get all volumes with dedup enabled
    let volumes: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name FROM volumes WHERE dedup = 1 AND status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    if volumes.is_empty() {
        return;
    }

    for (volume_id, volume_name) in &volumes {
        dedup_volume(state, volume_id, volume_name).await;
    }
}

/// Deduplicate a single volume: find duplicate SHA256 hashes and consolidate.
async fn dedup_volume(state: &CoreSanState, volume_id: &str, volume_name: &str) {
    // Find duplicate SHA256 hashes among non-deduplicated chunks.
    // Skip files with active write leases to avoid conflicts.
    let duplicates: Vec<(String, i64)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT fc.sha256, COUNT(*) as cnt
             FROM file_chunks fc
             JOIN file_map fm ON fm.id = fc.file_id
             WHERE fm.volume_id = ?1
               AND fc.dedup_sha256 IS NULL
               AND fc.sha256 != ''
               AND (fm.write_owner = '' OR fm.write_lease_until < datetime('now'))
             GROUP BY fc.sha256
             HAVING cnt > 1
             LIMIT 100"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![volume_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if duplicates.is_empty() {
        return;
    }

    tracing::info!("Dedup: volume '{}' has {} duplicate SHA256 groups to process", volume_name, duplicates.len());

    let mut consolidated = 0u64;
    let mut saved_bytes = 0u64;

    for (sha256, dup_count) in &duplicates {
        match consolidate_sha256(state, volume_id, sha256).await {
            Ok(saved) => {
                consolidated += 1;
                saved_bytes += saved;
            }
            Err(e) => {
                tracing::warn!("Dedup: failed to consolidate sha256={} in volume {}: {}",
                    &sha256[..16.min(sha256.len())], volume_name, e);
            }
        }
    }

    // Cleanup: remove dedup_store entries with ref_count = 0
    cleanup_orphaned(state, volume_id);

    if consolidated > 0 {
        tracing::info!("Dedup: volume '{}' consolidated {} groups, saved {} bytes",
            volume_name, consolidated, saved_bytes);
    }
}

/// Consolidate all chunks with a given SHA256 into the content-addressed store.
/// Returns bytes saved.
async fn consolidate_sha256(state: &CoreSanState, volume_id: &str, sha256: &str) -> Result<u64, String> {
    // Find all chunks and their backends for this SHA256
    let chunks: Vec<(i64, i64, u32, u64)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT fc.id, fc.file_id, fc.chunk_index, fc.size_bytes
             FROM file_chunks fc
             JOIN file_map fm ON fm.id = fc.file_id
             WHERE fm.volume_id = ?1 AND fc.sha256 = ?2 AND fc.dedup_sha256 IS NULL"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![volume_id, sha256],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    if chunks.len() < 2 {
        return Ok(0); // Not a duplicate anymore
    }

    let chunk_size = chunks[0].3;

    // Get all backends that hold replicas of these chunks
    let backend_ids: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let chunk_ids: Vec<i64> = chunks.iter().map(|(id, _, _, _)| *id).collect();
        // Get unique backends for these chunks on this node
        let placeholders: String = chunk_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let query = format!(
            "SELECT DISTINCT cr.backend_id, b.path
             FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             WHERE cr.chunk_id IN ({}) AND cr.node_id = ?1 AND cr.backend_id != ''",
            placeholders
        );
        let mut stmt = db.prepare(&query).unwrap();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = chunk_ids.iter()
            .map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        params.push(Box::new(state.node_id.clone()));
        stmt.query_map(rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    let mut saved = 0u64;

    // For each backend, ensure the .dedup/<sha256> file exists
    for (backend_id, backend_path) in &backend_ids {
        let dedup_path = chunk::dedup_chunk_path(backend_path, volume_id, sha256);

        if !dedup_path.exists() {
            // Find a source chunk on this backend
            let source_path = find_source_chunk(state, volume_id, &chunks, backend_id);
            if let Some(src) = source_path {
                // Atomic copy: read → tmp → fsync → rename
                let data = tokio::fs::read(&src).await
                    .map_err(|e| format!("Read source chunk: {}", e))?;

                // Verify SHA256
                let actual_sha = format!("{:x}", Sha256::digest(&data));
                if actual_sha != sha256 {
                    return Err(format!("SHA256 mismatch: expected {}, got {}", sha256, actual_sha));
                }

                // Create .dedup directory
                if let Some(parent) = dedup_path.parent() {
                    tokio::fs::create_dir_all(parent).await
                        .map_err(|e| format!("Create .dedup dir: {}", e))?;
                }

                // Atomic write
                let tmp = dedup_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                tokio::fs::write(&tmp, &data).await
                    .map_err(|e| format!("Write dedup tmp: {}", e))?;
                if let Ok(f) = tokio::fs::File::open(&tmp).await {
                    f.sync_all().await.ok();
                }
                tokio::fs::rename(&tmp, &dedup_path).await
                    .map_err(|e| format!("Rename dedup: {}", e))?;
            }
        }

        // Insert/update dedup_store
        {
            let db = state.db.lock().unwrap();
            let ref_count = chunks.len() as i64;
            db.execute(
                "INSERT INTO dedup_store (sha256, volume_id, backend_id, size_bytes, ref_count)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(sha256, volume_id, backend_id) DO UPDATE SET ref_count = ?5",
                rusqlite::params![sha256, volume_id, backend_id, chunk_size, ref_count],
            ).ok();
        }
    }

    // Update all file_chunks to point to the dedup store
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "UPDATE file_chunks SET dedup_sha256 = ?1
             WHERE sha256 = ?1 AND dedup_sha256 IS NULL
               AND file_id IN (SELECT id FROM file_map WHERE volume_id = ?2)",
            rusqlite::params![sha256, volume_id],
        ).ok();
    }

    // Delete old positional chunk files
    for (chunk_id, file_id, chunk_index, size) in &chunks {
        for (backend_id, backend_path) in &backend_ids {
            let old_path = chunk::chunk_path(backend_path, volume_id, *file_id, *chunk_index);
            if old_path.exists() {
                tokio::fs::remove_file(&old_path).await.ok();
                saved += size;
            }
        }
    }

    // We keep one copy in .dedup, so subtract that from savings
    saved = saved.saturating_sub(chunk_size * backend_ids.len() as u64);

    Ok(saved)
}

/// Find a source chunk file on a specific backend.
fn find_source_chunk(
    state: &CoreSanState,
    volume_id: &str,
    chunks: &[(i64, i64, u32, u64)],
    backend_id: &str,
) -> Option<std::path::PathBuf> {
    let db = state.db.lock().unwrap();
    for (chunk_id, file_id, chunk_index, _) in chunks {
        let has_replica: bool = db.query_row(
            "SELECT COUNT(*) FROM chunk_replicas WHERE chunk_id = ?1 AND backend_id = ?2 AND state = 'synced'",
            rusqlite::params![chunk_id, backend_id],
            |row| row.get::<_, i64>(0),
        ).map(|c| c > 0).unwrap_or(false);

        if has_replica {
            let backend_path: String = db.query_row(
                "SELECT path FROM backends WHERE id = ?1",
                rusqlite::params![backend_id],
                |row| row.get(0),
            ).unwrap_or_default();

            let path = chunk::chunk_path(&backend_path, volume_id, *file_id, *chunk_index);
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

/// Remove dedup_store entries with ref_count = 0 and delete orphaned .dedup files.
fn cleanup_orphaned(state: &CoreSanState, volume_id: &str) {
    let orphans: Vec<(String, String, String)> = {
        let db = state.db.lock().unwrap();

        // Recount refs: count how many file_chunks reference each dedup SHA256
        db.execute(
            "UPDATE dedup_store SET ref_count = (
                SELECT COUNT(*) FROM file_chunks fc
                JOIN file_map fm ON fm.id = fc.file_id
                WHERE fc.dedup_sha256 = dedup_store.sha256
                  AND fm.volume_id = dedup_store.volume_id
            ) WHERE volume_id = ?1",
            rusqlite::params![volume_id],
        ).ok();

        let mut stmt = db.prepare(
            "SELECT sha256, backend_id, (SELECT path FROM backends WHERE id = dedup_store.backend_id)
             FROM dedup_store WHERE volume_id = ?1 AND ref_count = 0"
        ).unwrap();
        stmt.query_map(rusqlite::params![volume_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2).unwrap_or_default())),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    for (sha256, backend_id, backend_path) in &orphans {
        if !backend_path.is_empty() {
            let path = chunk::dedup_chunk_path(backend_path, volume_id, sha256);
            std::fs::remove_file(&path).ok();
        }
    }

    if !orphans.is_empty() {
        let db = state.db.lock().unwrap();
        db.execute(
            "DELETE FROM dedup_store WHERE volume_id = ?1 AND ref_count = 0",
            rusqlite::params![volume_id],
        ).ok();
        tracing::info!("Dedup cleanup: removed {} orphaned dedup entries for volume {}", orphans.len(), volume_id);
    }
}
```

- [ ] **Step 2: Register module in `engine/mod.rs`**

Add after `pub mod mgmt_server;` (line 27):

```rust
pub mod dedup;
```

- [ ] **Step 3: Spawn dedup engine in `main.rs`**

Add after the SMART monitor spawn (around line 289, after `engine::smart_monitor::spawn`):

```rust
    engine::dedup::spawn(Arc::clone(&state));
    tracing::info!("Dedup engine started ({}s interval)", state.config.dedup.interval_secs);
```

- [ ] **Step 4: Build and verify**

Run: `cargo build -p vmm-san 2>&1 | head -30`
Expected: Clean compile

- [ ] **Step 5: Commit**

```bash
git add apps/vmm-san/src/engine/dedup.rs apps/vmm-san/src/engine/mod.rs apps/vmm-san/src/main.rs
git commit -m "feat(coresan): add post-process dedup background engine"
```

---

## Task 6: API — dedup in volume CRUD and dedup_stats

**Files:**
- Modify: `apps/vmm-san/src/api/volumes.rs`

- [ ] **Step 1: Add `dedup` to `CreateVolumeRequest`**

Add to the `CreateVolumeRequest` struct:

```rust
    #[serde(default)]
    pub dedup: bool,
```

- [ ] **Step 2: Add `dedup` to `UpdateVolumeRequest`**

Add to the `UpdateVolumeRequest` struct:

```rust
    pub dedup: Option<bool>,
```

- [ ] **Step 3: Add `dedup` and `dedup_stats` to `VolumeResponse`**

```rust
#[derive(Serialize)]
pub struct VolumeResponse {
    pub id: String,
    pub name: String,
    pub ftt: u32,
    pub local_raid: String,
    pub chunk_size_bytes: u64,
    pub max_size_bytes: u64,
    pub status: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub backend_count: u32,
    pub created_at: String,
    pub access_protocols: Vec<String>,
    pub dedup: bool,
    pub dedup_stats: Option<DedupStats>,
}

#[derive(Serialize)]
pub struct DedupStats {
    pub logical_bytes: u64,
    pub physical_bytes: u64,
    pub saved_bytes: u64,
    pub dedup_ratio: f64,
    pub dedup_chunk_count: u64,
    pub pending_chunk_count: u64,
}
```

- [ ] **Step 4: Add `query_dedup_stats` helper function**

Add a helper function that queries dedup statistics for a volume:

```rust
fn query_dedup_stats(db: &rusqlite::Connection, volume_id: &str, dedup: bool) -> Option<DedupStats> {
    if !dedup {
        return None;
    }

    let logical_bytes: u64 = db.query_row(
        "SELECT COALESCE(SUM(fc.size_bytes), 0)
         FROM file_chunks fc
         JOIN file_map fm ON fm.id = fc.file_id
         WHERE fm.volume_id = ?1 AND fc.dedup_sha256 IS NOT NULL",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    let physical_bytes: u64 = db.query_row(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM dedup_store WHERE volume_id = ?1",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    let dedup_chunk_count: u64 = db.query_row(
        "SELECT COUNT(*) FROM dedup_store WHERE volume_id = ?1",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    let pending_chunk_count: u64 = db.query_row(
        "SELECT COUNT(*) FROM (
            SELECT fc.sha256 FROM file_chunks fc
            JOIN file_map fm ON fm.id = fc.file_id
            WHERE fm.volume_id = ?1 AND fc.dedup_sha256 IS NULL AND fc.sha256 != ''
            GROUP BY fc.sha256 HAVING COUNT(*) > 1
        )",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    let saved_bytes = logical_bytes.saturating_sub(physical_bytes);
    let dedup_ratio = if physical_bytes > 0 {
        logical_bytes as f64 / physical_bytes as f64
    } else if logical_bytes > 0 {
        f64::INFINITY
    } else {
        1.0
    };

    Some(DedupStats {
        logical_bytes,
        physical_bytes,
        saved_bytes,
        dedup_ratio,
        dedup_chunk_count,
        pending_chunk_count,
    })
}
```

- [ ] **Step 5: Update `create()` handler to pass dedup**

In the `create` function, update the INSERT to include dedup:

```rust
    db.execute(
        "INSERT INTO volumes (id, name, ftt, chunk_size_bytes, local_raid, max_size_bytes, access_protocols, dedup, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'online')",
        rusqlite::params![&id, &body.name, body.ftt, body.chunk_size_bytes, &body.local_raid, body.max_size_bytes, &protocols_json, body.dedup as i32],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create volume: {}", e)))?;
```

And in the volume sync JSON:

```rust
    let vol_json = serde_json::json!({
        "id": &id, "name": &body.name, "ftt": body.ftt,
        "chunk_size_bytes": body.chunk_size_bytes, "local_raid": &body.local_raid,
        "max_size_bytes": body.max_size_bytes,
        "access_protocols": &body.access_protocols,
        "dedup": body.dedup,
    });
```

And in the response, add `dedup: body.dedup, dedup_stats: None,`.

- [ ] **Step 6: Update `list()` and `get()` to include dedup and dedup_stats**

In `list()`, update the SELECT to include `dedup` and compute stats:

The query adds `dedup` to the SELECT. In the row mapping, read `dedup` as `row.get::<_, i32>(N)? != 0` and call `query_dedup_stats(&db, &id, dedup)` for each volume.

In `get()`, do the same — read `dedup` from the row and compute stats.

- [ ] **Step 7: Update `update()` to handle dedup toggle**

In the `update` function, add after the `access_protocols` handling:

```rust
    if let Some(dedup) = body.dedup {
        db.execute("UPDATE volumes SET dedup = ?1 WHERE id = ?2",
            rusqlite::params![dedup as i32, &id]).ok();
    }
```

- [ ] **Step 8: Build and verify**

Run: `cargo build -p vmm-san 2>&1 | head -30`
Expected: Clean compile

- [ ] **Step 9: Commit**

```bash
git add apps/vmm-san/src/api/volumes.rs
git commit -m "feat(coresan): add dedup flag and dedup_stats to volume API"
```

---

## Task 7: Chunk Map API — dedup fields

**Files:**
- Modify: `apps/vmm-san/src/api/files.rs`

- [ ] **Step 1: Add dedup fields to `ChunkMapEntry`**

In `apps/vmm-san/src/api/files.rs`, add to the `ChunkMapEntry` struct (after `node_hostname`):

```rust
    pub deduplicated: bool,
    pub dedup_sha256: Option<String>,
```

- [ ] **Step 2: Update the chunk-map SQL query**

In the `chunk_map()` function, add `fc.dedup_sha256` to the SELECT and map it in the row:

Add to the SELECT: `fc.dedup_sha256`

In the row mapping, add:
```rust
let dedup_sha256: Option<String> = row.get(10).ok().flatten();
```

And in the `ChunkMapEntry` construction:
```rust
deduplicated: dedup_sha256.is_some(),
dedup_sha256,
```

- [ ] **Step 3: Build and verify**

Run: `cargo build -p vmm-san 2>&1 | head -20`
Expected: Clean compile

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/api/files.rs
git commit -m "feat(coresan): add dedup fields to chunk-map API response"
```

---

## Task 8: Integrity Engine — resolve dedup paths

**Files:**
- Modify: `apps/vmm-san/src/engine/integrity.rs`

- [ ] **Step 1: Extend integrity query to include dedup_sha256**

In `run_integrity_check`, modify the SQL query (line 35-40) to also select `fc.dedup_sha256`:

```rust
let chunks: Vec<(i64, i64, String, u32, String, String, Option<String>)> = {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT fc.id, fc.file_id, fc.sha256, fc.chunk_index, cr.backend_id, b.path, fc.dedup_sha256
         FROM chunk_replicas cr
         JOIN file_chunks fc ON fc.id = cr.chunk_id
         JOIN backends b ON b.id = cr.backend_id
         JOIN file_map fm ON fm.id = fc.file_id
         WHERE cr.node_id = ?1 AND cr.state = 'synced' AND fc.sha256 != ''"
    ).unwrap();
    stmt.query_map(
        rusqlite::params![&state.node_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
    ).unwrap().filter_map(|r| r.ok()).collect()
};
```

- [ ] **Step 2: Use correct path based on dedup status**

In the loop (line 57), update the destructuring and path resolution:

```rust
for (chunk_id, file_id, expected_sha256, chunk_index, backend_id, backend_path, dedup_sha256) in chunks {
    // ... volume_id lookup stays the same ...

    let path = if let Some(ref dsha) = dedup_sha256 {
        chunk::dedup_chunk_path(&backend_path, &volume_id, dsha)
    } else {
        chunk::chunk_path(&backend_path, &volume_id, file_id, chunk_index)
    };
```

- [ ] **Step 3: Build and verify**

Run: `cargo build -p vmm-san 2>&1 | head -20`
Expected: Clean compile

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/engine/integrity.rs
git commit -m "fix(coresan): integrity engine resolves dedup chunk paths"
```

---

## Task 9: Rebalancer — handle dedup chunks

**Files:**
- Modify: `apps/vmm-san/src/engine/rebalancer.rs`

- [ ] **Step 1: Read the rebalancer code**

Read `apps/vmm-san/src/engine/rebalancer.rs` fully to understand how chunks are copied between backends.

- [ ] **Step 2: Extend chunk queries to include dedup_sha256**

In `run_rebalance` and `repair_local_mirrors`, wherever the rebalancer reads a chunk from a source backend and copies it to a target backend, it needs to check `dedup_sha256`:

- If `dedup_sha256 IS NOT NULL`: read from `dedup_chunk_path` and write to `dedup_chunk_path` on the target backend. Also copy the `dedup_store` entry.
- If `dedup_sha256 IS NULL`: use the existing `chunk_path` logic (unchanged).

The key change is in the source path resolution and target path construction. Find the `chunk::chunk_path` calls in the rebalancer and add the dedup path branching.

- [ ] **Step 3: Build and verify**

Run: `cargo build -p vmm-san 2>&1 | head -20`
Expected: Clean compile

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/engine/rebalancer.rs
git commit -m "fix(coresan): rebalancer handles dedup chunk paths during evacuation"
```

---

## Task 10: TypeScript Types

**Files:**
- Modify: `apps/vmm-ui/src/api/types.ts`

- [ ] **Step 1: Add `DedupStats` interface**

After the `CoreSanVolume` interface (line 412), add:

```typescript
export interface DedupStats {
  logical_bytes: number
  physical_bytes: number
  saved_bytes: number
  dedup_ratio: number
  dedup_chunk_count: number
  pending_chunk_count: number
}
```

- [ ] **Step 2: Add dedup fields to `CoreSanVolume`**

Add to the `CoreSanVolume` interface (after `access_protocols`):

```typescript
  dedup: boolean
  dedup_stats: DedupStats | null
```

- [ ] **Step 3: Add dedup fields to `ChunkMapEntry`**

Add to the `ChunkMapEntry` interface (after `node_hostname`):

```typescript
  deduplicated: boolean
  dedup_sha256: string | null
```

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-ui/src/api/types.ts
git commit -m "feat(vmm-ui): add dedup TypeScript types"
```

---

## Task 11: CreateVolumeDialog — dedup toggle

**Files:**
- Modify: `apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx`
- Modify: `apps/vmm-ui/src/pages/StorageCoresan.tsx` (state + props)

- [ ] **Step 1: Add dedup state to StorageCoresan.tsx**

In `StorageCoresan.tsx`, find the existing form state declarations (around lines 87-94) and add:

```typescript
const [newVolDedup, setNewVolDedup] = useState(false)
```

- [ ] **Step 2: Pass dedup props to CreateVolumeDialog**

In the `CreateVolumeDialog` usage (around line 1062-1077), add the new props:

```typescript
newVolDedup={newVolDedup}
setNewVolDedup={setNewVolDedup}
```

- [ ] **Step 3: Include dedup in volume creation request**

In the `handleCreateVolume` function in `StorageCoresan.tsx`, add `dedup: newVolDedup` to the request body JSON.

- [ ] **Step 4: Update CreateVolumeDialog props and render**

In `CreateVolumeDialog.tsx`, add to the Props interface:

```typescript
  newVolDedup: boolean
  setNewVolDedup: (v: boolean) => void
```

Add to the destructured props. Then add the Toggle after the Local RAID field (after line 116, before the Access Protocols section):

```tsx
        <FormField label="Deduplication">
          <div className="flex items-center justify-between">
            <div>
              <div className="text-sm text-vmm-text">{newVolDedup ? 'Enabled' : 'Disabled'}</div>
              <div className="text-xs text-vmm-text-muted">Periodically consolidates identical data blocks to reduce storage usage.</div>
            </div>
            <button
              type="button"
              onClick={() => setNewVolDedup(!newVolDedup)}
              className={`relative w-11 h-6 rounded-full transition-colors cursor-pointer
                ${newVolDedup ? 'bg-vmm-accent' : 'bg-vmm-border-light'}`}>
              <span className={`absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform
                ${newVolDedup ? 'translate-x-5' : 'translate-x-0'}`} />
            </button>
          </div>
        </FormField>
```

- [ ] **Step 5: Commit**

```bash
git add apps/vmm-ui/src/components/coresan/CreateVolumeDialog.tsx apps/vmm-ui/src/pages/StorageCoresan.tsx
git commit -m "feat(vmm-ui): add dedup toggle to volume creation dialog"
```

---

## Task 12: Volume Detail — dedup stats display

**Files:**
- Modify: `apps/vmm-ui/src/pages/StorageCoresan.tsx`

- [ ] **Step 1: Add dedup SpecRow to volume detail**

In `StorageCoresan.tsx`, in the volume detail card (after the SpecRow grid around line 853), add a conditional dedup row:

```tsx
                {sel.dedup && sel.dedup_stats && (
                  <div className="mt-3 pt-3 border-t border-vmm-border">
                    <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                      <SpecRow label="Deduplication" value={`Enabled · ${sel.dedup_stats.dedup_ratio.toFixed(2)}x ratio`} />
                      <SpecRow label="Saved" value={formatBytes(sel.dedup_stats.saved_bytes)} />
                      <SpecRow label="Dedup Chunks" value={`${sel.dedup_stats.dedup_chunk_count}`} />
                      {sel.dedup_stats.pending_chunk_count > 0 && (
                        <SpecRow label="Pending" value={`${sel.dedup_stats.pending_chunk_count} chunks`} />
                      )}
                    </div>
                  </div>
                )}
```

- [ ] **Step 2: Add dedup savings indicator to volume card in the list**

In the volume card grid (around line 793-798), add a dedup savings line after the backends/effective row:

```tsx
                      {vol.dedup && vol.dedup_stats && vol.dedup_stats.saved_bytes > 0 && (
                        <div className="flex items-center gap-1.5 mt-1 text-xs" style={{ color: '#a855f7' }}>
                          <span>Dedup: {formatBytes(vol.dedup_stats.saved_bytes)} saved ({vol.dedup_stats.dedup_ratio.toFixed(1)}x)</span>
                        </div>
                      )}
```

- [ ] **Step 3: Add dedup toggle to inline edit volume dialog**

In the inline edit volume dialog section (around lines 1189-1233), add a dedup toggle that calls `PUT /api/volumes/{id}` with `{ dedup: newValue }`.

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-ui/src/pages/StorageCoresan.tsx
git commit -m "feat(vmm-ui): show dedup stats in volume detail and volume cards"
```

---

## Task 13: VolumeChunkMap — purple dedup colors and hover highlight

**Files:**
- Modify: `apps/vmm-ui/src/components/coresan/VolumeChunkMap.tsx`

- [ ] **Step 1: Extend ChunkHealth type and color maps**

Add `'dedup_protected'` and `'dedup_degraded'` to the `ChunkHealth` type:

```typescript
type ChunkHealth = 'protected' | 'degraded' | 'lost' | 'empty' | 'dedup_protected' | 'dedup_degraded'
```

Extend the color/border/label maps:

```typescript
const healthColor: Record<ChunkHealth, string> = {
  protected: '#22c55e',
  degraded: '#eab308',
  lost: '#ef4444',
  empty: '#1e293b',
  dedup_protected: '#a855f7',
  dedup_degraded: '#d97706',
}

const healthBorder: Record<ChunkHealth, string> = {
  protected: '#16a34a',
  degraded: '#ca8a04',
  lost: '#dc2626',
  empty: '#334155',
  dedup_protected: '#9333ea',
  dedup_degraded: '#b45309',
}

const healthLabels: Record<ChunkHealth, string> = {
  protected: 'Protected',
  degraded: 'Degraded',
  lost: 'Lost',
  empty: 'Free',
  dedup_protected: 'Dedup (Protected)',
  dedup_degraded: 'Dedup (Degraded)',
}
```

- [ ] **Step 2: Add dedup fields to ConsolidatedChunk**

```typescript
interface ConsolidatedChunk {
  file_id: number
  chunk_index: number
  rel_path: string
  size_bytes: number
  sha256: string
  health: ChunkHealth
  nodes: { node_id: string; hostname: string; state: string }[]
  deduplicated: boolean
  dedup_sha256: string | null
}
```

- [ ] **Step 3: Update consolidation logic to set dedup health**

In the consolidation `useMemo` (around line 109-156), when creating entries from the chunk data, read the `deduplicated` and `dedup_sha256` fields:

```typescript
map.set(key, {
  // ...existing fields...
  deduplicated: c.deduplicated || false,
  dedup_sha256: c.dedup_sha256 || null,
})
```

In the health computation loop, apply dedup prefix:

```typescript
for (const chunk of map.values()) {
  const syncedNodes = chunk.nodes.filter(n => n.state === 'synced').length
  const syncingNodes = chunk.nodes.filter(n => n.state === 'syncing').length
  if (syncedNodes >= required) {
    chunk.health = chunk.deduplicated ? 'dedup_protected' : 'protected'
  } else if (syncedNodes > 0 || syncingNodes > 0) {
    chunk.health = chunk.deduplicated ? 'dedup_degraded' : 'degraded'
  } else {
    chunk.health = 'lost'
  }
}
```

- [ ] **Step 4: Add hover highlight for same SHA256**

In the chunk block rendering (around line 232-254), add a highlight border when hovering over a dedup chunk and other chunks share the same `dedup_sha256`:

```typescript
const isHighlighted = hoveredChunk?.deduplicated && chunk.deduplicated
  && hoveredChunk.dedup_sha256 === chunk.dedup_sha256
  && hoveredChunk.chunk_index !== chunk.chunk_index

// In the style:
border: isHighlighted
  ? '2px solid #e9d5ff'
  : `1px solid ${healthBorder[chunk.health]}`,
```

- [ ] **Step 5: Add dedup info to hover panel**

In the hover detail panel (around line 276-323), add dedup-specific information after the SHA256 row:

```tsx
{hoveredChunk.deduplicated && (
  <>
    <span className="text-vmm-text-muted">Dedup:</span>
    <span className="text-vmm-text" style={{ color: '#a855f7' }}>
      {consolidated.filter(c => c.dedup_sha256 === hoveredChunk.dedup_sha256).length} references
    </span>
  </>
)}
```

- [ ] **Step 6: Update legend to include dedup colors**

Update the legend rendering (around line 218-225) to include the new health types. Only show dedup entries if any dedup chunks exist:

```tsx
{(['protected', 'degraded', 'lost',
   ...(stats.dedup_protected > 0 || stats.dedup_degraded > 0 ? ['dedup_protected', 'dedup_degraded'] as ChunkHealth[] : []),
   'empty'] as ChunkHealth[]).map(h => (
```

Update the stats calculation to include `dedup_protected` and `dedup_degraded` counters.

- [ ] **Step 7: Commit**

```bash
git add apps/vmm-ui/src/components/coresan/VolumeChunkMap.tsx
git commit -m "feat(vmm-ui): purple dedup chunk colors and hover highlight in allocation map"
```

---

## Task 14: StorageOverview — dedup savings in aggregate capacity

**Files:**
- Modify: `apps/vmm-ui/src/pages/StorageOverview.tsx`

- [ ] **Step 1: Calculate dedup savings from SAN status**

In `StorageOverview.tsx`, after the `sanUsedBytes` calculation (around line 87), add:

```typescript
const sanDedupSavedBytes = sanStatus?.volumes?.reduce((s: number, v: any) =>
  s + (v.dedup_stats?.saved_bytes || 0), 0) || 0
```

- [ ] **Step 2: Add dedup savings segment to the stacked bar**

In the aggregate capacity bar (around line 176-181), add a dedup savings segment after the CoreSAN segment:

```tsx
{sanDedupSavedBytes > 0 && (
  <div className="h-full" style={{
    width: `${Math.round((sanDedupSavedBytes / totalBytes) * 100)}%`,
    background: '#a855f7',
    opacity: 0.3,
  }} />
)}
```

- [ ] **Step 3: Add dedup legend entry**

In the legend (around line 183-199), add after the CoreSAN entry:

```tsx
{sanDedupSavedBytes > 0 && (
  <span className="flex items-center gap-1.5">
    <span className="w-2.5 h-2.5 rounded-full" style={{ background: '#a855f7', opacity: 0.3 }} /> Dedup Saved ({formatBytes(sanDedupSavedBytes)})
  </span>
)}
```

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-ui/src/pages/StorageOverview.tsx
git commit -m "feat(vmm-ui): show dedup savings in storage overview aggregate capacity"
```

---

## Task 15: Build & Smoke Test

- [ ] **Step 1: Full backend build**

Run: `cargo build -p vmm-san 2>&1 | tail -5`
Expected: Clean compile, no warnings related to dedup

- [ ] **Step 2: Frontend build**

Run: `cd apps/vmm-ui && npm run build 2>&1 | tail -10`
Expected: Clean build, no TypeScript errors

- [ ] **Step 3: Final commit**

If any fixes were needed, commit them:

```bash
git add -A
git commit -m "fix(coresan): resolve build issues from dedup implementation"
```
