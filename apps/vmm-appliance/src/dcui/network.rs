use std::fs;
use std::path::Path;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::network::{detect_interfaces, write_networkd_config, apply_networkd_config, NetworkConfig};
use crate::common::widgets::{SelectList, TextInput};

pub enum DialogResult {
    Continue,
    Close,
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Interface,
    Mode,
    Address,
    Gateway,
    Dns,
    Hostname,
}

impl Field {
    fn next(self, dhcp: bool) -> Self {
        match self {
            Field::Interface => Field::Mode,
            Field::Mode => {
                if dhcp { Field::Hostname } else { Field::Address }
            }
            Field::Address => Field::Gateway,
            Field::Gateway => Field::Dns,
            Field::Dns => Field::Hostname,
            Field::Hostname => Field::Interface,
        }
    }
    fn prev(self, dhcp: bool) -> Self {
        match self {
            Field::Interface => Field::Hostname,
            Field::Mode => Field::Interface,
            Field::Address => Field::Mode,
            Field::Gateway => Field::Address,
            Field::Dns => Field::Gateway,
            Field::Hostname => {
                if dhcp { Field::Mode } else { Field::Dns }
            }
        }
    }
}

pub struct Dialog {
    iface_list: SelectList,
    mode_list: SelectList,
    address: TextInput,
    gateway: TextInput,
    dns: TextInput,
    hostname: TextInput,
    focus: Field,
    message: Option<String>,
}

impl Dialog {
    pub fn new() -> Self {
        let interfaces = detect_interfaces().unwrap_or_default();
        let iface_names: Vec<String> = if interfaces.is_empty() {
            vec!["eth0".to_string()]
        } else {
            interfaces.iter().map(|i| i.name.clone()).collect()
        };

        let mut iface_list = SelectList::new("Interface:", iface_names);
        iface_list.focused = true;

        let mode_list = SelectList::new(
            "Mode:",
            vec!["DHCP".to_string(), "Static".to_string()],
        );

        let mut address = TextInput::new("Address (CIDR):");
        let mut gateway = TextInput::new("Gateway:");
        let mut dns = TextInput::new("DNS (comma-separated):");
        let mut hostname = TextInput::new("Hostname:");

        // Try to load current config
        let net_path = Path::new("/etc/systemd/network/10-management.network");
        if let Ok(content) = fs::read_to_string(net_path) {
            for line in content.lines() {
                if let Some(v) = line.strip_prefix("Address=") {
                    address.value = v.trim().to_string();
                    address.cursor = address.value.chars().count();
                } else if let Some(v) = line.strip_prefix("Gateway=") {
                    gateway.value = v.trim().to_string();
                    gateway.cursor = gateway.value.chars().count();
                } else if let Some(v) = line.strip_prefix("DNS=") {
                    if dns.value.is_empty() {
                        dns.value = v.trim().to_string();
                    } else {
                        dns.value.push(',');
                        dns.value.push_str(v.trim());
                    }
                    dns.cursor = dns.value.chars().count();
                }
            }
        }

        if let Ok(h) = fs::read_to_string("/etc/hostname") {
            hostname.value = h.trim().to_string();
            hostname.cursor = hostname.value.chars().count();
        }

        Self {
            iface_list,
            mode_list,
            address,
            gateway,
            dns,
            hostname,
            focus: Field::Interface,
            message: None,
        }
    }

    fn is_dhcp(&self) -> bool {
        self.mode_list.selected == 0
    }

    fn set_focus(&mut self, f: Field) {
        self.iface_list.focused = f == Field::Interface;
        self.mode_list.focused = f == Field::Mode;
        self.address.focused = f == Field::Address;
        self.gateway.focused = f == Field::Gateway;
        self.dns.focused = f == Field::Dns;
        self.hostname.focused = f == Field::Hostname;
        self.focus = f;
    }

    fn save(&mut self) {
        let dhcp = self.is_dhcp();
        let iface = self.iface_list.selected_item().unwrap_or("eth0").to_string();
        let hostname = self.hostname.value.trim().to_string();
        let hostname_clone = hostname.clone();

        let dns_list: Vec<String> = self.dns.value
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let config = NetworkConfig {
            interface: iface,
            dhcp,
            address: if dhcp { None } else { Some(self.address.value.trim().to_string()) },
            gateway: if dhcp { None } else { Some(self.gateway.value.trim().to_string()) },
            dns: if dhcp { Vec::new() } else { dns_list },
            hostname: hostname.clone(),
        };

        match write_networkd_config(Path::new("/"), &config) {
            Err(e) => {
                self.message = Some(format!("Error writing config: {}", e));
                return;
            }
            Ok(_) => {}
        }

        if let Err(e) = apply_networkd_config() {
            self.message = Some(format!("Config written, networkctl error: {}", e));
            return;
        }

        let _ = fs::write("/etc/hostname", format!("{}\n", hostname_clone));

        self.message = Some("Network config saved and applied.".to_string());
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        // If there's a result message, any key dismisses it
        if self.message.is_some() {
            self.message = None;
            return DialogResult::Close;
        }

        match key.code {
            KeyCode::Esc => return DialogResult::Close,
            KeyCode::Tab => {
                let dhcp = self.is_dhcp();
                let next = self.focus.next(dhcp);
                self.set_focus(next);
            }
            KeyCode::BackTab => {
                let dhcp = self.is_dhcp();
                let prev = self.focus.prev(dhcp);
                self.set_focus(prev);
            }
            KeyCode::Enter => {
                if self.focus == Field::Hostname {
                    self.save();
                } else {
                    let dhcp = self.is_dhcp();
                    let next = self.focus.next(dhcp);
                    self.set_focus(next);
                }
            }
            _ => match self.focus {
                Field::Interface => self.iface_list.handle_key(key),
                Field::Mode => self.mode_list.handle_key(key),
                Field::Address => self.address.handle_key(key),
                Field::Gateway => self.gateway.handle_key(key),
                Field::Dns => self.dns.handle_key(key),
                Field::Hostname => self.hostname.handle_key(key),
            },
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = area.width.min(70).max(50);
        let popup_height = if self.is_dhcp() { 18u16 } else { 28u16 }.min(area.height);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let block = Block::default()
            .title(" Network Configuration (F1) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

        let inner = block.inner(popup);
        block.render(popup, buf);

        if let Some(msg) = &self.message {
            Paragraph::new(msg.as_str())
                .style(Style::default().fg(Color::Green))
                .wrap(ratatui::widgets::Wrap { trim: true })
                .render(inner, buf);
            return;
        }

        let dhcp = self.is_dhcp();
        let constraints = if dhcp {
            vec![
                Constraint::Length(4), // iface
                Constraint::Length(4), // mode
                Constraint::Length(4), // hostname
                Constraint::Min(1),    // spacer
                Constraint::Length(1), // help
            ]
        } else {
            vec![
                Constraint::Length(4), // iface
                Constraint::Length(4), // mode
                Constraint::Length(4), // address
                Constraint::Length(4), // gateway
                Constraint::Length(4), // dns
                Constraint::Length(4), // hostname
                Constraint::Min(1),    // spacer
                Constraint::Length(1), // help
            ]
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        if dhcp {
            self.iface_list.render(chunks[0], buf);
            self.mode_list.render(chunks[1], buf);
            self.hostname.render(chunks[2], buf);
            Paragraph::new("[Tab] Next  [Enter] Save  [Esc] Cancel")
                .style(Style::default().fg(Color::DarkGray))
                .render(chunks[4], buf);
        } else {
            self.iface_list.render(chunks[0], buf);
            self.mode_list.render(chunks[1], buf);
            self.address.render(chunks[2], buf);
            self.gateway.render(chunks[3], buf);
            self.dns.render(chunks[4], buf);
            self.hostname.render(chunks[5], buf);
            Paragraph::new("[Tab] Next  [Enter on Hostname] Save  [Esc] Cancel")
                .style(Style::default().fg(Color::DarkGray))
                .render(chunks[7], buf);
        }
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
