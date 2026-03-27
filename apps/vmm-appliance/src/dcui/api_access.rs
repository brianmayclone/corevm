use std::fs;
use std::path::Path;
use std::process::Command;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::widgets::SelectList;

pub enum DialogResult {
    Continue,
    Close,
}

pub struct Dialog {
    select: SelectList,
    message: Option<(String, bool)>,
    config_path: String,
}

impl Dialog {
    pub fn new() -> Self {
        let (config_path, current_enabled) = detect_api_access();

        let mut select = SelectList::new(
            "CLI/API Access:",
            vec![
                "Enabled — allow remote management via vmmctl".to_string(),
                "Disabled — only web UI access".to_string(),
            ],
        );
        select.selected = if current_enabled { 0 } else { 1 };
        select.focused = true;

        Self {
            select,
            message: None,
            config_path,
        }
    }

    fn save(&mut self) {
        let enabled = self.select.selected == 0;

        match update_cli_access_in_toml(&self.config_path, enabled) {
            Err(e) => {
                self.message = Some((format!("Failed to update config: {}", e), true));
                return;
            }
            Ok(_) => {}
        }

        // Restart service to apply
        let service = if Path::new("/etc/vmm/vmm-cluster.toml").exists() {
            "vmm-cluster"
        } else {
            "vmm-server"
        };

        match Command::new("systemctl").args(["restart", service]).status() {
            Ok(s) if s.success() => {
                let status = if enabled { "enabled" } else { "disabled" };
                self.message = Some((format!("CLI/API access {}. Service restarted.", status), false));
            }
            Ok(_) => {
                self.message = Some(("Config updated. Service restart failed.".to_string(), true));
            }
            Err(e) => {
                self.message = Some((format!("Config updated. Could not restart: {}", e), true));
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
            _ => self.select.handle_key(key),
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = area.width.min(60).max(40);
        let popup_height = 16u16.min(area.height);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let block = Block::default()
            .title(" CLI/API Access (F9) ")
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
                Constraint::Length(2),   // description
                Constraint::Length(1),   // gap
                Constraint::Length(6),   // select list
                Constraint::Min(1),     // spacer
                Constraint::Length(1),   // help text
            ])
            .split(inner);

        let desc = Paragraph::new(
            "Controls whether remote CLI tools (vmmctl)\ncan access the server via REST API."
        )
        .style(Style::default().fg(Color::White))
        .wrap(ratatui::widgets::Wrap { trim: true });
        desc.render(chunks[0], buf);

        self.select.render(chunks[2], buf);

        Paragraph::new("[Enter] Save  [Esc] Cancel")
            .style(Style::default().fg(Color::DarkGray))
            .render(chunks[4], buf);
    }
}

fn detect_api_access() -> (String, bool) {
    let server_path = "/etc/vmm/vmm-server.toml";
    let cluster_path = "/etc/vmm/vmm-cluster.toml";

    let path = if Path::new(server_path).exists() {
        server_path
    } else if Path::new(cluster_path).exists() {
        cluster_path
    } else {
        server_path
    };

    let enabled = read_cli_access_from_toml(path).unwrap_or(true);
    (path.to_string(), enabled)
}

fn read_cli_access_from_toml(path: &str) -> Option<bool> {
    let content = fs::read_to_string(path).ok()?;
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[api]" {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with('[') {
            break;
        }
        if in_section {
            if let Some(v) = trimmed.strip_prefix("cli_access_enabled") {
                let v = v.trim().trim_start_matches('=').trim();
                return Some(v == "true");
            }
        }
    }
    None
}

fn update_cli_access_in_toml(path: &str, enabled: bool) -> anyhow::Result<()> {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|_| "[server]\nport = 8443\n".to_string());

    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut in_api_section = false;
    let mut replaced = false;
    let mut api_section_exists = false;

    for line in lines.iter_mut() {
        let trimmed = line.trim().to_string();
        if trimmed == "[api]" {
            in_api_section = true;
            api_section_exists = true;
            continue;
        }
        if in_api_section && trimmed.starts_with('[') {
            in_api_section = false;
        }
        if in_api_section && trimmed.starts_with("cli_access_enabled") && trimmed.contains('=') {
            *line = format!("cli_access_enabled = {}", enabled);
            replaced = true;
        }
    }

    if !api_section_exists {
        lines.push(String::new());
        lines.push("[api]".to_string());
        lines.push(format!("cli_access_enabled = {}", enabled));
    } else if !replaced {
        // Section exists but key doesn't — insert after [api]
        let mut new_lines = Vec::new();
        for line in &lines {
            new_lines.push(line.clone());
            if line.trim() == "[api]" {
                new_lines.push(format!("cli_access_enabled = {}", enabled));
            }
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
