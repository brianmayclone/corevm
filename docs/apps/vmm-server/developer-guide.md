# vmm-server — Developer Guide

This guide covers the internal architecture of vmm-server for contributors and developers extending the platform.

## Architecture Overview

```
                    ┌──────────────┐
                    │   Axum Router │
                    │  (REST + WS)  │
                    └──────┬───────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │ API Layer│ │ WS Layer │ │Agent API │
        │ src/api/ │ │ src/ws/  │ │src/agent/│
        └────┬─────┘ └────┬─────┘ └────┬─────┘
             │             │             │
             ▼             ▼             ▼
        ┌──────────────────────────────────────┐
        │           Service Layer               │
        │ src/services/ (VM, Storage, Users...) │
        └──────────────────┬───────────────────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │ Database │ │ VM Engine│ │  State   │
        │ src/db/  │ │ src/vm/  │ │src/state │
        └──────────┘ └──────────┘ └──────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │   libcorevm   │
                    └──────────────┘
```

## Source Structure

```
apps/vmm-server/src/
├── main.rs             Server bootstrap, CLI args, initialization
├── config.rs           Configuration file parsing (vmm-server.toml)
├── state.rs            Central AppState (DashMap for VMs, DB pool, config)
│
├── api/                REST API endpoints
│   ├── mod.rs          Router setup — maps all routes
│   ├── auth.rs         POST /api/auth/login, GET /api/auth/me
│   ├── vms.rs          VM CRUD + lifecycle (start, stop, screenshot)
│   ├── storage.rs      Storage pools, disk images, ISO upload
│   ├── network.rs      Network interfaces and stats
│   ├── users.rs        User CRUD (admin)
│   ├── settings.rs     Server settings, timezone, security
│   ├── system.rs       System info, dashboard stats, activity log
│   └── resources.rs    Resource groups and permissions
│
├── api/guard.rs        Managed-mode middleware (blocks API when cluster-managed)
│
├── agent/              Agent API (for cluster mode)
│   ├── mod.rs          Agent router
│   └── handlers.rs     Register, status, VM/storage, migration, packages, exec
│
├── services/           Business logic
│   ├── vm_service.rs   VM creation, config management, lifecycle
│   ├── storage_service.rs  Pool, image, ISO management
│   ├── user_service.rs     User/group CRUD
│   ├── audit_service.rs    Activity logging
│   └── ...
│
├── vm/                 VM execution layer
│   ├── mod.rs          VmInstance struct, VM thread management
│   ├── builder.rs      VM builder (libcorevm configuration)
│   └── control.rs      Runtime control (start, stop, force-stop)
│
├── ws/                 WebSocket handlers
│   ├── console.rs      VGA framebuffer streaming + keyboard/mouse
│   └── terminal.rs     Interactive terminal
│
├── db/                 Database layer
│   ├── mod.rs          SQLite connection pool, migrations
│   ├── schema.rs       Table definitions
│   └── migrations/     SQL migration files
│
└── auth/               Authentication
    ├── mod.rs          JWT token creation and validation
    └── middleware.rs   Axum middleware for protected routes
```

## Key Concepts

### AppState

Central state shared across all request handlers via Axum's `Extension`:

```rust
struct AppState {
    db: SqlitePool,
    vms: DashMap<String, VmInstance>,
    config: ServerConfig,
    // ...
}
```

- `DashMap` provides lock-free concurrent access to VM instances
- Each `VmInstance` holds the VM handle, control interface, framebuffer, and serial output

### VM Lifecycle

1. **Create** — `POST /api/vms` → `vm_service::create_vm()` → stores config in SQLite
2. **Start** — `POST /api/vms/{id}/start` → `vm::builder` configures libcorevm → spawns VM thread
3. **Run** — VM thread calls `libcorevm::runtime::run()` in a loop
4. **Stop** — `POST /api/vms/{id}/stop` sends ACPI shutdown → waits for graceful exit
5. **Force Stop** — `POST /api/vms/{id}/force-stop` terminates the VM thread

### WebSocket Console

The console WebSocket (`/ws/console/{vm_id}`) streams:
- **Server → Client:** JPEG-encoded VGA framebuffer frames
- **Client → Server:** Keyboard scancodes and mouse events

Framebuffer capture runs on a timer, encoding the raw framebuffer to JPEG via the `image` crate.

### Database Migrations

SQLite migrations are embedded in the binary and run automatically at startup. Add new migrations as SQL files in `src/db/migrations/`.

### Agent Mode

When the server registers with a vmm-cluster instance, it enters "managed" mode:
- Additional `/agent/*` endpoints become active
- The cluster can remotely manage VMs and storage
- Authentication uses `X-Agent-Token` header instead of JWT

### Managed Mode Guard (`api/guard.rs`)

An Axum middleware that intercepts all `/api/*` requests when the server is cluster-managed:
- **Allowed:** `/agent/*`, `/ws/*`, `/api/system/info`, `/api/auth/login`, `/api/auth/me`
- **Blocked:** All other `/api/*` endpoints return `403` with:
  ```json
  {"error": "managed_by_cluster", "cluster_url": "...", "message": "..."}
  ```
- Prevents out-of-band changes that could desync cluster state

### Agent Endpoints

Extended agent handlers in `agent/handlers.rs`:

| Endpoint | Purpose |
|----------|---------|
| `POST /agent/register` | Register with cluster |
| `GET /agent/status` | Report health (CPU, RAM, VMs, datastores) |
| `POST /agent/vms/provision` | Create VM from cluster |
| `POST /agent/vms/start\|stop\|force-stop\|destroy` | VM lifecycle |
| `POST /agent/storage/*` | Storage operations |
| `POST /agent/migration/send` | Stream VM disks to target host |
| `POST /agent/migration/receive` | Receive VM disks from source host |
| `POST /agent/packages/check` | Check if packages are installed (detects distro) |
| `POST /agent/packages/install` | Install packages via apt/dnf/yum with optional sudo |
| `POST /agent/exec` | Execute shell command with optional timeout and sudo |

## Adding a New API Endpoint

1. Add the handler function in the appropriate `src/api/*.rs` file
2. Register the route in `src/api/mod.rs`
3. Add service logic in `src/services/` if needed
4. Add database operations in `src/db/` if needed
5. Update the audit trail in `audit_service.rs`

## Dependencies

| Crate | Purpose |
|-------|---------|
| `axum` | HTTP framework |
| `tokio` | Async runtime |
| `tower-http` | CORS, static files, tracing |
| `rusqlite` | SQLite (bundled) |
| `jsonwebtoken` | JWT signing/validation |
| `argon2` | Password hashing |
| `image` | Framebuffer JPEG encoding |
| `dashmap` | Concurrent HashMap |
| `serde` / `toml` | Serialization |
| `tracing` | Structured logging |
| `vmm-core` | Shared data models |
| `libcorevm` | VM engine |
| `vmm-term` | Terminal command registry |
