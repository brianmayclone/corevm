use std::fs;
use std::path::Path;
use std::process::Command;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::widgets::TextInput;

pub enum DialogResult {
    Continue,
    Close,
}

pub struct Dialog {
    port_input: TextInput,
    message: Option<(String, bool)>,
    config_path: String,
    service_name: String,
    is_cluster: bool,
}

impl Dialog {
    pub fn new() -> Self {
        let (config_path, service_name, is_cluster, current_port) = detect_config();

        let mut port_input = TextInput::new("Service port (1-65535):");
        port_input.value = current_port.to_string();
        port_input.cursor = port_input.value.chars().count();
        port_input.focused = true;

        Self {
            port_input,
            message: None,
            config_path,
            service_name,
            is_cluster,
        }
    }

    fn save(&mut self) {
        let port_str = self.port_input.value.trim().to_string();
        let port: u16 = match port_str.parse() {
            Ok(p) if p >= 1 => p,
            _ => {
                self.message = Some(("Invalid port: must be 1-65535.".to_string(), true));
                return;
            }
        };

        // Read, modify, and write back the TOML file
        match update_port_in_toml(&self.config_path, port, self.is_cluster) {
            Err(e) => {
                self.message = Some((format!("Failed to update config: {}", e), true));
                return;
            }
            Ok(_) => {}
        }

        // Update nftables
        let nft_conf = format!(
            "#!/usr/sbin/nft -f\nflush ruleset\ntable inet filter {{\n    chain input {{\n        type filter hook input priority 0; policy drop;\n        iif lo accept\n        ct state established,related accept\n        ip protocol icmp accept\n        tcp dport 22 accept\n        tcp dport {} accept\n    }}\n    chain forward {{ type filter hook forward priority 0; policy drop; }}\n    chain output {{ type filter hook output priority 0; policy accept; }}\n}}\n",
            port
        );
        let _ = fs::write("/etc/nftables.conf", &nft_conf);
        let _ = Command::new("systemctl").args(["restart", "nftables"]).status();

        // Restart service
        match Command::new("systemctl")
            .args(["restart", &self.service_name])
            .status()
        {
            Ok(s) if s.success() => {
                self.message = Some((format!("Port updated to {}. Service restarted.", port), false));
            }
            Ok(_) => {
                self.message = Some((format!("Port updated to {}. Service restart failed.", port), true));
            }
            Err(e) => {
                self.message = Some((format!("Port updated. Could not restart service: {}", e), true));
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        if self.message.is_some() {
            self.message = None;
            return DialogResult::Close;
        }

        match key.code {
            KeyCode::Esc => return DialogResult::Close,
            KeyCode::Enter => self.save(),
            _ => self.port_input.handle_key(key),
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = area.width.min(60).max(40);
        let popup_height = 14u16.min(area.height);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let title = format!(" Service Port (F3) - {} ", self.service_name);
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

        let inner = block.inner(popup);
        block.render(popup, buf);

        if let Some((msg, is_err)) = &self.message {
            let color = if *is_err { Color::Red } else { Color::Green };
            Paragraph::new(msg.as_str())
                .style(Style::default().fg(color))
                .wrap(ratatui::widgets::Wrap { trim: true })
                .render(inner, buf);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);

        self.port_input.render(chunks[0], buf);
        Paragraph::new("[Enter] Save  [Esc] Cancel")
            .style(Style::default().fg(Color::DarkGray))
            .render(chunks[2], buf);
    }
}

fn detect_config() -> (String, String, bool, u16) {
    let server_path = "/etc/vmm/vmm-server.toml";
    let cluster_path = "/etc/vmm/vmm-cluster.toml";

    if Path::new(server_path).exists() {
        let port = read_port_from_toml(server_path, false).unwrap_or(8080);
        (server_path.to_string(), "vmm-server".to_string(), false, port)
    } else if Path::new(cluster_path).exists() {
        let port = read_port_from_toml(cluster_path, true).unwrap_or(8081);
        (cluster_path.to_string(), "vmm-cluster".to_string(), true, port)
    } else {
        (server_path.to_string(), "vmm-server".to_string(), false, 8080)
    }
}

fn read_port_from_toml(path: &str, is_cluster: bool) -> Option<u16> {
    let content = fs::read_to_string(path).ok()?;
    let section = if is_cluster { "[cluster]" } else { "[server]" };
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == section {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with('[') {
            break;
        }
        if in_section {
            if let Some(v) = trimmed.strip_prefix("port") {
                let v = v.trim().trim_start_matches('=').trim();
                return v.parse().ok();
            }
        }
    }
    None
}

fn update_port_in_toml(path: &str, port: u16, is_cluster: bool) -> anyhow::Result<()> {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|_| if is_cluster {
            format!("[cluster]\nport = 8081\n")
        } else {
            format!("[server]\nport = 8080\n")
        });

    let section = if is_cluster { "[cluster]" } else { "[server]" };
    let mut in_section = false;
    let mut replaced = false;
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    for line in lines.iter_mut() {
        let trimmed = line.trim().to_string();
        if trimmed == section {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with('[') {
            in_section = false;
        }
        if in_section && trimmed.starts_with("port") && trimmed.contains('=') {
            *line = format!("port = {}", port);
            replaced = true;
        }
    }

    if !replaced {
        // Insert port under section header
        let mut new_lines = Vec::new();
        let mut inserted = false;
        for line in &lines {
            new_lines.push(line.clone());
            if line.trim() == section && !inserted {
                new_lines.push(format!("port = {}", port));
                inserted = true;
            }
        }
        if !inserted {
            new_lines.push(section.to_string());
            new_lines.push(format!("port = {}", port));
        }
        lines = new_lines;
    }

    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(path, lines.join("\n") + "\n")
        .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", path, e))
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
