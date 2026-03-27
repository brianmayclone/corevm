# vmmctl — Developer Guide

Architecture, internals, and extension guide for the vmmctl CLI tool.

## Overview

vmmctl is a pure REST API client — it does not link against libcorevm or run VMs directly. It communicates with vmm-server (or vmm-cluster) via HTTP/HTTPS using JWT bearer tokens for authentication.

```
vmmctl (CLI)
  │
  ├─ config.rs     ~/.vmmctl/config.toml + tokens/
  ├─ client.rs     reqwest HTTP client
  ├─ auth.rs       JWT decode, login types
  ├─ output.rs     table / JSON formatting
  └─ commands/     one module per subcommand group
       │
       ↓ HTTPS + JWT
  vmm-server / vmm-cluster (REST API)
```

## Source Structure

```
apps/vmmctl/
├── Cargo.toml
└── src/
    ├── main.rs              CLI entry point, clap parser
    ├── client.rs            HTTP client (GET/POST/PUT/DELETE, multipart upload)
    ├── config.rs            Multi-context config + token storage
    ├── auth.rs              Login request/response types, JWT payload decode
    ├── output.rs            Output formatting (table, JSON)
    └── commands/
        ├── mod.rs           Module declarations
        ├── login.rs         vmmctl login
        ├── auth_status.rs   vmmctl auth
        ├── config_cmd.rs    vmmctl config (set-server, use-context, etc.)
        ├── vm.rs            vmmctl vm (list, create, start, stop, delete, etc.)
        ├── system.rs        vmmctl system (info, stats, activity)
        ├── storage.rs       vmmctl storage (pool, disk, iso)
        ├── network.rs       vmmctl network (list, bridge)
        ├── user.rs          vmmctl user (list, create, delete, password)
        ├── cluster.rs       vmmctl cluster (host, drs, migrate)
        └── api_access.rs    vmmctl api-access (status, enable, disable)
```

## Key Components

### CLI Parser (`main.rs`)

Uses [clap](https://docs.rs/clap) with derive macros. The top-level `Cli` struct defines global options (`--output`, `--insecure`, `--server`), and `Commands` enum dispatches to subcommand handlers.

Each subcommand module exposes an `execute(cli: &Cli, command: &XxxCommands)` function.

### HTTP Client (`client.rs`)

Built on [reqwest](https://docs.rs/reqwest) with rustls for TLS. Provides typed methods:

- `get<T>(path)` — GET with JSON deserialization
- `post<B, T>(path, body)` — POST with JSON body
- `post_empty<T>(path)` — POST with no body
- `put<B, T>(path, body)` — PUT with JSON body
- `delete<T>(path)` — DELETE
- `get_bytes(path)` — GET raw bytes (screenshots)
- `upload_file<T>(path, file_path)` — Multipart file upload

Error handling parses JSON error responses from the server and produces human-readable messages.

### Config & Token Management (`config.rs`)

- Config file: `~/.vmmctl/config.toml`
- Token files: `~/.vmmctl/tokens/<context-name>` (permissions `0600`)
- Environment override: `VMMCTL_TOKEN`
- Context resolution: CLI `--server` flag > current context > error

### Output Formatting (`output.rs`)

Uses [tabled](https://docs.rs/tabled) for table rendering. All list commands support:
- `table` — human-readable table with borders
- `json` — `serde_json::to_string_pretty`
- `wide` — extended table (same as table, reserved for future columns)

Response types implement both `serde::Deserialize` (for API response parsing) and `tabled::Tabled` (for table rendering).

## Adding a New Command

### 1. Create the Command Module

Create `src/commands/mycommand.rs`:

```rust
use clap::Subcommand;
use serde::{Serialize, Deserialize};
use tabled::Tabled;
use crate::Cli;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum MyCommands {
    /// List items
    List,
    /// Create an item
    Create {
        #[arg(long)]
        name: String,
    },
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct MyItem {
    pub id: String,
    pub name: String,
}

pub async fn execute(cli: &Cli, command: &MyCommands) -> Result<(), String> {
    let client = ApiClient::from_cli(cli)?;

    match command {
        MyCommands::List => {
            let items: Vec<MyItem> = client.get("/api/myitems").await?;
            output::print_list(&items, &cli.output, cli.no_header);
        }
        MyCommands::Create { name } => {
            let resp: serde_json::Value = client.post("/api/myitems",
                &serde_json::json!({"name": name})).await?;
            output::print_ok(&format!("Created: {}", name), &cli.output);
        }
    }
    Ok(())
}
```

### 2. Register the Module

In `src/commands/mod.rs`:
```rust
pub mod mycommand;
```

### 3. Add to CLI Parser

In `src/main.rs`, add to the `Commands` enum:
```rust
/// My new feature
MyCommand {
    #[command(subcommand)]
    command: commands::mycommand::MyCommands,
},
```

And to the match block:
```rust
Commands::MyCommand { command } => commands::mycommand::execute(&cli, command).await,
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing with derive |
| `reqwest` | HTTP client with rustls TLS |
| `tokio` | Async runtime |
| `serde` / `serde_json` | JSON serialization |
| `toml` | Config file parsing |
| `tabled` | Table output formatting |
| `dirs` | Home directory detection |
| `rpassword` | Secure password prompting |
| `base64` | JWT payload decoding |
| `chrono` | Token expiry formatting |
| `colored` | Terminal colors |
| `vmm-core` | Shared data models (VmConfig, enums) |

## Server-Side Components

vmmctl's functionality depends on these server-side features:

### API Access Control

The server has an `[api]` config section and middleware (`auth/api_access.rs`) that can block CLI requests:

```toml
[api]
cli_access_enabled = true
allowed_ips = []
```

Endpoints:
- `GET /api/settings/api-access` — read current settings
- `PUT /api/settings/api-access` — update settings (admin only)

### TLS

The server supports TLS via `axum-server` with rustls:

```toml
[server]
tls_cert = "/etc/vmm/server.crt"
tls_key = "/etc/vmm/server.key"
```

### Appliance Integration

The CoreVM appliance ISO includes:
- **Installer wizard**: "API Access" screen (between Ports and Certs)
- **DCUI console**: F9 — API Access dialog for toggling CLI access post-installation
