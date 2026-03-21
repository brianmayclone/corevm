//! WebSocket terminal — accepts text commands, executes via vmm-term, returns output.

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::jwt;

#[derive(Deserialize)]
pub struct TerminalQuery {
    token: String,
}

#[derive(Serialize)]
struct TerminalResponse {
    #[serde(rename = "type")]
    msg_type: String,
    lines: Vec<vmm_term::registry::OutputLine>,
}

#[derive(Serialize)]
struct CompletionResponse {
    #[serde(rename = "type")]
    msg_type: String,
    completions: Vec<String>,
}

pub async fn ws_terminal(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TerminalQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    tracing::info!("Terminal WebSocket: upgrade request received");
    // Validate JWT
    let claims = match jwt::validate_token(&q.token, &state.jwt_secret) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Terminal WebSocket: auth failed: {}", e);
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
    };

    tracing::info!("Terminal WebSocket: user '{}' authenticated", claims.username);

    ws.on_upgrade(move |socket| handle_terminal(socket, state, claims))
}

async fn handle_terminal(
    mut socket: WebSocket,
    state: Arc<AppState>,
    claims: jwt::Claims,
) {
    // Build the command registry
    let mut registry = vmm_term::registry::CommandRegistry::new();
    vmm_term::commands::register_builtins(&mut registry);

    // Send welcome message
    let welcome = TerminalResponse {
        msg_type: "output".to_string(),
        lines: vec![
            vmm_term::registry::OutputLine::info("CoreVM Terminal v0.1.0"),
            vmm_term::registry::OutputLine::info(format!("Logged in as: {} ({})", claims.username, claims.role)),
            vmm_term::registry::OutputLine::info("Type 'help' for available commands."),
            vmm_term::registry::OutputLine::stdout(""),
        ],
    };
    if let Ok(json) = serde_json::to_string(&welcome) {
        let _ = socket.send(Message::Text(json.into())).await;
    }

    // Main loop — read commands, execute, respond
    while let Some(Ok(msg)) = socket.recv().await {
        let text = match &msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };

        #[derive(Deserialize)]
        struct ClientMessage {
            #[serde(rename = "type")]
            msg_type: String,
            #[serde(default)]
            input: String,
        }

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };

        match client_msg.msg_type.as_str() {
            "exec" => {
                let input = client_msg.input.trim();
                if input.is_empty() { continue; }

                // Parse the command
                let parsed = match vmm_term::parser::parse_line(input) {
                    Some(p) => p,
                    None => continue,
                };
                let (cmd_name, args) = parsed;
                let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

                // Build context with pre-computed data
                let mut ctx = vmm_term::registry::CommandContext::new();
                ctx.env.insert("username".to_string(), claims.username.clone());
                ctx.env.insert("user_role".to_string(), claims.role.clone());

                // Pre-compute command-specific data from AppState
                populate_context(&cmd_name, &arg_refs, &state, &mut ctx);

                let result = registry.execute(&cmd_name, &arg_refs, &ctx);
                let lines = match result {
                    Ok(lines) => lines,
                    Err(lines) => lines,
                };

                let response = TerminalResponse {
                    msg_type: "output".to_string(),
                    lines,
                };
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
            "complete" => {
                let input = client_msg.input.trim();
                let parsed = vmm_term::parser::parse_line(input);

                let completions = match &parsed {
                    None => registry.complete_command(""),
                    Some((cmd, args)) if args.is_empty() && !input.ends_with(' ') => {
                        registry.complete_command(cmd)
                    }
                    Some((cmd, _args)) => {
                        // Provide VM names/IDs as argument completions
                        let partial = if input.ends_with(' ') { "" } else {
                            _args.last().map(|s| s.as_str()).unwrap_or("")
                        };
                        let mut candidates = Vec::new();
                        if cmd.starts_with("vm-") && cmd != "vm-list" {
                            for entry in state.vms.iter() {
                                if entry.value().config.name.to_lowercase().starts_with(&partial.to_lowercase()) {
                                    candidates.push(entry.value().config.name.clone());
                                }
                                if entry.key().starts_with(partial) {
                                    candidates.push(entry.key().clone());
                                }
                            }
                        }
                        candidates
                    }
                };

                let response = CompletionResponse {
                    msg_type: "completion".to_string(),
                    completions,
                };
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
            _ => {}
        }
    }

    tracing::info!("Terminal WebSocket: user '{}' disconnected", claims.username);
}

/// Pre-compute command data and inject it into the context.
fn populate_context(cmd: &str, args: &[&str], state: &AppState, ctx: &mut vmm_term::registry::CommandContext) {
    match cmd {
        "help" => {
            let mut reg = vmm_term::registry::CommandRegistry::new();
            vmm_term::commands::register_builtins(&mut reg);
            let cmds = reg.list();
            let max_name = cmds.iter().map(|(n, _, _)| n.len()).max().unwrap_or(10);
            let mut help_text = String::from("Available commands:\n\n");
            for (name, desc, usage) in &cmds {
                help_text.push_str(&format!("  {:<width$}  {}\n", name, desc, width = max_name + 2));
                help_text.push_str(&format!("  {:<width$}  Usage: {}\n", "", usage, width = max_name + 2));
            }
            ctx.env.insert("__help_text".to_string(), help_text);
        }
        "vm-list" => {
            let mut table = String::new();
            table.push_str(&format!("{:<34} {:<20} {:<10} {:<6} {:<8}\n",
                "ID", "NAME", "STATE", "VCPU", "RAM"));
            table.push_str(&format!("{}\n", "─".repeat(82)));
            for entry in state.vms.iter() {
                let vm = entry.value();
                let state_str = match vm.state {
                    crate::state::VmState::Running => "running",
                    crate::state::VmState::Stopped => "stopped",
                    crate::state::VmState::Paused => "paused",
                    crate::state::VmState::Stopping => "stopping",
                };
                table.push_str(&format!("{:<34} {:<20} {:<10} {:<6} {:<8}\n",
                    truncate(&vm.id, 32),
                    truncate(&vm.config.name, 18),
                    state_str,
                    vm.config.cpu_cores,
                    format!("{}MB", vm.config.ram_mb),
                ));
            }
            if state.vms.is_empty() {
                table.push_str("  (no virtual machines)\n");
            }
            ctx.env.insert("__vm_list".to_string(), table);
        }
        "vm-start" | "vm-stop" | "vm-force-stop" | "vm-restart" | "vm-delete" => {
            if let Some(target) = args.first() {
                let vm_id = find_vm_id(state, target);
                let result_key = format!("__{}", cmd.replace('-', "_")).replace("__", &format!("__{}_{}_", "", ""))
                    .replace("___", "__");
                // Simpler key: __vm_start_<target>, __vm_stop_<target>, etc.
                let result_key = format!("__{}_{}", cmd.replace('-', "_"), target);

                let msg = match vm_id {
                    Some(ref id) => {
                        match state.vms.get(id) {
                            Some(vm) => {
                                match cmd {
                                    "vm-start" => {
                                        if matches!(vm.state, crate::state::VmState::Running) {
                                            format!("VM '{}' is already running", vm.config.name)
                                        } else {
                                            format!("OK:VM '{}' start requested — use the web UI to start VMs", vm.config.name)
                                        }
                                    }
                                    "vm-stop" => {
                                        if matches!(vm.state, crate::state::VmState::Stopped) {
                                            format!("VM '{}' is not running", vm.config.name)
                                        } else if let Some(ctrl) = &vm.control {
                                            ctrl.request_stop();
                                            format!("OK:Stop signal sent to '{}'", vm.config.name)
                                        } else {
                                            format!("VM '{}' has no control handle", vm.config.name)
                                        }
                                    }
                                    "vm-force-stop" => {
                                        if matches!(vm.state, crate::state::VmState::Stopped) {
                                            format!("VM '{}' is not running", vm.config.name)
                                        } else if let Some(ctrl) = &vm.control {
                                            ctrl.request_stop();
                                            format!("OK:Force-stop sent to '{}'", vm.config.name)
                                        } else {
                                            format!("VM '{}' has no control handle", vm.config.name)
                                        }
                                    }
                                    "vm-restart" => {
                                        format!("OK:Restart for '{}' — stop + start via web UI", vm.config.name)
                                    }
                                    "vm-delete" => {
                                        format!("OK:Delete not available in terminal — use the web UI")
                                    }
                                    _ => format!("Unknown action"),
                                }
                            }
                            None => format!("VM '{}' not found", target),
                        }
                    }
                    None => format!("VM '{}' not found", target),
                };
                ctx.env.insert(result_key, msg);
            }
        }
        "vm-info" => {
            if let Some(target) = args.first() {
                let vm_id = find_vm_id(state, target);
                let result_key = format!("__vm_info_{}", target);

                match vm_id {
                    Some(ref id) => {
                        if let Some(vm) = state.vms.get(id) {
                            let state_str = match vm.state {
                                crate::state::VmState::Running => "Running",
                                crate::state::VmState::Stopped => "Stopped",
                                crate::state::VmState::Paused => "Paused",
                                crate::state::VmState::Stopping => "Stopping",
                            };
                            let info = format!(
                                "VM Information\n\
                                 ──────────────────────────────\n\
                                 Name:       {}\n\
                                 ID:         {}\n\
                                 State:      {}\n\
                                 CPU Cores:  {}\n\
                                 RAM:        {} MB\n\
                                 OS Type:    {:?}\n\
                                 Boot Order: {:?}\n\
                                 NIC:        {:?}\n\
                                 GPU:        {:?}",
                                vm.config.name,
                                vm.id,
                                state_str,
                                vm.config.cpu_cores,
                                vm.config.ram_mb,
                                vm.config.guest_os,
                                vm.config.boot_order,
                                vm.config.nic_model,
                                vm.config.gpu_model,
                            );
                            ctx.env.insert(result_key, info);
                        }
                    }
                    None => {}
                }
            }
        }
        "status" => {
            let uptime = state.started_at.elapsed();
            let hours = uptime.as_secs() / 3600;
            let mins = (uptime.as_secs() % 3600) / 60;
            let secs = uptime.as_secs() % 60;

            let total_vms = state.vms.len();
            let running = state.vms.iter()
                .filter(|e| matches!(e.value().state, crate::state::VmState::Running))
                .count();

            let status = format!(
                "Server Status\n\
                 ──────────────────────────────\n\
                 Version:     vmm-server v0.1.0\n\
                 Uptime:      {}h {}m {}s\n\
                 VMs Total:   {}\n\
                 VMs Running: {}\n\
                 Bind:        {}:{}",
                hours, mins, secs,
                total_vms, running,
                state.config.server.bind, state.config.server.port,
            );
            ctx.env.insert("__status".to_string(), status);
        }
        "uptime" => {
            let uptime = state.started_at.elapsed();
            let hours = uptime.as_secs() / 3600;
            let mins = (uptime.as_secs() % 3600) / 60;
            let secs = uptime.as_secs() % 60;
            ctx.env.insert("__uptime".to_string(),
                format!("Server uptime: {}h {}m {}s", hours, mins, secs));
        }
        "pool-list" => {
            let db = state.db.lock().unwrap();
            match crate::services::storage::StorageService::list_pools(&db) {
                Ok(pools) => {
                    let mut table = String::new();
                    table.push_str(&format!("{:<6} {:<16} {:<30} {:<10} {:<10}\n",
                        "ID", "NAME", "PATH", "TYPE", "SHARED"));
                    table.push_str(&format!("{}\n", "─".repeat(76)));
                    for p in &pools {
                        table.push_str(&format!("{:<6} {:<16} {:<30} {:<10} {:<10}\n",
                            p.id, p.name, truncate(&p.path, 28),
                            p.pool_type, if p.shared { "yes" } else { "no" }));
                    }
                    if pools.is_empty() {
                        table.push_str("  (no storage pools)\n");
                    }
                    ctx.env.insert("__pool_list".to_string(), table);
                }
                Err(e) => {
                    ctx.env.insert("__pool_list".to_string(), format!("Error: {}", e));
                }
            }
        }
        "disk-list" => {
            let db = state.db.lock().unwrap();
            match crate::services::storage::StorageService::list_images(&db) {
                Ok(images) => {
                    let mut table = String::new();
                    table.push_str(&format!("{:<6} {:<24} {:<10} {:<10} {:<8}\n",
                        "ID", "NAME", "SIZE", "FORMAT", "VM"));
                    table.push_str(&format!("{}\n", "─".repeat(62)));
                    for img in &images {
                        let size_str = format!("{:.1} GB", img.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0));
                        let vm_str = img.vm_id.as_deref().unwrap_or("-");
                        table.push_str(&format!("{:<6} {:<24} {:<10} {:<10} {:<8}\n",
                            img.id, truncate(&img.name, 22),
                            size_str, img.format, truncate(vm_str, 6)));
                    }
                    if images.is_empty() {
                        table.push_str("  (no disk images)\n");
                    }
                    ctx.env.insert("__disk_list".to_string(), table);
                }
                Err(e) => {
                    ctx.env.insert("__disk_list".to_string(), format!("Error: {}", e));
                }
            }
        }
        "pool-create" => {
            if args.len() >= 2 {
                let name = args[0];
                let path = args[1];
                let mut pool_type = "local".to_string();
                let mut shared = false;
                let mut i = 2;
                while i < args.len() {
                    match args[i] {
                        "--type" if i + 1 < args.len() => { pool_type = args[i+1].to_string(); i += 2; }
                        "--shared" => { shared = true; i += 1; }
                        _ => { i += 1; }
                    }
                }
                let db = state.db.lock().unwrap();
                let mount_src = if shared { Some(path) } else { None };
                match crate::services::storage::StorageService::create_pool(
                    &db, name, path, &pool_type, mount_src, None
                ) {
                    Ok(_) => ctx.env.insert("__pool_create".to_string(), format!("OK:Pool '{}' created at {}", name, path)),
                    Err(e) => ctx.env.insert("__pool_create".to_string(), format!("Error: {}", e)),
                };
            }
        }
        "pool-delete" => {
            if let Some(id_str) = args.first() {
                if let Ok(id) = id_str.parse::<i64>() {
                    let db = state.db.lock().unwrap();
                    match crate::services::storage::StorageService::delete_pool(&db, id) {
                        Ok(_) => ctx.env.insert("__pool_delete".to_string(), format!("OK:Pool {} deleted", id)),
                        Err(e) => ctx.env.insert("__pool_delete".to_string(), format!("Error: {}", e)),
                    };
                } else {
                    ctx.env.insert("__pool_delete".to_string(), "Error: invalid pool ID".to_string());
                }
            }
        }
        "pool-info" => {
            if let Some(id_str) = args.first() {
                if let Ok(id) = id_str.parse::<i64>() {
                    let db = state.db.lock().unwrap();
                    match crate::services::storage::StorageService::list_pools(&db) {
                        Ok(pools) => {
                            if let Some(p) = pools.iter().find(|p| p.id == id) {
                                let info = format!(
                                    "Storage Pool\n\
                                     ──────────────────────────────\n\
                                     ID:     {}\n\
                                     Name:   {}\n\
                                     Path:   {}\n\
                                     Type:   {}\n\
                                     Shared: {}",
                                    p.id, p.name, p.path, p.pool_type,
                                    if p.shared { "yes" } else { "no" }
                                );
                                ctx.env.insert("__pool_info".to_string(), info);
                            } else {
                                ctx.env.insert("__pool_info".to_string(), format!("Pool {} not found", id));
                            }
                        }
                        Err(e) => { ctx.env.insert("__pool_info".to_string(), format!("Error: {}", e)); }
                    }
                }
            }
        }
        "disk-create" => {
            if args.len() >= 3 {
                let name = args[0];
                let size_gb: f64 = args[1].parse().unwrap_or(0.0);
                let pool_id: i64 = args[2].parse().unwrap_or(0);
                if size_gb <= 0.0 || pool_id <= 0 {
                    ctx.env.insert("__disk_create".to_string(), "Error: invalid size or pool ID".to_string());
                } else {
                    let db = state.db.lock().unwrap();
                    match crate::services::storage::StorageService::create_image(
                        &db, name, size_gb as u64, pool_id, 2048
                    ) {
                        Ok(_) => ctx.env.insert("__disk_create".to_string(), format!("OK:Disk '{}' ({} GB) created in pool {}", name, size_gb, pool_id)),
                        Err(e) => ctx.env.insert("__disk_create".to_string(), format!("Error: {}", e)),
                    };
                }
            }
        }
        "disk-delete" => {
            if let Some(id_str) = args.first() {
                if let Ok(id) = id_str.parse::<i64>() {
                    let db = state.db.lock().unwrap();
                    match crate::services::storage::StorageService::delete_image(&db, id) {
                        Ok(_) => ctx.env.insert("__disk_delete".to_string(), format!("OK:Disk {} deleted", id)),
                        Err(e) => ctx.env.insert("__disk_delete".to_string(), format!("Error: {}", e)),
                    };
                } else {
                    ctx.env.insert("__disk_delete".to_string(), "Error: invalid disk ID".to_string());
                }
            }
        }
        "disk-resize" => {
            if args.len() >= 2 {
                let id: i64 = args[0].parse().unwrap_or(0);
                let new_gb: f64 = args[1].parse().unwrap_or(0.0);
                if id <= 0 || new_gb <= 0.0 {
                    ctx.env.insert("__disk_resize".to_string(), "Error: invalid ID or size".to_string());
                } else {
                    let db = state.db.lock().unwrap();
                    match crate::services::storage::StorageService::resize_image(
                        &db, id, (new_gb * 1024.0 * 1024.0 * 1024.0) as u64
                    ) {
                        Ok(_) => ctx.env.insert("__disk_resize".to_string(), format!("OK:Disk {} resized to {} GB", id, new_gb)),
                        Err(e) => ctx.env.insert("__disk_resize".to_string(), format!("Error: {}", e)),
                    };
                }
            }
        }
        "iso-list" => {
            let db = state.db.lock().unwrap();
            match crate::services::storage::StorageService::list_isos(&db) {
                Ok(isos) => {
                    let mut table = String::new();
                    table.push_str(&format!("{:<6} {:<30} {:<12} {:<30}\n",
                        "ID", "NAME", "SIZE", "PATH"));
                    table.push_str(&format!("{}\n", "─".repeat(80)));
                    for iso in &isos {
                        let size_str = format!("{:.1} MB", iso.size_bytes as f64 / (1024.0 * 1024.0));
                        table.push_str(&format!("{:<6} {:<30} {:<12} {:<30}\n",
                            iso.id, truncate(&iso.name, 28), size_str, truncate(&iso.path, 28)));
                    }
                    if isos.is_empty() {
                        table.push_str("  (no ISO images)\n");
                    }
                    ctx.env.insert("__iso_list".to_string(), table);
                }
                Err(e) => { ctx.env.insert("__iso_list".to_string(), format!("Error: {}", e)); }
            }
        }
        // ── Resource Groups ──────────────────────────────────────────────
        "rg-list" => {
            let db = state.db.lock().unwrap();
            match crate::services::resource_groups::ResourceGroupService::list(&db) {
                Ok(groups) => {
                    let mut table = String::new();
                    table.push_str(&format!("{:<6} {:<24} {:<40}\n", "ID", "NAME", "DESCRIPTION"));
                    table.push_str(&format!("{}\n", "─".repeat(72)));
                    for g in &groups {
                        table.push_str(&format!("{:<6} {:<24} {:<40}\n",
                            g.id, truncate(&g.name, 22),
                            if g.description.is_empty() { "-" } else { truncate(&g.description, 38) }));
                    }
                    if groups.is_empty() {
                        table.push_str("  (no resource groups)\n");
                    }
                    ctx.env.insert("__rg_list".to_string(), table);
                }
                Err(e) => { ctx.env.insert("__rg_list".to_string(), format!("Error: {}", e)); }
            }
        }
        "rg-create" => {
            if !args.is_empty() {
                let name = args[0];
                let desc = if args.len() > 1 { Some(args[1..].join(" ")) } else { None };
                let db = state.db.lock().unwrap();
                let desc_str = desc.unwrap_or_default();
                match crate::services::resource_groups::ResourceGroupService::create(
                    &db, name, &desc_str
                ) {
                    Ok(_) => ctx.env.insert("__rg_create".to_string(), format!("OK:Resource group '{}' created", name)),
                    Err(e) => ctx.env.insert("__rg_create".to_string(), format!("Error: {}", e)),
                };
            }
        }
        "rg-delete" => {
            if let Some(id_str) = args.first() {
                if let Ok(id) = id_str.parse::<i64>() {
                    let db = state.db.lock().unwrap();
                    match crate::services::resource_groups::ResourceGroupService::delete(&db, id) {
                        Ok(_) => ctx.env.insert("__rg_delete".to_string(), format!("OK:Resource group {} deleted", id)),
                        Err(e) => ctx.env.insert("__rg_delete".to_string(), format!("Error: {}", e)),
                    };
                } else {
                    ctx.env.insert("__rg_delete".to_string(), "Error: invalid group ID".to_string());
                }
            }
        }
        "rg-info" => {
            if let Some(id_str) = args.first() {
                if let Ok(id) = id_str.parse::<i64>() {
                    let db = state.db.lock().unwrap();
                    match crate::services::resource_groups::ResourceGroupService::list(&db) {
                        Ok(groups) => {
                            if let Some(g) = groups.iter().find(|g| g.id == id) {
                                let mut info = format!(
                                    "Resource Group\n\
                                     ──────────────────────────────\n\
                                     ID:          {}\n\
                                     Name:        {}\n\
                                     Description: {}",
                                    g.id, g.name,
                                    if g.description.is_empty() { "-" } else { &g.description }
                                );
                                // Show permissions
                                match crate::services::resource_groups::ResourceGroupService::get_permissions(&db, id) {
                                    Ok(perms) => {
                                        info.push_str("\n\nPermissions:");
                                        if perms.is_empty() {
                                            info.push_str("\n  (none assigned)");
                                        }
                                        for p in &perms {
                                            info.push_str(&format!("\n  Group {}: {}", p.group_id, p.permissions.join(", ")));
                                        }
                                    }
                                    Err(_) => {}
                                }
                                ctx.env.insert("__rg_info".to_string(), info);
                            }
                        }
                        Err(e) => { ctx.env.insert("__rg_info".to_string(), format!("Error: {}", e)); }
                    }
                }
            }
        }
        "rg-assign" => {
            if args.len() >= 2 {
                let vm_target = args[0];
                let rg_id: i64 = args[1].parse().unwrap_or(0);
                if rg_id <= 0 {
                    ctx.env.insert("__rg_assign".to_string(), "Error: invalid resource group ID".to_string());
                } else {
                    match find_vm_id(state, vm_target) {
                        Some(vm_id) => {
                            let db = state.db.lock().unwrap();
                            match db.execute(
                                "UPDATE vms SET resource_group_id = ?1 WHERE id = ?2",
                                rusqlite::params![rg_id, vm_id],
                            ) {
                                Ok(_) => ctx.env.insert("__rg_assign".to_string(), format!("OK:VM assigned to resource group {}", rg_id)),
                                Err(e) => ctx.env.insert("__rg_assign".to_string(), format!("Error: {}", e)),
                            };
                        }
                        None => { ctx.env.insert("__rg_assign".to_string(), format!("Error: VM '{}' not found", vm_target)); }
                    }
                }
            }
        }
        "rg-perms" => {
            if let Some(id_str) = args.first() {
                if let Ok(rg_id) = id_str.parse::<i64>() {
                    if args.len() >= 3 && args[1] == "set" {
                        // rg-perms <rg-id> set <group-id> <perms...>
                        let group_id: i64 = args[2].parse().unwrap_or(0);
                        let perms: Vec<String> = args[3..].iter().map(|s| s.to_string()).collect();
                        if group_id <= 0 || perms.is_empty() {
                            ctx.env.insert("__rg_perms_set".to_string(), "Error: invalid group ID or empty permissions".to_string());
                        } else {
                            let db = state.db.lock().unwrap();
                            match crate::services::resource_groups::ResourceGroupService::set_permissions(
                                &db, rg_id, group_id, &perms
                            ) {
                                Ok(_) => ctx.env.insert("__rg_perms_set".to_string(), format!("OK:Permissions set for group {} on resource group {}", group_id, rg_id)),
                                Err(e) => ctx.env.insert("__rg_perms_set".to_string(), format!("Error: {}", e)),
                            };
                        }
                    } else {
                        // Show current permissions
                        let db = state.db.lock().unwrap();
                        match crate::services::resource_groups::ResourceGroupService::get_permissions(&db, rg_id) {
                            Ok(perms) => {
                                let mut out = format!("Permissions for resource group {}:\n──────────────────────────────\n", rg_id);
                                if perms.is_empty() {
                                    out.push_str("  (no permissions assigned)\n");
                                }
                                for p in &perms {
                                    out.push_str(&format!("  Group {}: {}\n", p.group_id, p.permissions.join(", ")));
                                }
                                ctx.env.insert("__rg_perms".to_string(), out);
                            }
                            Err(e) => { ctx.env.insert("__rg_perms".to_string(), format!("Error: {}", e)); }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Find a VM ID by ID prefix or name (case-insensitive).
fn find_vm_id(state: &AppState, target: &str) -> Option<String> {
    if state.vms.contains_key(target) {
        return Some(target.to_string());
    }
    for entry in state.vms.iter() {
        if entry.key().starts_with(target) {
            return Some(entry.key().clone());
        }
    }
    let target_lower = target.to_lowercase();
    for entry in state.vms.iter() {
        if entry.value().config.name.to_lowercase() == target_lower {
            return Some(entry.key().clone());
        }
    }
    None
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
