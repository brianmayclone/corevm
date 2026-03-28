use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::common::config::ApplianceRole;
use crate::common::widgets::render_installer_frame;
use super::{InstallConfig, ScreenResult};

pub struct SummaryState {}

impl SummaryState {
    pub fn new() -> Self {
        Self {}
    }

    pub fn handle_key(&mut self, key: KeyEvent, _config: &mut InstallConfig) -> ScreenResult {
        match key.code {
            KeyCode::Esc   => ScreenResult::Prev,
            KeyCode::Enter => ScreenResult::Next,
            _              => ScreenResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, config: &InstallConfig) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        let content = render_installer_frame(
            area, buf,
            "Installation Summary",
            "[Enter] Start Installation  [Esc] Go Back",
            Some((8, 8)),
        );

        // Build summary rows
        let rows = build_summary_rows(config);
        let text: Vec<Line> = rows
            .iter()
            .map(|(k, v)| {
                Line::from(vec![
                    Span::styled(
                        format!("  {:<22}", k),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(v.as_str(), Style::default().fg(Color::White)),
                ])
            })
            .collect();

        let col = centered_horizontal(content, 70);
        let table_area = Rect { y: content.y, height: content.height, x: col.x, width: col.width };

        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(60, 70, 85))),
            )
            .render(table_area, buf);
    }
}

fn build_summary_rows(config: &InstallConfig) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();

    let role = match &config.role {
        Some(ApplianceRole::Server)  => "Standalone Server",
        Some(ApplianceRole::Cluster) => "Cluster Controller",
        None                         => "(not set)",
    };
    rows.push(("Role:".to_string(), role.to_string()));

    let lang = if config.language.is_empty() { "(not set)".to_string() } else { config.language.clone() };
    rows.push(("Language:".to_string(), lang));

    let disk = config
        .disk
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(not set)".to_string());
    rows.push(("Disk:".to_string(), disk));

    // Network
    if let Some(net) = &config.network {
        rows.push(("Interface:".to_string(), net.interface.clone()));
        if net.dhcp {
            rows.push(("IP Mode:".to_string(), "DHCP".to_string()));
        } else {
            let addr = net.address.as_deref().unwrap_or("(none)");
            let gw = net.gateway.as_deref().unwrap_or("(none)");
            rows.push(("IP Mode:".to_string(), "Static".to_string()));
            rows.push(("IP Address:".to_string(), addr.to_string()));
            rows.push(("Gateway:".to_string(), gw.to_string()));
            let dns = if net.dns.is_empty() {
                "(none)".to_string()
            } else {
                net.dns.join(", ")
            };
            rows.push(("DNS:".to_string(), dns));
        }
        rows.push(("Hostname:".to_string(), net.hostname.clone()));
    } else {
        rows.push(("Network:".to_string(), "(not set)".to_string()));
    }

    // Timezone
    let tz = if config.timezone.is_empty() { "(not set)".to_string() } else { config.timezone.clone() };
    rows.push(("Timezone:".to_string(), tz));
    rows.push(("NTP:".to_string(), if config.ntp_enabled { "Enabled".to_string() } else { "Disabled".to_string() }));
    if config.ntp_enabled {
        rows.push(("NTP Server:".to_string(), config.ntp_server.clone()));
    }

    // Users
    let root_set = if config.root_password.is_empty() { "not set" } else { "set" };
    rows.push(("Root Password:".to_string(), format!("[{}]", root_set)));
    let user = if config.username.is_empty() { "(not set)".to_string() } else { config.username.clone() };
    rows.push(("Username:".to_string(), user));
    let user_pw = if config.user_password.is_empty() { "not set" } else { "set" };
    rows.push(("User Password:".to_string(), format!("[{}]", user_pw)));

    // Ports
    if config.server_port != 0 {
        rows.push(("Server Port:".to_string(), config.server_port.to_string()));
    } else {
        rows.push(("Server Port:".to_string(), "(not set)".to_string()));
    }
    if matches!(config.role, Some(ApplianceRole::Cluster)) && config.cluster_port != 0 {
        rows.push(("Cluster Port:".to_string(), config.cluster_port.to_string()));
    }

    // API Access
    rows.push(("CLI/API Access:".to_string(),
        if config.cli_access_enabled { "Enabled".to_string() } else { "Disabled".to_string() }));

    // Certs
    let cert_mode = if config.self_signed_cert {
        "Self-signed (generated)".to_string()
    } else {
        let cert = config
            .cert_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".to_string());
        format!("Custom ({})", cert)
    };
    rows.push(("Certificate:".to_string(), cert_mode));

    rows
}

fn centered_horizontal(area: Rect, percent: u16) -> Rect {
    let width = (area.width * percent / 100).max(50).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect { x, width, ..area }
}
