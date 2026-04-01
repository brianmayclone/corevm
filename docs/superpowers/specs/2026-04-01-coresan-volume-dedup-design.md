# CoreSAN Volume-Level Deduplication

**Date:** 2026-04-01  
**Status:** Draft  
**Scope:** Post-process, node-local, per-volume deduplication for CoreSAN

## Overview

Adds transparent, opt-in deduplication to CoreSAN volumes. A periodic background engine consolidates identical chunks (by SHA256) into a content-addressed store, freeing duplicate disk space without modifying the write path. The UI surfaces dedup savings and visually distinguishes deduplicated chunks in the allocation map.

## Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Dedup timing | Post-process | No write-path latency impact |
| Scope | Node-local | No coordination with replication/quorum |
| Volume scope | Per-volume | Clean volume lifecycle, no cross-volume ref-counting |
| Storage layout | Content-addressed store (`.dedup/<sha256>`) | Clean separation from position-based chunks |
| Migration | None | No production data to migrate |
| Trigger | Periodic (configurable interval) | Consistent with existing engines |
| Activation | Opt-in per volume (`dedup` flag) | Not all workloads benefit |

## 1. Data Model

### New table: `dedup_store`

```sql
CREATE TABLE dedup_store (
    sha256       TEXT NOT NULL,
    volume_id    TEXT NOT NULL,
    backend_id   TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    ref_count    INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (sha256, volume_id, backend_id)
);
```

Each row represents a unique physical chunk in the content-addressed store. `ref_count` tracks how many `file_chunks` reference this data block.

### New column on `volumes`

```sql
ALTER TABLE volumes ADD COLUMN dedup INTEGER NOT NULL DEFAULT 0;
```

### New column on `file_chunks`

```sql
ALTER TABLE file_chunks ADD COLUMN dedup_sha256 TEXT DEFAULT NULL;
```

- `NULL` — chunk is in the classic path (`<file_id>/chunk_<index>`)
- Set — chunk has been deduplicated, data lives at `.dedup/<sha256>`

### Storage layout on disk

```
<backend>/.coresan/<volume_id>/
├── <file_id>/
│   ├── chunk_000000          # Non-deduplicated (classic)
│   └── chunk_000001
└── .dedup/
    ├── a3f2b8c9d1e4...       # Content-addressed chunks (SHA256 filename)
    └── f7e1c3a9b2d6...
```

### Read-path change

`read_chunk_data()` checks `file_chunks.dedup_sha256`:
- `NULL` → read from `<file_id>/chunk_<index>` (unchanged)
- Set → read from `.dedup/<sha256>`

## 2. Dedup Engine

### Module: `engine/dedup.rs`

New background engine following the pattern of `integrity.rs` and `rebalancer.rs`. Spawned as a Tokio task from `main.rs`.

### Configuration in `vmm-san.toml`

```toml
[dedup]
enabled = true
interval_secs = 300
```

`enabled = false` disables the engine entirely (regardless of per-volume flags). The volume-level `dedup` flag controls which volumes are processed.

### Algorithm per cycle

```
1. SELECT id FROM volumes WHERE dedup = 1

2. For each volume:
   a. Find duplicates:
      SELECT sha256, COUNT(*) as cnt
      FROM file_chunks
      WHERE file_id IN (SELECT id FROM file_map WHERE volume_id = ?)
        AND dedup_sha256 IS NULL
      GROUP BY sha256
      HAVING cnt > 1

   b. For each duplicated SHA256:
      - For each backend that holds replicas of these chunks
        (with mirror/stripe_mirror, a chunk may exist on multiple backends):
        - Check if .dedup/<sha256> already exists on this backend
        - If not: copy first chunk there (atomic: tmp → fsync → rename)
        - Verify SHA256 of written file
        - INSERT OR UPDATE dedup_store (ref_count = number of references)
      - UPDATE file_chunks SET dedup_sha256 = <sha256>
        for all matching chunks in this volume
      - Delete old chunk files at <file_id>/chunk_<index> on all backends

3. Cleanup: delete chunks in dedup_store with ref_count = 0
   (arises when files are deleted)
```

### Concurrency / locking

The dedup engine only operates on chunks with **no active write lease** (`file_map.write_owner IS NULL OR write_lease_until < NOW()`). This prevents conflicts with in-progress writes from the disk server.

### Interaction with other engines

| Engine | Impact |
|---|---|
| **Integrity** | Must check `.dedup/` path when `dedup_sha256` is set |
| **Rebalancer** | Must relocate `.dedup/` chunks when a backend goes offline/draining |
| **Push replicator** | Unchanged — replicates chunks as before; dedup runs independently per node |
| **Repair** | Unchanged — operates at chunk-replica level |

## 3. API Extensions

### Volume response (GET /api/volumes, GET /api/volumes/{id})

Extended with dedup fields:

```json
{
  "id": "vol-123",
  "name": "my-volume",
  "dedup": true,
  "dedup_stats": {
    "logical_bytes": 1073741824,
    "physical_bytes": 805306368,
    "saved_bytes": 268435456,
    "dedup_ratio": 1.33,
    "dedup_chunk_count": 42,
    "pending_chunk_count": 7
  }
}
```

| Field | Description |
|---|---|
| `logical_bytes` | Total size of all deduplicated chunks (what would be used without dedup) |
| `physical_bytes` | Actual space occupied in `.dedup/` store |
| `saved_bytes` | `logical_bytes - physical_bytes` |
| `dedup_ratio` | `logical_bytes / physical_bytes` (1.0 = no savings) |
| `dedup_chunk_count` | Unique chunks in content-addressed store |
| `pending_chunk_count` | Duplicate chunks not yet deduplicated |

`dedup_stats` is `null` when `dedup` is `false`.

### Stats computation (per API call)

```sql
-- logical_bytes
SELECT COALESCE(SUM(fc.size_bytes), 0)
FROM file_chunks fc
JOIN file_map fm ON fc.file_id = fm.id
WHERE fm.volume_id = ? AND fc.dedup_sha256 IS NOT NULL;

-- physical_bytes
SELECT COALESCE(SUM(size_bytes), 0)
FROM dedup_store WHERE volume_id = ?;

-- dedup_chunk_count
SELECT COUNT(*) FROM dedup_store WHERE volume_id = ?;

-- pending_chunk_count
SELECT COUNT(*) FROM (
  SELECT sha256 FROM file_chunks fc
  JOIN file_map fm ON fc.file_id = fm.id
  WHERE fm.volume_id = ? AND fc.dedup_sha256 IS NULL
  GROUP BY sha256 HAVING COUNT(*) > 1
);
```

### Volume create / update

- `POST /api/volumes` accepts `dedup: bool` (default `false`)
- `PUT /api/volumes/{id}` allows toggling dedup on/off
- Disabling stops the dedup engine for this volume — already deduplicated chunks remain in the content-addressed store (no "un-deduplication")

## 4. Chunk Map Extension

### Endpoint: `GET /api/volumes/{id}/chunk-map`

New fields per chunk entry:

```json
{
  "chunk_index": 0,
  "file_id": "abc123",
  "size_bytes": 4194304,
  "protection_status": "protected",
  "deduplicated": true,
  "dedup_sha256": "a3f2b8c9..."
}
```

### VolumeChunkMap color scheme

| Status | Color | Note |
|---|---|---|
| protected | `#22c55e` (green) | Existing |
| degraded | `#eab308` (yellow) | Existing |
| lost | `#ef4444` (red) | Existing |
| empty | `#1e293b` (dark) | Existing |
| **dedup_protected** | **`#a855f7` (purple)** | **New** |
| **dedup_degraded** | **`#d97706` (orange)** | **New** |

### Hover behavior

On hover over a deduplicated chunk:
- Show existing info: file_id, size, path, backend/node
- Additionally: **"Dedup: 4 references"** (number of chunks sharing this SHA256)
- **Highlight all chunks with the same SHA256** in the grid (e.g., brighter border) so the user can visually identify which chunks are identical

## 5. UI Changes

### Volume detail (StorageCoresan.tsx)

**Stacked ProgressBar** with three segments for dedup-enabled volumes:
- **Purple** (`#a855f7`) — Space saved by dedup
- **Blue** (existing) — Actually used space
- **Gray** — Free

**SpecRow** below capacity bar:
```
Deduplication    Enabled · Ratio 1.33x · 256 MB saved
```

Not shown when dedup is disabled.

**Metric card** (alongside existing "Synced Chunks", "Stale Chunks"):
```
Dedup Savings
256 MB (25%)
```

With pending chunks:
```
Dedup Savings          Pending
256 MB (25%)           7 chunks
```

### CreateVolumeDialog

New Toggle field after Local RAID selection:

```
Deduplication     [Toggle: Off]
```

Description text: *"Periodically consolidates identical data blocks to reduce storage usage."*

Default: Off (opt-in).

### StorageOverview

The aggregated capacity stacked ProgressBar splits the CoreSAN segment:
- **CoreSAN Used** (purple) — actual storage consumed
- **CoreSAN Dedup Saved** (purple-dim/transparent) — space reclaimed by dedup

This surfaces the dedup savings in the overall storage dashboard.

## Non-Goals

- Inline deduplication (would require write-path changes)
- Cross-volume deduplication (would require shared ref-counting)
- Cross-node deduplication (would require cluster coordination)
- Variable-size chunking / content-defined chunking
- Compression (orthogonal feature, could be added later)
- Un-deduplication when disabling the feature
