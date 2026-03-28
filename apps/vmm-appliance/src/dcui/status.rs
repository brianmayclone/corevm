use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};
use sysinfo::System;

use crate::common::config::{ApplianceConfig, ApplianceRole};
use crate::common::network::read_current_ip;

/// CoreSAN status info — only populated when vmm-san has claimed disks.
struct SanStatus {
    quorum: String,
    peers: u32,
    volumes: u32,
    claimed_disks: u32,
    free_bytes: u64,
    total_bytes: u64,
    is_leader: bool,
}

pub struct StatusBar {
    sys: System,
    last_refresh: Instant,
    hostname: String,
    ip: String,
    role: String,
    service_name: String,
    service_status: String,
    port: u16,
    version: String,
    uptime_secs: u64,
    cpu_pct: f32,
    ram_used: u64,
    ram_total: u64,
    interface: String,
    san_status: Option<SanStatus>,
}

impl StatusBar {
    pub fn new() -> Self {
        // Load appliance config with fallbacks
        let cfg = ApplianceConfig::load(Path::new("/etc/vmm/appliance.toml"))
            .unwrap_or(ApplianceConfig {
                role: ApplianceRole::Server,
                language: "en_US".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            });

        let (role_str, service_name, port) = match cfg.role {
            ApplianceRole::Server => {
                let port = read_port_from_toml("/etc/vmm/vmm-server.toml", "server").unwrap_or(8080);
                ("Server".to_string(), "vmm-server".to_string(), port)
            }
            ApplianceRole::Cluster => {
                let port = read_port_from_toml("/etc/vmm/vmm-cluster.toml", "cluster").unwrap_or(8081);
                ("Cluster".to_string(), "vmm-cluster".to_string(), port)
            }
        };

        let mut sys = System::new_all();
        sys.refresh_all();

        let mut bar = StatusBar {
            sys,
            last_refresh: Instant::now() - Duration::from_secs(10), // force immediate refresh
            hostname: String::new(),
            ip: String::new(),
            role: role_str,
            service_name,
            service_status: String::new(),
            port,
            version: cfg.version,
            uptime_secs: 0,
            cpu_pct: 0.0,
            ram_used: 0,
            ram_total: 0,
            interface: detect_primary_interface(),
            san_status: None,
        };
        bar.refresh();
        bar
    }

    pub fn refresh(&mut self) {
        if self.last_refresh.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_refresh = Instant::now();

        // Hostname
        self.hostname = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_default()
            .trim()
            .to_string();
        if self.hostname.is_empty() {
            self.hostname = hostname_from_cmd();
        }

        // IP
        self.ip = read_current_ip(&self.interface)
            .ok()
            .flatten()
            .unwrap_or_else(|| "?.?.?.?".to_string());

        // Uptime
        self.uptime_secs = System::uptime();

        // Service status
        self.service_status = get_service_status(&self.service_name);

        // CPU / RAM
        self.sys.refresh_all();
        self.cpu_pct = self.sys.global_cpu_usage();
        self.ram_used = self.sys.used_memory();
        self.ram_total = self.sys.total_memory();

        // CoreSAN status — only if vmm-san service is running
        self.san_status = fetch_san_status();
    }

    /// Returns the required height for the status bar (including borders).
    pub fn height(&self) -> u16 {
        7 // 5 lines + 2 border — always fixed, SAN goes in the right panel
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Split horizontally: left = system info, right = CoreSAN (if active)
        if self.san_status.is_some() {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(55),
                    Constraint::Percentage(45),
                ])
                .split(area);

            self.render_system(cols[0], buf);
            self.render_san(cols[1], buf);
        } else {
            self.render_system(area, buf);
        }
    }

    /// Render the left panel: system information.
    fn render_system(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        // Line 1: Title + Role
        let line1 = Line::from(vec![
            Span::styled(
                format!(" CoreVM v{}", self.version),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  [{}]", self.role),
                Style::default().fg(Color::Yellow),
            ),
        ]);

        // Line 2: Hostname + Uptime
        let (days, hours, mins) = uptime_parts(self.uptime_secs);
        let uptime_str = format!("{}d {}h {}m", days, hours, mins);
        let line2 = Line::from(vec![
            Span::styled(" Host: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<18}", self.hostname),
                Style::default().fg(Color::White),
            ),
            Span::styled("Up: ", Style::default().fg(Color::DarkGray)),
            Span::styled(uptime_str, Style::default().fg(Color::Green)),
        ]);

        // Line 3: IP + CPU + RAM
        let ram_used_mb = self.ram_used / 1024 / 1024;
        let ram_total_mb = self.ram_total / 1024 / 1024;
        let line3 = Line::from(vec![
            Span::styled(" IP: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<16}", self.ip),
                Style::default().fg(Color::White),
            ),
            Span::styled("CPU: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:>5.1}%  ", self.cpu_pct),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled("RAM: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/{} MB", ram_used_mb, ram_total_mb),
                Style::default().fg(Color::Yellow),
            ),
        ]);

        // Line 4: Service + status indicator + port
        let (indicator, status_color) = if self.service_status == "active" {
            ("● running", Color::Green)
        } else {
            ("○ stopped", Color::Red)
        };
        let line4 = Line::from(vec![
            Span::styled(" Svc: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<14}", self.service_name),
                Style::default().fg(Color::White),
            ),
            Span::styled(indicator, Style::default().fg(status_color)),
            Span::styled("  :", Style::default().fg(Color::DarkGray)),
            Span::styled(
                self.port.to_string(),
                Style::default().fg(Color::Cyan),
            ),
        ]);

        // Line 5: URL
        let line5 = Line::from(vec![
            Span::styled(" URL: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("https://{}:{}", self.ip, self.port),
                Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED),
            ),
        ]);

        let para = Paragraph::new(vec![line1, line2, line3, line4, line5]);
        para.render(inner, buf);
    }

    /// Render the right panel: CoreSAN status.
    fn render_san(&self, area: Rect, buf: &mut Buffer) {
        let san = match self.san_status {
            Some(ref s) => s,
            None => return,
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .style(Style::default().fg(Color::Magenta));

        let inner = block.inner(area);
        block.render(area, buf);

        let (status_icon, status_color) = match san.quorum.as_str() {
            "active" => ("●", Color::Green),
            "degraded" => ("●", Color::Yellow),
            "sanitizing" => ("◌", Color::Yellow),
            "solo" => ("●", Color::Cyan),
            "fenced" => ("●", Color::Red),
            _ => ("○", Color::DarkGray),
        };

        let leader_str = if san.is_leader { "  ★ Leader" } else { "" };

        // Line 1: CoreSAN title + quorum status
        let san1 = Line::from(vec![
            Span::styled(
                " CoreSAN ",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} {}", status_icon, san.quorum),
                Style::default().fg(status_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(leader_str, Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        ]);

        // Line 2: Peers + Volumes
        let san2 = Line::from(vec![
            Span::styled(" Peers: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<6}", san.peers),
                Style::default().fg(Color::White),
            ),
            Span::styled("Volumes: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", san.volumes),
                Style::default().fg(Color::White),
            ),
        ]);

        // Line 3: Claimed disks
        let san3 = Line::from(vec![
            Span::styled(" Disks: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} claimed", san.claimed_disks),
                Style::default().fg(Color::White),
            ),
        ]);

        // Line 4: Storage capacity
        let free_gb = san.free_bytes / 1024 / 1024 / 1024;
        let total_gb = san.total_bytes / 1024 / 1024 / 1024;
        let used_gb = total_gb.saturating_sub(free_gb);
        let pct_used = if total_gb > 0 { (used_gb * 100) / total_gb } else { 0 };
        let capacity_color = if pct_used > 90 { Color::Red } else if pct_used > 75 { Color::Yellow } else { Color::Green };

        let san4 = Line::from(vec![
            Span::styled(" Storage: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/{} GB", free_gb, total_gb),
                Style::default().fg(capacity_color),
            ),
            Span::styled(
                format!(" ({}% used)", pct_used),
                Style::default().fg(capacity_color),
            ),
        ]);

        // Line 5: empty / padding
        let san5 = Line::from(vec![]);

        let para = Paragraph::new(vec![san1, san2, san3, san4, san5]);
        para.render(inner, buf);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn uptime_parts(secs: u64) -> (u64, u64, u64) {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    (days, hours, mins)
}

fn get_service_status(service: &str) -> String {
    Command::new("systemctl")
        .args(["is-active", service])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn hostname_from_cmd() -> String {
    Command::new("hostname")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn detect_primary_interface() -> String {
    // Read /sys/class/net looking for first non-lo interface
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name != "lo" {
                return name;
            }
        }
    }
    "eth0".to_string()
}

/// Fetch CoreSAN status from the local vmm-san daemon.
/// Returns None if vmm-san is not running or has no claimed disks.
fn fetch_san_status() -> Option<SanStatus> {
    // Check if vmm-san service is active first (cheap check)
    let service_active = Command::new("systemctl")
        .args(["is-active", "vmm-san"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false);

    if !service_active {
        return None;
    }

    // Read vmm-san port from config
    let san_port = read_port_from_toml("/etc/vmm/vmm-san.toml", "server").unwrap_or(7443);

    // Try /api/status first (simpler, flat response)
    let url = format!("http://127.0.0.1:{}/api/status", san_port);
    let output = Command::new("curl")
        .args(["-s", "-m", "2", "--connect-timeout", "1", &url])
        .output()
        .ok()?;

    let body = String::from_utf8_lossy(&output.stdout);
    if body.is_empty() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_str(&body).ok()?;

    // /api/status returns a flat structure directly (not nested under "status")
    // But /api/dashboard wraps it in { status: {...}, total_capacity_bytes, ... }
    // Handle both:
    let st = if json.get("status").is_some() && json["status"].is_object() {
        &json["status"]
    } else {
        &json
    };

    // Check if there are claimed disks — only show SAN status if disks are claimed
    let claimed_disks = st["claimed_disks"].as_u64().unwrap_or(0) as u32;
    if claimed_disks == 0 {
        return None;
    }

    let quorum = st["quorum_status"].as_str().unwrap_or("unknown").to_string();
    let peers = st["peer_count"].as_u64().unwrap_or(0) as u32;
    let volumes = st["volumes"].as_array().map(|a| a.len() as u32).unwrap_or(0);
    let is_leader = st["is_leader"].as_bool().unwrap_or(false);

    // Storage capacity — try various field names
    let total_bytes = json["total_capacity_bytes"].as_u64()
        .or_else(|| st["volumes"].as_array().map(|vols|
            vols.iter().filter_map(|v| v["total_bytes"].as_u64()).sum()))
        .unwrap_or(0);
    let used_bytes = json["used_capacity_bytes"].as_u64().unwrap_or(0);
    let free_bytes = if total_bytes > 0 {
        total_bytes.saturating_sub(used_bytes)
    } else {
        st["volumes"].as_array().map(|vols|
            vols.iter().filter_map(|v| v["free_bytes"].as_u64()).sum()
        ).unwrap_or(0)
    };

    Some(SanStatus {
        quorum,
        peers,
        volumes,
        claimed_disks,
        free_bytes,
        total_bytes,
        is_leader,
    })
}

fn read_port_from_toml(path: &str, section: &str) -> Option<u16> {
    let content = std::fs::read_to_string(path).ok()?;
    // Simple line-by-line scan inside the right section
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == format!("[{}]", section) {
            in_section = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_section = false;
        }
        if in_section {
            if let Some(rest) = trimmed.strip_prefix("port") {
                let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '=');
                if let Ok(p) = rest.trim().parse::<u16>() {
                    return Some(p);
                }
            }
        }
    }
    None
}
