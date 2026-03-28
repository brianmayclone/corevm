use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::common::config::ApplianceRole;
use crate::common::widgets::{TextInput, render_installer_frame};
use super::{InstallConfig, ScreenResult};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Field {
    ServerPort,
    ClusterPort,
}

impl Field {
    fn next(self, show_cluster: bool) -> Self {
        match self {
            Field::ServerPort => {
                if show_cluster { Field::ClusterPort } else { Field::ServerPort }
            }
            Field::ClusterPort => Field::ServerPort,
        }
    }
    fn prev(self, show_cluster: bool) -> Self {
        self.next(show_cluster) // symmetric cycle
    }
}

pub struct PortsState {
    server_port: TextInput,
    cluster_port: TextInput,
    focus: Field,
    error: Option<String>,
}

impl PortsState {
    pub fn new() -> Self {
        let mut server_port = TextInput::new("Server Port:");
        server_port.value = "8443".to_string();
        server_port.cursor = server_port.value.chars().count();
        server_port.focused = true;

        let mut cluster_port = TextInput::new("Cluster Port:");
        cluster_port.value = "9443".to_string();
        cluster_port.cursor = cluster_port.value.chars().count();

        Self {
            server_port,
            cluster_port,
            focus: Field::ServerPort,
            error: None,
        }
    }

    fn show_cluster(config: &InstallConfig) -> bool {
        matches!(config.role, Some(ApplianceRole::Cluster))
    }

    fn set_focus(&mut self, field: Field) {
        self.server_port.focused = false;
        self.cluster_port.focused = false;
        match field {
            Field::ServerPort  => self.server_port.focused = true,
            Field::ClusterPort => self.cluster_port.focused = true,
        }
        self.focus = field;
    }

    fn parse_port(s: &str, name: &str) -> Result<u16, String> {
        s.trim()
            .parse::<u16>()
            .map_err(|_| format!("{} must be a number between 1 and 65535.", name))
            .and_then(|p| {
                if p == 0 { Err(format!("{} must be > 0.", name)) } else { Ok(p) }
            })
    }

    fn validate(&self, config: &InstallConfig) -> Result<(u16, u16), String> {
        let sp = Self::parse_port(&self.server_port.value, "Server port")?;
        if Self::show_cluster(config) {
            let cp = Self::parse_port(&self.cluster_port.value, "Cluster port")?;
            if sp == cp {
                return Err("Server port and cluster port must differ.".to_string());
            }
            Ok((sp, cp))
        } else {
            Ok((sp, 0))
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        let show_cluster = Self::show_cluster(config);
        match key.code {
            KeyCode::Esc => return ScreenResult::Prev,
            KeyCode::Tab => {
                let next = self.focus.next(show_cluster);
                self.set_focus(next);
            }
            KeyCode::BackTab => {
                let prev = self.focus.prev(show_cluster);
                self.set_focus(prev);
            }
            KeyCode::Enter => {
                match self.validate(config) {
                    Ok((sp, cp)) => {
                        config.server_port = sp;
                        if show_cluster {
                            config.cluster_port = cp;
                        }
                        return ScreenResult::Next;
                    }
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
            }
            _ => {
                match self.focus {
                    Field::ServerPort  => self.server_port.handle_key(key),
                    Field::ClusterPort => self.cluster_port.handle_key(key),
                }
            }
        }
        ScreenResult::Continue
    }

    pub fn render(&self, frame: &mut Frame, config: &InstallConfig) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        let content = render_installer_frame(
            area, buf,
            "Service Ports",
            "[Tab] Switch field  [Enter] Continue  [Esc] Back",
            Some((5, 8)),
        );

        let show_cluster = Self::show_cluster(config);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),   // 0: server port
                Constraint::Length(4),   // 1: cluster port
                Constraint::Min(0),      // 2: spacer
                Constraint::Length(1),   // 3: error
            ])
            .split(content);

        let col = centered_horizontal(content, 50);

        let sp_area = Rect { y: chunks[0].y, height: chunks[0].height, x: col.x, width: col.width };
        self.server_port.render(sp_area, buf);

        if show_cluster {
            let cp_area = Rect { y: chunks[1].y, height: chunks[1].height, x: col.x, width: col.width };
            self.cluster_port.render(cp_area, buf);
        }

        if let Some(err) = &self.error {
            Paragraph::new(err.as_str())
                .style(Style::default().fg(Color::Red))
                .alignment(Alignment::Center)
                .render(chunks[3], buf);
        }
    }
}

fn centered_horizontal(area: Rect, percent: u16) -> Rect {
    let width = (area.width * percent / 100).max(30).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect { x, width, ..area }
}
