use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::common::network::{apply_and_verify, detect_interfaces, generate_default_hostname, NetworkConfig, NetworkInterface};
use crate::common::widgets::{SelectList, TextInput, render_installer_frame};
use super::{InstallConfig, ScreenResult};
use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq)]
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
            Field::Address  => Field::Gateway,
            Field::Gateway  => Field::Dns,
            Field::Dns      => Field::Hostname,
            Field::Hostname => Field::Interface,
        }
    }

    fn prev(self, dhcp: bool) -> Self {
        match self {
            Field::Interface => Field::Hostname,
            Field::Mode      => Field::Interface,
            Field::Address   => Field::Mode,
            Field::Gateway   => Field::Address,
            Field::Dns       => Field::Gateway,
            Field::Hostname  => {
                if dhcp { Field::Mode } else { Field::Dns }
            }
        }
    }
}

pub struct NetworkState {
    interfaces: Vec<NetworkInterface>,
    iface_list: SelectList,
    mode_list: SelectList,
    address: TextInput,
    gateway: TextInput,
    dns: TextInput,
    hostname: TextInput,
    focus: Field,
    error: Option<String>,
    verifying: bool,
    verify_status: Option<String>,
    verify_rx: Option<mpsc::Receiver<Result<(NetworkConfig, String), String>>>,
}

impl NetworkState {
    pub fn new() -> Self {
        let interfaces = detect_interfaces().unwrap_or_default();

        let iface_items: Vec<String> = interfaces
            .iter()
            .map(|i| {
                let link = if i.has_link { "link" } else { "no link" };
                format!("{} ({}) [{}]", i.name, i.mac, link)
            })
            .collect();

        let mut iface_list = SelectList::new("Network Interface:", iface_items);
        iface_list.focused = true;

        let mode_list = SelectList::new(
            "Configuration Mode:",
            vec!["DHCP".to_string(), "Static".to_string()],
        );

        let mut hostname = TextInput::new("Hostname:");
        hostname.value = generate_default_hostname();
        hostname.cursor = hostname.value.chars().count();

        Self {
            interfaces,
            iface_list,
            mode_list,
            address: TextInput::new("IP Address (CIDR, e.g. 192.168.1.10/24):"),
            gateway: TextInput::new("Default Gateway:"),
            dns: TextInput::new("DNS Server(s) (comma separated):"),
            hostname,
            focus: Field::Interface,
            error: None,
            verifying: false,
            verify_status: None,
            verify_rx: None,
        }
    }

    fn is_dhcp(&self) -> bool {
        self.mode_list.selected == 0
    }

    fn set_focus(&mut self, field: Field) {
        self.iface_list.focused = false;
        self.mode_list.focused = false;
        self.address.focused = false;
        self.gateway.focused = false;
        self.dns.focused = false;
        self.hostname.focused = false;

        match field {
            Field::Interface => self.iface_list.focused = true,
            Field::Mode      => self.mode_list.focused = true,
            Field::Address   => self.address.focused = true,
            Field::Gateway   => self.gateway.focused = true,
            Field::Dns       => self.dns.focused = true,
            Field::Hostname  => self.hostname.focused = true,
        }
        self.focus = field;
    }

    fn validate(&self) -> Result<NetworkConfig, String> {
        if self.interfaces.is_empty() {
            return Err("No network interfaces detected.".to_string());
        }
        let iface = &self.interfaces[self.iface_list.selected];
        let hostname = self.hostname.value.trim().to_string();
        if hostname.is_empty() {
            return Err("Hostname must not be empty.".to_string());
        }

        if self.is_dhcp() {
            Ok(NetworkConfig {
                interface: iface.name.clone(),
                dhcp: true,
                address: None,
                gateway: None,
                dns: vec![],
                hostname,
            })
        } else {
            let address = self.address.value.trim().to_string();
            if address.is_empty() {
                return Err("IP address is required for static configuration.".to_string());
            }
            let gateway = self.gateway.value.trim().to_string();
            if gateway.is_empty() {
                return Err("Gateway is required for static configuration.".to_string());
            }
            let dns_str = self.dns.value.trim().to_string();
            let dns: Vec<String> = if dns_str.is_empty() {
                vec![]
            } else {
                dns_str.split(',').map(|s| s.trim().to_string()).collect()
            };
            Ok(NetworkConfig {
                interface: iface.name.clone(),
                dhcp: false,
                address: Some(address),
                gateway: Some(gateway),
                dns,
                hostname,
            })
        }
    }

    pub fn tick(&mut self, config: &mut InstallConfig) -> Option<ScreenResult> {
        if let Some(rx) = &self.verify_rx {
            if let Ok(result) = rx.try_recv() {
                self.verifying = false;
                self.verify_rx = None;
                match result {
                    Ok((cfg, ip)) => {
                        self.verify_status = Some(format!("IP acquired: {}", ip));
                        config.network = Some(cfg);
                        return Some(ScreenResult::Next);
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.verify_status = None;
                    }
                }
            }
        }
        None
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        // Ignore input while verifying network
        if self.verifying {
            return ScreenResult::Continue;
        }

        match key.code {
            KeyCode::Esc => return ScreenResult::Prev,
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
                match self.validate() {
                    Ok(cfg) => {
                        self.error = None;
                        self.verifying = true;
                        let timeout = if cfg.dhcp { 15 } else { 5 };
                        self.verify_status = Some(if cfg.dhcp {
                            "Requesting DHCP lease...".to_string()
                        } else {
                            "Applying network configuration...".to_string()
                        });

                        let (tx, rx) = mpsc::channel();
                        self.verify_rx = Some(rx);
                        let cfg_clone = cfg.clone();
                        std::thread::spawn(move || {
                            let result = apply_and_verify(&cfg_clone, timeout)
                                .map(|ip| (cfg_clone, ip))
                                .map_err(|e| format!("{}", e));
                            let _ = tx.send(result);
                        });
                    }
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
            }
            _ => {
                match self.focus {
                    Field::Interface => self.iface_list.handle_key(key),
                    Field::Mode      => self.mode_list.handle_key(key),
                    Field::Address   => self.address.handle_key(key),
                    Field::Gateway   => self.gateway.handle_key(key),
                    Field::Dns       => self.dns.handle_key(key),
                    Field::Hostname  => self.hostname.handle_key(key),
                }
            }
        }
        ScreenResult::Continue
    }

    pub fn render(&self, frame: &mut Frame, _config: &InstallConfig) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        let help = if self.verifying {
            "Verifying network configuration..."
        } else {
            "[Tab] Next field  [↑↓] Select/type  [Enter] Continue  [Esc] Back"
        };

        let content = render_installer_frame(
            area, buf,
            "Network Configuration",
            help,
            Some((2, 8)),
        );

        let dhcp = self.is_dhcp();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),   // 0: interface list
                Constraint::Length(1),   // 1: gap
                Constraint::Length(4),   // 2: mode list
                Constraint::Length(1),   // 3: gap
                Constraint::Length(4),   // 4: address
                Constraint::Length(4),   // 5: gateway
                Constraint::Length(4),   // 6: dns
                Constraint::Length(4),   // 7: hostname
                Constraint::Min(0),      // 8: spacer
                Constraint::Length(1),   // 9: error/status
            ])
            .split(content);

        let col = centered_horizontal(content, 70);

        let iface_area = Rect { y: chunks[0].y, height: chunks[0].height, x: col.x, width: col.width };
        self.iface_list.render(iface_area, buf);

        let mode_area = Rect { y: chunks[2].y, height: chunks[2].height, x: col.x, width: col.width };
        self.mode_list.render(mode_area, buf);

        let grey = Style::default().fg(Color::DarkGray);

        let addr_area = Rect { y: chunks[4].y, height: chunks[4].height, x: col.x, width: col.width };
        if dhcp {
            render_greyed_field("IP Address (CIDR, e.g. 192.168.1.10/24):", addr_area, buf, grey);
        } else {
            self.address.render(addr_area, buf);
        }

        let gw_area = Rect { y: chunks[5].y, height: chunks[5].height, x: col.x, width: col.width };
        if dhcp {
            render_greyed_field("Default Gateway:", gw_area, buf, grey);
        } else {
            self.gateway.render(gw_area, buf);
        }

        let dns_area = Rect { y: chunks[6].y, height: chunks[6].height, x: col.x, width: col.width };
        if dhcp {
            render_greyed_field("DNS Server(s) (comma separated):", dns_area, buf, grey);
        } else {
            self.dns.render(dns_area, buf);
        }

        let host_area = Rect { y: chunks[7].y, height: chunks[7].height, x: col.x, width: col.width };
        self.hostname.render(host_area, buf);

        if let Some(status) = &self.verify_status {
            Paragraph::new(status.as_str())
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center)
                .render(chunks[9], buf);
        } else if let Some(err) = &self.error {
            Paragraph::new(err.as_str())
                .style(Style::default().fg(Color::Red))
                .alignment(Alignment::Center)
                .render(chunks[9], buf);
        }
    }
}

fn render_greyed_field(label: &str, area: Rect, buf: &mut Buffer, style: Style) {
    if area.height < 2 {
        return;
    }
    let label_area = Rect { height: 1, ..area };
    let box_area = Rect { y: area.y + 1, height: area.height - 1, ..area };
    Paragraph::new(label)
        .style(style)
        .render(label_area, buf);
    ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .render(box_area, buf);
}

fn centered_horizontal(area: Rect, percent: u16) -> Rect {
    let width = (area.width * percent / 100).max(40).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect { x, width, ..area }
}
