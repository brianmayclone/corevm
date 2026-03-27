use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs};
use ratatui::Frame;

pub enum DialogResult {
    Continue,
    Close,
}

struct LogSource {
    name: &'static str,
    paths: &'static [&'static str],
    config_path: &'static str,
    service_name: &'static str,
}

const LOG_SOURCES: &[LogSource] = &[
    LogSource {
        name: "vmm-server",
        paths: &["/var/log/vmm/vmm-server.log", "/var/log/vmm-server.log"],
        config_path: "/etc/vmm/vmm-server.toml",
        service_name: "vmm-server.service",
    },
    LogSource {
        name: "vmm-san",
        paths: &["/var/log/vmm/vmm-san.log", "/var/log/vmm-san.log"],
        config_path: "/etc/vmm/vmm-san.toml",
        service_name: "vmm-san.service",
    },
    LogSource {
        name: "vmm-cluster",
        paths: &["/var/log/vmm/vmm-cluster.log", "/var/log/vmm-cluster.log"],
        config_path: "/etc/vmm/vmm-cluster.toml",
        service_name: "vmm-cluster.service",
    },
];

const TAIL_LINES: usize = 200;
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

pub struct Dialog {
    active_tab: usize,
    tabs: Vec<TabState>,
    debug_mode: bool,
    last_refresh: Instant,
    status_message: Option<(String, Instant)>,
}

struct TabState {
    log_path: String,
    lines: Vec<String>,
    scroll: usize,
    auto_scroll: bool,
}

impl Dialog {
    pub fn new() -> Self {
        let debug_mode = is_debug_mode_active();
        let tabs: Vec<TabState> = LOG_SOURCES.iter().map(|src| {
            let log_path = src.paths.iter()
                .find(|p| Path::new(p).exists())
                .map(|p| p.to_string())
                .unwrap_or_else(|| src.paths[0].to_string());
            let mut tab = TabState { log_path, lines: Vec::new(), scroll: 0, auto_scroll: true };
            tab.refresh();
            tab
        }).collect();

        Self {
            active_tab: 0,
            tabs,
            debug_mode,
            last_refresh: Instant::now(),
            status_message: None,
        }
    }

    fn refresh_all(&mut self) {
        for tab in &mut self.tabs {
            tab.refresh();
        }
        self.last_refresh = Instant::now();
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh_all();
        }

        match key.code {
            KeyCode::Esc => return DialogResult::Close,
            KeyCode::Tab | KeyCode::Right => {
                self.active_tab = (self.active_tab + 1) % self.tabs.len();
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.active_tab = if self.active_tab == 0 { self.tabs.len() - 1 } else { self.active_tab - 1 };
            }
            KeyCode::Up => {
                let tab = &mut self.tabs[self.active_tab];
                if tab.scroll > 0 { tab.scroll -= 1; }
                tab.auto_scroll = false;
            }
            KeyCode::Down => {
                let tab = &mut self.tabs[self.active_tab];
                if tab.scroll + 1 < tab.lines.len() { tab.scroll += 1; }
            }
            KeyCode::PageUp => {
                let tab = &mut self.tabs[self.active_tab];
                tab.scroll = tab.scroll.saturating_sub(20);
                tab.auto_scroll = false;
            }
            KeyCode::PageDown => {
                let tab = &mut self.tabs[self.active_tab];
                let max = tab.lines.len().saturating_sub(1);
                tab.scroll = (tab.scroll + 20).min(max);
            }
            KeyCode::Home => {
                let tab = &mut self.tabs[self.active_tab];
                tab.scroll = 0;
                tab.auto_scroll = false;
            }
            KeyCode::End => {
                let tab = &mut self.tabs[self.active_tab];
                tab.scroll = tab.lines.len().saturating_sub(1);
                tab.auto_scroll = true;
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                self.toggle_debug_mode();
            }
            _ => {}
        }
        DialogResult::Continue
    }

    fn toggle_debug_mode(&mut self) {
        let new_level = if self.debug_mode { "info" } else { "debug" };

        for src in LOG_SOURCES {
            if Path::new(src.config_path).exists() {
                if let Err(e) = set_log_level(src.config_path, new_level) {
                    self.status_message = Some((
                        format!("Error updating {}: {}", src.config_path, e),
                        Instant::now(),
                    ));
                    return;
                }
                // Restart service to pick up new log level
                let _ = Command::new("systemctl")
                    .args(["restart", src.service_name])
                    .output();
            }
        }

        self.debug_mode = !self.debug_mode;
        let mode_name = if self.debug_mode { "DEBUG" } else { "INFO" };
        self.status_message = Some((
            format!("Log level set to {} — services restarting...", mode_name),
            Instant::now(),
        ));
    }

    pub fn render(&mut self, frame: &mut Frame) {
        // Auto-refresh on each render cycle if enough time has elapsed
        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh_all();
        }

        let area = frame.area();
        let popup_width = area.width.saturating_sub(4).max(40);
        let popup_height = area.height.saturating_sub(4).max(10);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let debug_indicator = if self.debug_mode { " [DEBUG]" } else { "" };
        let title = format!(" Logs (F7){} ", debug_indicator);
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

        let inner = block.inner(popup);
        block.render(popup, buf);

        if inner.height < 4 {
            return;
        }

        // Tab bar
        let tab_titles: Vec<Line> = LOG_SOURCES.iter().enumerate().map(|(i, src)| {
            let style = if i == self.active_tab {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(src.name, style))
        }).collect();

        let tabs_widget = Tabs::new(tab_titles)
            .select(self.active_tab)
            .divider(" | ")
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

        let tab_area = Rect { height: 1, ..inner };
        tabs_widget.render(tab_area, buf);

        // File path line
        let tab = &self.tabs[self.active_tab];
        let path_area = Rect { y: inner.y + 1, height: 1, ..inner };
        let path_style = if Path::new(&tab.log_path).exists() {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        Paragraph::new(Span::styled(&tab.log_path, path_style))
            .render(path_area, buf);

        // Log content
        let content_height = inner.height.saturating_sub(4) as usize;
        let content_area = Rect { y: inner.y + 2, height: content_height as u16, ..inner };

        let start = if tab.lines.len() > content_height {
            let max_scroll = tab.lines.len().saturating_sub(content_height);
            tab.scroll.min(max_scroll)
        } else {
            0
        };

        let visible: Vec<Line> = tab.lines.iter()
            .skip(start)
            .take(content_height)
            .map(|l| Line::from(Span::raw(l.as_str())))
            .collect();

        Paragraph::new(visible)
            .style(Style::default().fg(Color::White))
            .render(content_area, buf);

        // Status message or help line
        let help_y = inner.y + inner.height.saturating_sub(1);
        let help_area = Rect { y: help_y, height: 1, ..inner };

        if let Some((ref msg, at)) = self.status_message {
            if at.elapsed() < Duration::from_secs(5) {
                Paragraph::new(msg.as_str())
                    .style(Style::default().fg(Color::Yellow))
                    .render(help_area, buf);
                return;
            }
        }

        let debug_key = if self.debug_mode { "[D] Info-Modus" } else { "[D] Debug-Modus" };
        let help = format!("[Tab] Service  [↑↓ PgUp/PgDn] Scroll  {}  [Esc] Close", debug_key);
        Paragraph::new(help)
            .style(Style::default().fg(Color::DarkGray))
            .render(help_area, buf);
    }
}

impl TabState {
    fn refresh(&mut self) {
        self.lines = read_last_lines(&self.log_path, TAIL_LINES);
        if self.auto_scroll {
            self.scroll = self.lines.len().saturating_sub(1);
        }
    }
}

fn read_last_lines(path: &str, n: usize) -> Vec<String> {
    match fs::read_to_string(path) {
        Ok(content) => {
            let all: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            let skip = all.len().saturating_sub(n);
            all.into_iter().skip(skip).collect()
        }
        Err(e) => vec![format!("Log-Datei nicht verfügbar: {} ({})", path, e)],
    }
}

/// Check if any service config currently has debug level
fn is_debug_mode_active() -> bool {
    for src in LOG_SOURCES {
        if let Ok(content) = fs::read_to_string(src.config_path) {
            if content.contains("level = \"debug\"") {
                return true;
            }
        }
    }
    false
}

/// Update the log level in a TOML config file
fn set_log_level(config_path: &str, level: &str) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|e| format!("read error: {}", e))?;

    // Replace the level line in [logging] section
    let mut result = String::new();
    let mut in_logging = false;
    let mut replaced = false;

    for line in content.lines() {
        if line.trim() == "[logging]" {
            in_logging = true;
            result.push_str(line);
            result.push('\n');
            continue;
        }
        if in_logging && line.starts_with('[') {
            in_logging = false;
        }
        if in_logging && line.trim().starts_with("level") {
            result.push_str(&format!("level = \"{}\"", level));
            result.push('\n');
            replaced = true;
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }

    if !replaced {
        // If no level line found, append to [logging] section or create one
        if content.contains("[logging]") {
            result = content.replace("[logging]", &format!("[logging]\nlevel = \"{}\"", level));
        } else {
            result.push_str(&format!("\n[logging]\nlevel = \"{}\"\n", level));
        }
    }

    fs::write(config_path, result)
        .map_err(|e| format!("write error: {}", e))
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
