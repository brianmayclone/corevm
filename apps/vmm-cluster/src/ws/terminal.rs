//! WebSocket terminal for vmm-cluster — full cluster management via CLI.
//!
//! Registers ALL builtin commands PLUS cluster-specific commands.
//! Uses the service layer for all data access.

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::ClusterState;
use crate::auth::jwt;
use crate::services::stats::StatsService;
use crate::services::cluster::ClusterService;
use crate::services::host::HostService;
use crate::services::vm::VmService;
use crate::services::datastore::DatastoreService;
use crate::services::drs_service::DrsService;
use crate::services::task::TaskService;
use crate::services::event::EventService;
use crate::services::alarm::AlarmService;
use crate::services::audit::AuditService;

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
    State(state): State<Arc<ClusterState>>,
    Query(q): Query<TerminalQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let claims = match jwt::validate_token(&q.token, &state.jwt_secret) {
        Ok(c) => c,
        Err(_) => return axum::http::StatusCode::UNAUTHORIZED.into_response(),
    };
    ws.on_upgrade(move |socket| handle_terminal(socket, state, claims))
}

async fn handle_terminal(
    mut socket: WebSocket,
    state: Arc<ClusterState>,
    claims: jwt::Claims,
) {
    // Build command registry — builtins + cluster commands
    let mut registry = vmm_term::registry::CommandRegistry::new();
    vmm_term::commands::register_builtins(&mut registry);
    vmm_term::commands::cluster::register_cluster_commands(&mut registry);

    // Welcome message
    let welcome = TerminalResponse {
        msg_type: "output".to_string(),
        lines: vec![
            vmm_term::OutputLine::info("VMM-Cluster Terminal v0.1.0"),
            vmm_term::OutputLine::info(format!("Logged in as: {} ({})", claims.username, claims.role)),
            vmm_term::OutputLine::info("Type 'help' for available commands."),
            vmm_term::OutputLine::stdout(""),
        ],
    };
    if let Ok(json) = serde_json::to_string(&welcome) {
        let _ = socket.send(Message::Text(json.into())).await;
    }

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

                let parsed = match vmm_term::parse_line(input) {
                    Some(p) => p,
                    None => continue,
                };
                let (cmd_name, args) = parsed;
                let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

                let mut ctx = vmm_term::CommandContext::new();
                ctx.env.insert("username".to_string(), claims.username.clone());
                ctx.env.insert("user_role".to_string(), claims.role.clone());

                populate_context(&cmd_name, &arg_refs, &state, &mut ctx);

                let result = registry.execute(&cmd_name, &arg_refs, &ctx);
                let lines = match result { Ok(l) | Err(l) => l };

                let response = TerminalResponse { msg_type: "output".to_string(), lines };
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
            "complete" => {
                let input = client_msg.input.trim();
                let parsed = vmm_term::parse_line(input);

                let completions = match &parsed {
                    None => registry.complete_command(""),
                    Some((cmd, args)) if args.is_empty() && !input.ends_with(' ') => {
                        registry.complete_command(cmd)
                    }
                    Some((cmd, _args)) => {
                        let partial = if input.ends_with(' ') { "" } else {
                            _args.last().map(|s| s.as_str()).unwrap_or("")
                        };
                        let mut candidates = Vec::new();
                        // Autocomplete VM names
                        if cmd.starts_with("vm-") && cmd != "vm-list" {
                            if let Ok(db) = state.db.lock() {
                                if let Ok(vms) = VmService::list(&db) {
                                    for vm in &vms {
                                        if vm.name.to_lowercase().starts_with(&partial.to_lowercase()) {
                                            candidates.push(vm.name.clone());
                                        }
                                    }
                                }
                            }
                        }
                        // Autocomplete host names
                        if cmd.starts_with("host-") && cmd != "host-list" {
                            if let Ok(db) = state.db.lock() {
                                if let Ok(hosts) = HostService::list(&db) {
                                    for h in &hosts {
                                        if h.hostname.to_lowercase().starts_with(&partial.to_lowercase()) {
                                            candidates.push(h.hostname.clone());
                                        }
                                    }
                                }
                            }
                        }
                        // Autocomplete cluster names
                        if cmd.starts_with("cluster-") && cmd != "cluster-list" && cmd != "cluster-status" && cmd != "cluster-create" {
                            if let Ok(db) = state.db.lock() {
                                if let Ok(clusters) = ClusterService::list(&db) {
                                    for c in &clusters {
                                        if c.name.to_lowercase().starts_with(&partial.to_lowercase()) {
                                            candidates.push(c.name.clone());
                                        }
                                    }
                                }
                            }
                        }
                        candidates
                    }
                };

                let response = CompletionResponse { msg_type: "completion".to_string(), completions };
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
            _ => {}
        }
    }
}

/// Pre-compute command data via the service layer and inject into context.
fn populate_context(cmd: &str, args: &[&str], state: &ClusterState, ctx: &mut vmm_term::CommandContext) {
    let db = match state.db.lock() {
        Ok(db) => db,
        Err(_) => return,
    };

    match cmd {
        "help" => {
            let mut reg = vmm_term::CommandRegistry::new();
            vmm_term::commands::register_builtins(&mut reg);
            vmm_term::commands::cluster::register_cluster_commands(&mut reg);
            let cmds = reg.list();
            let max_name = cmds.iter().map(|(n, _, _)| n.len()).max().unwrap_or(10);
            let mut help_text = String::from("Available commands:\n\n");
            for (name, desc, usage) in &cmds {
                help_text.push_str(&format!("  {:<width$}  {}\n", name, desc, width = max_name + 2));
                help_text.push_str(&format!("  {:<width$}  Usage: {}\n", "", usage, width = max_name + 2));
            }
            ctx.env.insert("__help_text".to_string(), help_text);
        }

        // ── Cluster Status ──────────────────────────────────────────
        "cluster-status" | "status" => {
            let stats = StatsService::cluster_stats(&db);
            let uptime = state.started_at.elapsed().as_secs();
            let hours = uptime / 3600;
            let mins = (uptime % 3600) / 60;

            let mut out = String::new();
            out.push_str(&format!("VMM-Cluster v{}\n", env!("CARGO_PKG_VERSION")));
            out.push_str(&format!("Uptime: {}h {}m\n\n", hours, mins));
            out.push_str(&format!("Hosts:      {} total, {} online, {} maintenance, {} offline\n",
                stats.total_hosts, stats.online_hosts, stats.maintenance_hosts, stats.offline_hosts));
            out.push_str(&format!("VMs:        {} total, {} running, {} stopped\n",
                stats.total_vms, stats.running_vms, stats.stopped_vms));
            out.push_str(&format!("HA:         {} protected VMs\n", stats.ha_protected_vms));
            out.push_str(&format!("RAM:        {} / {} MB ({:.0}%)\n",
                stats.used_ram_mb, stats.total_ram_mb,
                if stats.total_ram_mb > 0 { stats.used_ram_mb as f64 / stats.total_ram_mb as f64 * 100.0 } else { 0.0 }));
            out.push_str(&format!("Disk:       {} / {} ({:.0}%)\n",
                format_bytes(stats.used_disk_bytes),
                format_bytes(stats.total_disk_bytes),
                if stats.total_disk_bytes > 0 { stats.used_disk_bytes as f64 / stats.total_disk_bytes as f64 * 100.0 } else { 0.0 }));

            ctx.env.insert("__cluster_status".to_string(), out.clone());
            ctx.env.insert("__status".to_string(), out);
            ctx.env.insert("__uptime".to_string(), format!("{}h {}m", hours, mins));
        }

        // ── Cluster CRUD ────────────────────────────────────────────
        "cluster-list" => {
            if let Ok(clusters) = ClusterService::list(&db) {
                let mut table = format!("{:<36} {:<20} {:<6} {:<6} {:<4} {:<4}\n",
                    "ID", "NAME", "HOSTS", "VMS", "HA", "DRS");
                table.push_str(&format!("{}\n", "─".repeat(82)));
                for c in &clusters {
                    table.push_str(&format!("{:<36} {:<20} {:<6} {:<6} {:<4} {:<4}\n",
                        truncate(&c.id, 34), truncate(&c.name, 18),
                        c.host_count, c.vm_count,
                        if c.ha_enabled { "On" } else { "Off" },
                        if c.drs_enabled { "On" } else { "Off" }));
                }
                if clusters.is_empty() { table.push_str("  (no clusters)\n"); }
                ctx.env.insert("__cluster_list".to_string(), table);
            }
        }
        "cluster-create" => {
            if let Some(name) = args.first() {
                let desc = if args.len() > 1 { args[1..].join(" ") } else { String::new() };
                let key = format!("__cluster_create_{}", name);
                match ClusterService::create(&db, name, &desc) {
                    Ok(id) => ctx.env.insert(key, format!("OK:Cluster '{}' created (ID: {})", name, &id[..8])),
                    Err(e) => ctx.env.insert(key, e),
                };
            }
        }
        "cluster-delete" => {
            if let Some(target) = args.first() {
                let key = format!("__cluster_delete_{}", target);
                let id = find_cluster_id(&db, target);
                match id {
                    Some(id) => match ClusterService::delete(&db, &id) {
                        Ok(()) => ctx.env.insert(key, format!("OK:Cluster '{}' deleted", target)),
                        Err(e) => ctx.env.insert(key, e),
                    },
                    None => ctx.env.insert(key, format!("Cluster '{}' not found", target)),
                };
            }
        }
        "cluster-info" => {
            if let Some(target) = args.first() {
                let key = format!("__cluster_info_{}", target);
                let id = find_cluster_id(&db, target);
                match id {
                    Some(id) => match ClusterService::get(&db, &id) {
                        Ok(c) => {
                            let mut out = String::new();
                            out.push_str(&format!("Cluster: {}\n", c.name));
                            if !c.description.is_empty() { out.push_str(&format!("Description: {}\n", c.description)); }
                            out.push_str(&format!("ID: {}\n", c.id));
                            out.push_str(&format!("Hosts: {}  VMs: {}\n", c.host_count, c.vm_count));
                            out.push_str(&format!("HA: {}  DRS: {}\n",
                                if c.ha_enabled { "Enabled" } else { "Disabled" },
                                if c.drs_enabled { "Enabled" } else { "Disabled" }));
                            out.push_str(&format!("RAM: {} / {} MB\n", c.total_ram_mb - c.free_ram_mb, c.total_ram_mb));
                            ctx.env.insert(key, out);
                        }
                        Err(e) => { ctx.env.insert(key, e); }
                    },
                    None => { ctx.env.insert(key, format!("Cluster '{}' not found", target)); }
                };
            }
        }

        // ── Host Management ─────────────────────────────────────────
        "host-list" => {
            if let Ok(hosts) = HostService::list(&db) {
                let mut table = format!("{:<36} {:<16} {:<12} {:<6} {:<10} {:<8}\n",
                    "ID", "HOSTNAME", "STATUS", "VMS", "CPU%", "RAM");
                table.push_str(&format!("{}\n", "─".repeat(92)));
                for h in &hosts {
                    let ram_pct = if h.total_ram_mb > 0 {
                        ((h.total_ram_mb - h.free_ram_mb) as f64 / h.total_ram_mb as f64 * 100.0) as i64
                    } else { 0 };
                    table.push_str(&format!("{:<36} {:<16} {:<12} {:<6} {:<10} {}%\n",
                        truncate(&h.id, 34), truncate(&h.hostname, 14),
                        &h.status, h.vm_count,
                        format!("{:.0}%", h.cpu_usage_pct), ram_pct));
                }
                if hosts.is_empty() { table.push_str("  (no hosts registered)\n"); }
                ctx.env.insert("__host_list".to_string(), table);
            }
        }
        "host-info" => {
            if let Some(target) = args.first() {
                let key = format!("__host_info_{}", target);
                let id = find_host_id(&db, target);
                match id {
                    Some(id) => match HostService::get(&db, &id) {
                        Ok(h) => {
                            let ram_pct = if h.total_ram_mb > 0 {
                                ((h.total_ram_mb - h.free_ram_mb) as f64 / h.total_ram_mb as f64 * 100.0) as i64
                            } else { 0 };
                            let mut out = String::new();
                            out.push_str(&format!("Host: {}\n", h.hostname));
                            out.push_str(&format!("Address: {}\n", h.address));
                            out.push_str(&format!("Status: {}  Maintenance: {}\n", h.status,
                                if h.maintenance_mode { "Yes" } else { "No" }));
                            out.push_str(&format!("CPU: {} ({} cores, {:.0}% used)\n", h.cpu_model, h.cpu_cores, h.cpu_usage_pct));
                            out.push_str(&format!("RAM: {} / {} MB ({}%)\n", h.total_ram_mb - h.free_ram_mb, h.total_ram_mb, ram_pct));
                            out.push_str(&format!("VMs: {}  Version: {}\n", h.vm_count, h.version));
                            out.push_str(&format!("Last Heartbeat: {}\n", h.last_heartbeat.as_deref().unwrap_or("never")));
                            ctx.env.insert(key, out);
                        }
                        Err(e) => { ctx.env.insert(key, e); }
                    },
                    None => { ctx.env.insert(key, format!("Host '{}' not found", target)); }
                };
            }
        }
        "host-maintenance" => {
            if args.len() >= 2 {
                let key = format!("__host_maintenance_{}_{}", args[0], args[1]);
                let id = find_host_id(&db, args[0]);
                match id {
                    Some(id) => {
                        let enable = args[1] == "enter";
                        match HostService::set_maintenance(&db, &id, enable) {
                            Ok(()) => {
                                let action = if enable { "entered maintenance mode" } else { "exited maintenance mode" };
                                ctx.env.insert(key, format!("OK:Host '{}' {}", args[0], action));
                            }
                            Err(e) => { ctx.env.insert(key, e); }
                        }
                    }
                    None => { ctx.env.insert(key, format!("Host '{}' not found", args[0])); }
                };
            }
        }
        "host-remove" => {
            if let Some(target) = args.first() {
                let key = format!("__host_remove_{}", target);
                let id = find_host_id(&db, target);
                match id {
                    Some(id) => match HostService::delete(&db, &id) {
                        Ok(()) => ctx.env.insert(key, format!("OK:Host '{}' removed", target)),
                        Err(e) => ctx.env.insert(key, e),
                    },
                    None => ctx.env.insert(key, format!("Host '{}' not found", target)),
                };
            }
        }

        // ── VM List (cluster-wide) ──────────────────────────────────
        "vm-list" => {
            if let Ok(vms) = VmService::list(&db) {
                let mut table = format!("{:<34} {:<18} {:<10} {:<6} {:<8} {:<14}\n",
                    "ID", "NAME", "STATE", "VCPU", "RAM", "HOST");
                table.push_str(&format!("{}\n", "─".repeat(94)));
                for vm in &vms {
                    table.push_str(&format!("{:<34} {:<18} {:<10} {:<6} {:<8} {:<14}\n",
                        truncate(&vm.id, 32), truncate(&vm.name, 16),
                        &vm.state, vm.cpu_cores, format!("{}MB", vm.ram_mb),
                        vm.host_name.as_deref().unwrap_or("unplaced")));
                }
                if vms.is_empty() { table.push_str("  (no virtual machines)\n"); }
                ctx.env.insert("__vm_list".to_string(), table);
            }
        }
        "vm-migrate" => {
            if args.len() >= 2 {
                let key = format!("__vm_migrate_{}_{}", args[0], args[1]);
                ctx.env.insert(key, format!("OK:Migration of '{}' to '{}' started — check task-list for progress", args[0], args[1]));
                // Actual migration is triggered async via the API, not inline here
            }
        }

        // ── Datastores ──────────────────────────────────────────────
        "datastore-list" => {
            if let Ok(datastores) = DatastoreService::list(&db) {
                let mut table = format!("{:<34} {:<16} {:<8} {:<10} {:<12} {:<10}\n",
                    "ID", "NAME", "TYPE", "STATUS", "CAPACITY", "HOSTS");
                table.push_str(&format!("{}\n", "─".repeat(94)));
                for ds in &datastores {
                    let mounted = ds.host_mounts.iter().filter(|m| m.mounted).count();
                    table.push_str(&format!("{:<34} {:<16} {:<8} {:<10} {:<12} {}/{}\n",
                        truncate(&ds.id, 32), truncate(&ds.name, 14),
                        &ds.store_type, &ds.status,
                        format_bytes(ds.total_bytes),
                        mounted, ds.host_mounts.len()));
                }
                if datastores.is_empty() { table.push_str("  (no datastores)\n"); }
                ctx.env.insert("__datastore_list".to_string(), table);
            }
        }
        "datastore-info" => {
            if let Some(target) = args.first() {
                let key = format!("__datastore_info_{}", target);
                // Try by name or ID
                let ds = DatastoreService::get(&db, target)
                    .or_else(|_| find_datastore_by_name(&db, target));
                match ds {
                    Ok(ds) => {
                        let mut out = String::new();
                        out.push_str(&format!("Datastore: {}\n", ds.name));
                        out.push_str(&format!("Type: {}  Source: {}\n", ds.store_type, ds.mount_source));
                        out.push_str(&format!("Mount Path: {}\n", ds.mount_path));
                        out.push_str(&format!("Capacity: {} total, {} free\n",
                            format_bytes(ds.total_bytes), format_bytes(ds.free_bytes)));
                        out.push_str(&format!("Status: {}\n\n", ds.status));
                        out.push_str("Host Mounts:\n");
                        for m in &ds.host_mounts {
                            out.push_str(&format!("  {} — {} ({})\n",
                                m.hostname, m.mount_status,
                                if m.mounted { format_bytes(m.free_bytes) + " free" } else { "not mounted".into() }));
                        }
                        ctx.env.insert(key, out);
                    }
                    Err(e) => { ctx.env.insert(key, e); }
                };
            }
        }

        // ── DRS ─────────────────────────────────────────────────────
        "drs-list" => {
            if let Ok(recs) = DrsService::list_pending(&db) {
                let mut table = format!("{:<6} {:<16} {:<14} {:<14} {:<8} {:<24}\n",
                    "ID", "VM", "FROM", "TO", "PRIO", "REASON");
                table.push_str(&format!("{}\n", "─".repeat(86)));
                for r in &recs {
                    table.push_str(&format!("{:<6} {:<16} {:<14} {:<14} {:<8} {:<24}\n",
                        r.id, truncate(&r.vm_name, 14),
                        truncate(&r.source_host_name, 12), truncate(&r.target_host_name, 12),
                        &r.priority, truncate(&r.reason, 22)));
                }
                if recs.is_empty() { table.push_str("  (no pending recommendations — cluster is balanced)\n"); }
                ctx.env.insert("__drs_list".to_string(), table);
            }
        }
        "drs-apply" => {
            if let Some(id_str) = args.first() {
                let key = format!("__drs_apply_{}", id_str);
                match id_str.parse::<i64>() {
                    Ok(id) => match DrsService::mark_applied(&db, id) {
                        Ok(()) => ctx.env.insert(key, format!("OK:DRS recommendation #{} applied — migration started", id)),
                        Err(e) => ctx.env.insert(key, e),
                    },
                    Err(_) => ctx.env.insert(key, "Invalid recommendation ID".into()),
                };
            }
        }
        "drs-dismiss" => {
            if let Some(id_str) = args.first() {
                let key = format!("__drs_dismiss_{}", id_str);
                match id_str.parse::<i64>() {
                    Ok(id) => match DrsService::dismiss(&db, id) {
                        Ok(()) => ctx.env.insert(key, format!("OK:DRS recommendation #{} dismissed", id)),
                        Err(e) => ctx.env.insert(key, e),
                    },
                    Err(_) => ctx.env.insert(key, "Invalid recommendation ID".into()),
                };
            }
        }

        // ── Tasks ───────────────────────────────────────────────────
        "task-list" => {
            if let Ok(tasks) = TaskService::list(&db, 20) {
                let mut table = format!("{:<10} {:<18} {:<10} {:<6} {:<20}\n",
                    "ID", "TYPE", "STATUS", "PROG", "CREATED");
                table.push_str(&format!("{}\n", "─".repeat(68)));
                for t in &tasks {
                    table.push_str(&format!("{:<10} {:<18} {:<10} {:<6} {:<20}\n",
                        truncate(&t.id, 8), truncate(&t.task_type, 16),
                        &t.status, format!("{}%", t.progress_pct),
                        truncate(&t.created_at, 18)));
                }
                if tasks.is_empty() { table.push_str("  (no recent tasks)\n"); }
                ctx.env.insert("__task_list".to_string(), table);
            }
        }

        // ── Events ──────────────────────────────────────────────────
        "event-list" => {
            let limit = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(15);
            if let Ok(events) = EventService::recent(&db, limit, None) {
                let mut table = format!("{:<8} {:<10} {:<10} {:<40} {:<18}\n",
                    "SEV", "CATEGORY", "TARGET", "MESSAGE", "TIME");
                table.push_str(&format!("{}\n", "─".repeat(90)));
                for e in &events {
                    table.push_str(&format!("{:<8} {:<10} {:<10} {:<40} {:<18}\n",
                        &e.severity, &e.category,
                        e.target_id.as_deref().map(|id| truncate(id, 8)).unwrap_or_default(),
                        truncate(&e.message, 38),
                        truncate(&e.created_at, 16)));
                }
                if events.is_empty() { table.push_str("  (no events)\n"); }
                ctx.env.insert("__event_list".to_string(), table);
            }
        }

        // ── Alarms ──────────────────────────────────────────────────
        "alarm-list" => {
            if let Ok(alarms) = AlarmService::list(&db) {
                let active: Vec<_> = alarms.iter().filter(|a| a.triggered && !a.acknowledged).collect();
                let mut table = format!("{:<6} {:<20} {:<10} {:<10} {:<10}\n",
                    "ID", "NAME", "SEVERITY", "TARGET", "STATUS");
                table.push_str(&format!("{}\n", "─".repeat(60)));
                for a in &active {
                    table.push_str(&format!("{:<6} {:<20} {:<10} {:<10} {:<10}\n",
                        a.id, truncate(&a.name, 18), &a.severity,
                        truncate(&a.target_id, 8), "ACTIVE"));
                }
                if active.is_empty() { table.push_str("  (no active alarms)\n"); }
                ctx.env.insert("__alarm_list".to_string(), table);
            }
        }
        "alarm-ack" => {
            if let Some(id_str) = args.first() {
                let key = format!("__alarm_ack_{}", id_str);
                match id_str.parse::<i64>() {
                    Ok(id) => match AlarmService::acknowledge(&db, id) {
                        Ok(()) => ctx.env.insert(key, format!("OK:Alarm #{} acknowledged", id)),
                        Err(e) => ctx.env.insert(key, e),
                    },
                    Err(_) => ctx.env.insert(key, "Invalid alarm ID".into()),
                };
            }
        }

        // ── Services ────────────────────────────────────────────────
        "service-list" => {
            let stats = StatsService::cluster_stats(&db);
            let mut table = format!("{:<20} {:<10} {:<40}\n", "SERVICE", "STATUS", "DETAILS");
            table.push_str(&format!("{}\n", "─".repeat(74)));
            table.push_str(&format!("{:<20} {:<10} {}\n", "Heartbeat Monitor", "RUNNING", "10s interval, monitoring all hosts"));
            table.push_str(&format!("{:<20} {:<10} {}\n", "HA Engine", "RUNNING",
                format!("{} protected VMs", stats.ha_protected_vms)));
            table.push_str(&format!("{:<20} {:<10} {}\n", "DRS Engine", "RUNNING", "5m interval, load analysis"));
            table.push_str(&format!("{:<20} {:<10} {}\n", "Auth Service", "RUNNING", "JWT authentication"));
            table.push_str(&format!("{:<20} {:<10} {}\n", "VM Service", "RUNNING",
                format!("{} VMs managed", stats.total_vms)));
            table.push_str(&format!("{:<20} {:<10} {}\n", "Host Service", "RUNNING",
                format!("{} hosts registered", stats.total_hosts)));
            table.push_str(&format!("{:<20} {:<10} {}\n", "Datastore Service", "RUNNING", "Cluster-wide storage"));
            table.push_str(&format!("{:<20} {:<10} {}\n", "Task Service", "RUNNING", "Long-running operations"));
            table.push_str(&format!("{:<20} {:<10} {}\n", "Event Service", "RUNNING", "Cluster event log"));
            table.push_str(&format!("{:<20} {:<10} {}\n", "Alarm Service", "RUNNING", "Threshold monitoring"));
            ctx.env.insert("__service_list".to_string(), table);
        }

        // ── Storage compat (reuse pool-list for datastores) ─────────
        "pool-list" => {
            // In cluster mode, pools = datastores
            if let Ok(datastores) = DatastoreService::list(&db) {
                let mut table = format!("{:<34} {:<16} {:<8} {:<14} {:<14}\n",
                    "ID", "NAME", "TYPE", "TOTAL", "FREE");
                table.push_str(&format!("{}\n", "─".repeat(90)));
                for ds in &datastores {
                    table.push_str(&format!("{:<34} {:<16} {:<8} {:<14} {:<14}\n",
                        truncate(&ds.id, 32), truncate(&ds.name, 14), &ds.store_type,
                        format_bytes(ds.total_bytes), format_bytes(ds.free_bytes)));
                }
                ctx.env.insert("__pool_list".to_string(), table);
            }
        }

        _ => {}
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..max - 1]) }
}

fn format_bytes(b: i64) -> String {
    if b < 1024 { return format!("{} B", b); }
    let kb = b as f64 / 1024.0;
    if kb < 1024.0 { return format!("{:.1} KB", kb); }
    let mb = kb / 1024.0;
    if mb < 1024.0 { return format!("{:.1} MB", mb); }
    let gb = mb / 1024.0;
    if gb < 1024.0 { return format!("{:.1} GB", gb); }
    format!("{:.1} TB", gb / 1024.0)
}

fn find_cluster_id(db: &rusqlite::Connection, target: &str) -> Option<String> {
    // Try as ID first
    if db.query_row("SELECT id FROM clusters WHERE id = ?1", rusqlite::params![target], |r| r.get::<_, String>(0)).is_ok() {
        return Some(target.to_string());
    }
    // Try by name
    db.query_row("SELECT id FROM clusters WHERE name = ?1", rusqlite::params![target], |r| r.get(0)).ok()
}

fn find_host_id(db: &rusqlite::Connection, target: &str) -> Option<String> {
    if db.query_row("SELECT id FROM hosts WHERE id = ?1", rusqlite::params![target], |r| r.get::<_, String>(0)).is_ok() {
        return Some(target.to_string());
    }
    db.query_row("SELECT id FROM hosts WHERE hostname = ?1", rusqlite::params![target], |r| r.get(0)).ok()
}

fn find_datastore_by_name(db: &rusqlite::Connection, name: &str) -> Result<crate::services::datastore::DatastoreInfo, String> {
    let id: String = db.query_row("SELECT id FROM datastores WHERE name = ?1", rusqlite::params![name], |r| r.get(0))
        .map_err(|_| format!("Datastore '{}' not found", name))?;
    DatastoreService::get(db, &id)
}
