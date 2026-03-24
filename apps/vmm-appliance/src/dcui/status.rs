use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};
use sysinfo::System;

use crate::common::config::{ApplianceConfig, ApplianceRole};
use crate::common::network::read_current_ip;

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
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        // Line 1: Title + Role
        let line1 = Line::from(vec![
            Span::styled(
                format!(" CoreVM Appliance v{}", self.version),
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
            Span::styled(" Hostname: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<20}", self.hostname),
                Style::default().fg(Color::White),
            ),
            Span::styled("Uptime: ", Style::default().fg(Color::DarkGray)),
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
            Span::styled(" Service: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<15}", self.service_name),
                Style::default().fg(Color::White),
            ),
            Span::styled(indicator, Style::default().fg(status_color)),
            Span::styled("  Port: ", Style::default().fg(Color::DarkGray)),
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

        let lines = vec![line1, line2, line3, line4, line5];
        let para = Paragraph::new(lines);
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
