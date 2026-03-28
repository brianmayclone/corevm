//! Network management API — host interfaces, bridges, VLANs.

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::middleware::{AuthUser, AppError};

#[derive(Serialize)]
pub struct NetworkInterface {
    pub name: String,
    pub kind: String,          // "ethernet", "bridge", "loopback", "virtual", "wireless"
    pub mac: String,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub mtu: u32,
    pub state: String,         // "up", "down"
    pub speed_mbps: Option<u32>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Serialize)]
pub struct NetworkStats {
    pub total_interfaces: usize,
    pub active_interfaces: usize,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
}

/// GET /api/network/interfaces — list all host network interfaces.
pub async fn list_interfaces(_auth: AuthUser) -> Result<Json<Vec<NetworkInterface>>, AppError> {
    let ifaces = read_host_interfaces()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(ifaces))
}

/// GET /api/network/stats
pub async fn network_stats(_auth: AuthUser) -> Result<Json<NetworkStats>, AppError> {
    let ifaces = read_host_interfaces()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let active = ifaces.iter().filter(|i| i.state == "up").count();
    let total_rx = ifaces.iter().map(|i| i.rx_bytes).sum();
    let total_tx = ifaces.iter().map(|i| i.tx_bytes).sum();
    Ok(Json(NetworkStats {
        total_interfaces: ifaces.len(),
        active_interfaces: active,
        total_rx_bytes: total_rx,
        total_tx_bytes: total_tx,
    }))
}

/// Read host network interfaces for agent API (public, no auth wrapper).
pub fn list_interfaces_raw() -> Result<Vec<NetworkInterface>, String> {
    read_host_interfaces()
}

/// Read host network interfaces from /sys/class/net (Linux).
fn read_host_interfaces() -> Result<Vec<NetworkInterface>, String> {
    let mut interfaces = Vec::new();

    let net_dir = std::path::Path::new("/sys/class/net");
    if !net_dir.exists() {
        return Ok(interfaces);
    }

    let entries = std::fs::read_dir(net_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let base = entry.path();

        let read_file = |name: &str| -> String {
            std::fs::read_to_string(base.join(name)).unwrap_or_default().trim().to_string()
        };

        let mac = read_file("address");
        let mtu: u32 = read_file("mtu").parse().unwrap_or(1500);
        let operstate = read_file("operstate");
        let state = if operstate == "up" { "up" } else { "down" };
        let speed_str = read_file("speed");
        let speed_mbps: Option<u32> = speed_str.parse().ok().filter(|&s: &u32| s > 0 && s < 1_000_000);

        // RX/TX bytes
        let rx_bytes: u64 = read_file("statistics/rx_bytes").parse().unwrap_or(0);
        let tx_bytes: u64 = read_file("statistics/tx_bytes").parse().unwrap_or(0);

        // Determine interface kind
        let kind = if name == "lo" {
            "loopback"
        } else if base.join("bridge").exists() {
            "bridge"
        } else if base.join("wireless").exists() || base.join("phy80211").exists() {
            "wireless"
        } else if name.starts_with("veth") || name.starts_with("virbr") || name.starts_with("docker")
            || name.starts_with("br-") || name.starts_with("vnet") || name.starts_with("tap") {
            "virtual"
        } else {
            "ethernet"
        };

        // Get IPv4 address via ip command (fast, no external deps)
        let ipv4 = get_ipv4_address(&name);
        let ipv6 = get_ipv6_address(&name);

        interfaces.push(NetworkInterface {
            name, kind: kind.to_string(), mac, ipv4, ipv6, mtu, state: state.to_string(),
            speed_mbps, rx_bytes, tx_bytes,
        });
    }

    // Sort: ethernet first, then bridges, then virtual, loopback last
    interfaces.sort_by(|a, b| {
        let order = |k: &str| match k { "ethernet" => 0, "wireless" => 1, "bridge" => 2, "virtual" => 3, "loopback" => 4, _ => 5 };
        order(&a.kind).cmp(&order(&b.kind)).then(a.name.cmp(&b.name))
    });

    Ok(interfaces)
}

fn get_ipv4_address(iface: &str) -> Option<String> {
    let output = std::process::Command::new("ip")
        .args(["-4", "addr", "show", iface])
        .output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("inet ") {
            return trimmed.split_whitespace().nth(1).map(|s| s.to_string());
        }
    }
    None
}

// ── Bridge Management ────────────────────────────────────────────────────

/// POST /api/network/bridges — Create a Linux bridge (+ optional VXLAN).
pub async fn create_bridge(
    _auth: AuthUser,
    Json(req): Json<vmm_core::cluster::SetupBridgeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    setup_bridge(&req)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true, "bridge": req.bridge_name})))
}

/// DELETE /api/network/bridges/{name} — Remove a bridge (+ associated VXLAN).
pub async fn delete_bridge(
    _auth: AuthUser,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    teardown_bridge(&name)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// GET /api/network/bridges — List existing bridges.
pub async fn list_bridges(
    _auth: AuthUser,
) -> Result<Json<Vec<BridgeInfo>>, AppError> {
    let bridges = read_bridges()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(bridges))
}

#[derive(serde::Serialize)]
pub struct BridgeInfo {
    pub name: String,
    pub state: String,
    pub members: Vec<String>,
}

/// Create a Linux bridge, optionally with a VXLAN interface attached.
pub fn setup_bridge(req: &vmm_core::cluster::SetupBridgeRequest) -> Result<(), String> {
    // Check if bridge already exists
    let bridge_path = format!("/sys/class/net/{}", req.bridge_name);
    if std::path::Path::new(&bridge_path).exists() {
        eprintln!("[net] Bridge '{}' already exists, skipping creation", req.bridge_name);
        return Ok(());
    }

    // Create the bridge
    run_ip(&["link", "add", &req.bridge_name, "type", "bridge"])?;
    run_ip(&["link", "set", &req.bridge_name, "up"])?;
    eprintln!("[net] Created bridge '{}'", req.bridge_name);

    // Set up VXLAN overlay if configured
    if let Some(ref vxlan) = req.vxlan {
        let vxlan_name = format!("vx{}", req.network_id);
        let vni_str = vxlan.vni.to_string();
        let port_str = vxlan.port.to_string();

        let mut args = vec![
            "link", "add", &vxlan_name, "type", "vxlan",
            "id", &vni_str, "dstport", &port_str,
        ];

        if !vxlan.group.is_empty() {
            args.extend(&["group", &vxlan.group]);
        }
        if !vxlan.local_ip.is_empty() {
            args.extend(&["local", &vxlan.local_ip]);
        }
        // Use the physical device for multicast routing
        args.extend(&["dev", "lo"]); // will be overridden below if we can find the default route dev

        // Try to find the default route interface for VXLAN
        if let Some(dev) = get_default_route_dev() {
            let last = args.len() - 1;
            args[last] = &dev;
            run_ip(&args)?;
        } else {
            // Remove dev/lo and try without explicit dev
            args.truncate(args.len() - 2);
            run_ip(&args)?;
        }

        run_ip(&["link", "set", &vxlan_name, "master", &req.bridge_name])?;
        run_ip(&["link", "set", &vxlan_name, "up"])?;
        eprintln!("[net] Created VXLAN '{}' (VNI={}) on bridge '{}'", vxlan_name, vxlan.vni, req.bridge_name);
    }

    Ok(())
}

/// Tear down a bridge and any associated VXLAN interface.
pub fn teardown_bridge(bridge_name: &str) -> Result<(), String> {
    let bridge_path = format!("/sys/class/net/{}", bridge_name);
    if !std::path::Path::new(&bridge_path).exists() {
        return Ok(()); // Already gone
    }

    // Find and remove VXLAN interfaces attached to this bridge
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("vx") {
                // Check if this vxlan is a member of our bridge
                let master_path = format!("/sys/class/net/{}/master", name);
                if let Ok(link) = std::fs::read_link(&master_path) {
                    if link.file_name().map(|f| f.to_string_lossy().to_string()) == Some(bridge_name.to_string()) {
                        let _ = run_ip(&["link", "del", &name]);
                        eprintln!("[net] Removed VXLAN '{}'", name);
                    }
                }
            }
        }
    }

    run_ip(&["link", "set", bridge_name, "down"])?;
    run_ip(&["link", "del", bridge_name])?;
    eprintln!("[net] Removed bridge '{}'", bridge_name);
    Ok(())
}

/// Read all bridges from /sys/class/net.
fn read_bridges() -> Result<Vec<BridgeInfo>, String> {
    let mut bridges = Vec::new();
    let net_dir = std::path::Path::new("/sys/class/net");
    if !net_dir.exists() { return Ok(bridges); }

    for entry in std::fs::read_dir(net_dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !entry.path().join("bridge").exists() { continue; }

        let state = std::fs::read_to_string(entry.path().join("operstate"))
            .unwrap_or_default().trim().to_string();

        // List bridge members
        let mut members = Vec::new();
        let brif_path = entry.path().join("brif");
        if let Ok(ports) = std::fs::read_dir(&brif_path) {
            for port in ports.flatten() {
                members.push(port.file_name().to_string_lossy().to_string());
            }
        }

        bridges.push(BridgeInfo { name, state, members });
    }
    Ok(bridges)
}

fn run_ip(args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("ip")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run 'ip {}': {}", args.join(" "), e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("'ip {}' failed: {}", args.join(" "), stderr.trim()));
    }
    Ok(())
}

fn get_default_route_dev() -> Option<String> {
    let output = std::process::Command::new("ip")
        .args(["route", "show", "default"])
        .output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // "default via 192.168.1.1 dev eth0 ..."
    let parts: Vec<&str> = stdout.split_whitespace().collect();
    parts.iter().position(|&p| p == "dev").and_then(|i| parts.get(i + 1).map(|s| s.to_string()))
}

// ── viSwitch Management ──────────────────────────────────────────────────

/// Set up a viSwitch: bridge + optional bonding device + uplinks.
pub fn setup_viswitch(req: &vmm_core::cluster::SetupViSwitchRequest) -> Result<(), String> {
    let bridge = &req.bridge_name;

    // Check if bridge already exists
    if std::path::Path::new(&format!("/sys/class/net/{}", bridge)).exists() {
        eprintln!("[viswitch] Bridge '{}' already exists, tearing down first", bridge);
        let _ = teardown_viswitch(bridge);
    }

    // 1. Create bridge with configured MTU
    run_ip(&["link", "add", bridge, "type", "bridge"])?;
    if req.mtu > 0 {
        let mtu_str = req.mtu.to_string();
        run_ip(&["link", "set", bridge, "mtu", &mtu_str])?;
    }
    run_ip(&["link", "set", bridge, "up"])?;
    eprintln!("[viswitch] Created bridge '{}' (MTU={})", bridge, req.mtu);

    // Filter uplinks to only those with "vm" traffic type (bridge carries VM traffic)
    let vm_uplinks: Vec<_> = req.uplinks.iter()
        .filter(|u| u.traffic_types.split(',').any(|t| t.trim() == "vm"))
        .collect();

    if vm_uplinks.is_empty() {
        eprintln!("[viswitch] No VM-traffic uplinks configured for '{}'", bridge);
        return Ok(());
    }

    if vm_uplinks.len() > 1 {
        // Multiple uplinks: create bond device
        let bond_name = format!("bond-{}", bridge);
        setup_viswitch_bond(&bond_name, &req.uplink_policy, &vm_uplinks, bridge)?;
    } else if let Some(uplink) = vm_uplinks.first() {
        // Single uplink: join directly to bridge
        let iface = resolve_uplink_interface(uplink, bridge)?;
        run_ip(&["link", "set", &iface, "master", bridge])?;
        run_ip(&["link", "set", &iface, "up"])?;
        eprintln!("[viswitch] Uplink '{}' joined to bridge '{}'", iface, bridge);
    }

    Ok(())
}

/// Create a Linux bonding device for viSwitch uplink teaming.
fn setup_viswitch_bond(
    bond_name: &str,
    policy: &str,
    uplinks: &[&vmm_core::cluster::ViSwitchUplink],
    bridge: &str,
) -> Result<(), String> {
    let mode = match policy {
        "roundrobin" => "balance-rr",
        "failover" | "rulebased" => "active-backup",
        _ => "active-backup",
    };

    // Load bonding module if not loaded
    let _ = std::process::Command::new("modprobe").arg("bonding").output();

    run_ip(&["link", "add", bond_name, "type", "bond", "mode", mode])?;
    run_ip(&["link", "set", bond_name, "up"])?;

    for uplink in uplinks {
        let iface = resolve_uplink_interface(uplink, bridge)?;
        // Bond slaves must be down before enslaving
        let _ = run_ip(&["link", "set", &iface, "down"]);
        run_ip(&["link", "set", &iface, "master", bond_name])?;
        run_ip(&["link", "set", &iface, "up"])?;
        eprintln!("[viswitch] Bond '{}' slave: '{}'", bond_name, iface);
    }

    // Join bond to bridge
    run_ip(&["link", "set", bond_name, "master", bridge])?;
    eprintln!("[viswitch] Bond '{}' (mode={}) joined to bridge '{}'", bond_name, mode, bridge);

    if policy == "rulebased" {
        eprintln!("[viswitch] Warning: rule-based routing not yet implemented, using failover");
    }

    Ok(())
}

/// Resolve an uplink to a Linux interface name.
fn resolve_uplink_interface(
    uplink: &vmm_core::cluster::ViSwitchUplink,
    bridge: &str,
) -> Result<String, String> {
    match uplink.uplink_type.as_str() {
        "physical" => Ok(uplink.physical_nic.clone()),
        "virtual" => {
            if let Some(ref vxlan) = uplink.vxlan {
                let vxlan_name = format!("vxs{}-{}", bridge.trim_start_matches("vs"), uplink.uplink_index);
                let vni_str = vxlan.vni.to_string();
                let port_str = vxlan.port.to_string();

                let mut args = vec![
                    "link", "add", &vxlan_name, "type", "vxlan",
                    "id", &vni_str, "dstport", &port_str,
                ];
                if !vxlan.group.is_empty() {
                    args.extend(&["group", &vxlan.group]);
                }
                if !vxlan.local_ip.is_empty() {
                    args.extend(&["local", &vxlan.local_ip]);
                }
                if let Some(dev) = get_default_route_dev() {
                    args.extend(&["dev", &dev]);
                    run_ip(&args)?;
                } else {
                    run_ip(&args)?;
                }
                run_ip(&["link", "set", &vxlan_name, "up"])?;
                eprintln!("[viswitch] Created VXLAN '{}' (VNI={})", vxlan_name, vxlan.vni);
                Ok(vxlan_name)
            } else {
                Err("Virtual uplink requires VXLAN config".into())
            }
        }
        _ => Err(format!("Unknown uplink type: {}", uplink.uplink_type)),
    }
}

/// Tear down a viSwitch: remove bond, VXLAN interfaces, and bridge.
pub fn teardown_viswitch(bridge_name: &str) -> Result<(), String> {
    let bridge_path = format!("/sys/class/net/{}", bridge_name);
    if !std::path::Path::new(&bridge_path).exists() {
        return Ok(());
    }

    // Remove bond device if exists
    let bond_name = format!("bond-{}", bridge_name);
    if std::path::Path::new(&format!("/sys/class/net/{}", bond_name)).exists() {
        let _ = run_ip(&["link", "set", &bond_name, "down"]);
        let _ = run_ip(&["link", "del", &bond_name]);
        eprintln!("[viswitch] Removed bond '{}'", bond_name);
    }

    // Remove VXLAN interfaces belonging to this viSwitch (vxs{id}-*)
    let vs_id = bridge_name.trim_start_matches("vs");
    let vxlan_prefix = format!("vxs{}-", vs_id);
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&vxlan_prefix) {
                let _ = run_ip(&["link", "del", &name]);
                eprintln!("[viswitch] Removed VXLAN '{}'", name);
            }
        }
    }

    // Remove bridge
    run_ip(&["link", "set", bridge_name, "down"])?;
    run_ip(&["link", "del", bridge_name])?;
    eprintln!("[viswitch] Removed bridge '{}'", bridge_name);
    Ok(())
}

fn get_ipv6_address(iface: &str) -> Option<String> {
    let output = std::process::Command::new("ip")
        .args(["-6", "addr", "show", iface, "scope", "global"])
        .output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("inet6 ") {
            return trimmed.split_whitespace().nth(1).map(|s| s.to_string());
        }
    }
    None
}
