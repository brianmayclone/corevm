use std::process::Command;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use sysinfo::System;

pub enum DialogResult {
    Continue,
    Close,
}

pub struct Dialog {
    lines: Vec<String>,
    scroll: usize,
}

impl Dialog {
    pub fn new() -> Self {
        let lines = collect_diagnostics();
        Self { lines, scroll: 0 }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        match key.code {
            KeyCode::Esc => return DialogResult::Close,
            KeyCode::Up => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                }
            }
            KeyCode::Down => {
                if self.scroll + 1 < self.lines.len() {
                    self.scroll += 1;
                }
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                let max = self.lines.len().saturating_sub(1);
                self.scroll = (self.scroll + 10).min(max);
            }
            _ => {}
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = area.width.saturating_sub(4).max(40);
        let popup_height = area.height.saturating_sub(4).max(10);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let block = Block::default()
            .title(" Diagnostics (F11) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));

        let inner = block.inner(popup);
        block.render(popup, buf);

        if inner.height == 0 {
            return;
        }

        let content_height = inner.height.saturating_sub(1) as usize;
        let start = if self.lines.len() > content_height {
            let max_scroll = self.lines.len().saturating_sub(content_height);
            self.scroll.min(max_scroll)
        } else {
            0
        };

        let visible: Vec<Line> = self.lines.iter()
            .skip(start)
            .take(content_height)
            .map(|l| Line::from(Span::raw(l.as_str())))
            .collect();

        let content_area = Rect { height: inner.height.saturating_sub(1), ..inner };
        Paragraph::new(visible)
            .style(Style::default().fg(Color::White))
            .render(content_area, buf);

        let help_area = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
        Paragraph::new("[↑↓ PgUp/PgDn] Scroll  [Esc] Close")
            .style(Style::default().fg(Color::DarkGray))
            .render(help_area, buf);
    }
}

fn cmd_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("error: {}", e))
}

fn collect_diagnostics() -> Vec<String> {
    let mut lines = Vec::new();

    // System info header
    lines.push("=== System Information ===".to_string());

    let hostname = std::fs::read_to_string("/etc/hostname")
        .unwrap_or_else(|_| "unknown".to_string());
    lines.push(format!("Hostname:    {}", hostname.trim()));

    let kernel = cmd_output("uname", &["-r"]);
    lines.push(format!("Kernel:      {}", kernel));

    // CPU / RAM via sysinfo
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_count = sys.cpus().len();
    let cpu_name = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
    lines.push(format!("CPU:         {} x {}", cpu_count, cpu_name));

    let total_mb = sys.total_memory() / 1024 / 1024;
    let used_mb = sys.used_memory() / 1024 / 1024;
    lines.push(format!("RAM:         {} MB used / {} MB total", used_mb, total_mb));

    lines.push(String::new());
    lines.push("=== Disk Usage ===".to_string());

    let df_root = cmd_output("df", &["-h", "/"]);
    for line in df_root.lines() {
        lines.push(line.to_string());
    }

    let vmm_path = "/var/lib/vmm";
    if std::path::Path::new(vmm_path).exists() {
        let df_vmm = cmd_output("df", &["-h", vmm_path]);
        for line in df_vmm.lines().skip(1) {
            lines.push(line.to_string());
        }
    }

    lines.push(String::new());
    lines.push("=== Network Interfaces ===".to_string());

    let ip_out = cmd_output("ip", &["-br", "addr"]);
    for line in ip_out.lines() {
        lines.push(line.to_string());
    }

    lines.push(String::new());
    lines.push("=== Service Status ===".to_string());

    for svc in &["vmm-server", "vmm-cluster", "nftables", "chronyd", "systemd-networkd"] {
        let status = cmd_output("systemctl", &["is-active", svc]);
        lines.push(format!("{:20} {}", svc, status));
    }

    lines.push(String::new());
    lines.push("=== Connectivity Tests ===".to_string());

    // Ping gateway
    let gw = get_default_gateway();
    if let Some(ref gw_ip) = gw {
        let ping = cmd_output("ping", &["-c", "1", "-W", "2", gw_ip]);
        let ok = if ping.contains("1 received") { "OK" } else { "FAIL" };
        lines.push(format!("Ping gateway ({}): {}", gw_ip, ok));
    } else {
        lines.push("Ping gateway: no gateway found".to_string());
    }

    // DNS resolution
    let dns_out = cmd_output("host", &["google.com"]);
    let dns_ok = if dns_out.contains("has address") || dns_out.contains("has IPv6") {
        "OK"
    } else {
        "FAIL"
    };
    lines.push(format!("DNS resolution (google.com): {}", dns_ok));
    if dns_ok == "FAIL" {
        lines.push(format!("  ({})", dns_out.lines().next().unwrap_or("")));
    }

    lines
}

fn get_default_gateway() -> Option<String> {
    let out = Command::new("ip")
        .args(["route", "show", "default"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    // "default via 192.168.1.1 dev eth0"
    for part in s.split_whitespace() {
        if part.contains('.') && part != "default" {
            return Some(part.to_string());
        }
    }
    None
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
