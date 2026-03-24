use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

pub enum DialogResult {
    Continue,
    Close,
}

const LOG_PATHS: &[&str] = &[
    "/var/log/vmm/vmm-server.log",
    "/var/log/vmm/vmm-cluster.log",
    "/var/log/vmm-server.log",
    "/var/log/vmm-cluster.log",
    "/var/log/syslog",
];

const TAIL_LINES: usize = 50;
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

pub struct Dialog {
    log_path: String,
    lines: Vec<String>,
    scroll: usize,
    last_refresh: Instant,
}

impl Dialog {
    pub fn new() -> Self {
        let log_path = LOG_PATHS.iter()
            .find(|p| Path::new(p).exists())
            .map(|p| p.to_string())
            .unwrap_or_else(|| "/var/log/syslog".to_string());

        let mut dlg = Self {
            log_path,
            lines: Vec::new(),
            scroll: 0,
            last_refresh: Instant::now(),
        };
        dlg.refresh();
        dlg
    }

    fn refresh(&mut self) {
        self.lines = read_last_lines(&self.log_path, TAIL_LINES);
        self.last_refresh = Instant::now();
        // Auto-scroll to bottom
        self.scroll = self.lines.len().saturating_sub(1);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        // Auto-refresh check
        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh();
        }

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

        let title = format!(" Logs (F7) - {} ", self.log_path);
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

        let inner = block.inner(popup);
        block.render(popup, buf);

        if inner.height == 0 {
            return;
        }

        let content_height = inner.height.saturating_sub(1) as usize;

        // Determine which lines to show
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

        // Help line
        let help_area = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
        Paragraph::new("[↑↓ PgUp/PgDn] Scroll  [Esc] Close  (auto-refresh 2s)")
            .style(Style::default().fg(Color::DarkGray))
            .render(help_area, buf);
    }
}

fn read_last_lines(path: &str, n: usize) -> Vec<String> {
    let content = fs::read_to_string(path).unwrap_or_else(|e| format!("Error reading {}: {}", path, e));
    let all: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let skip = all.len().saturating_sub(n);
    all.into_iter().skip(skip).collect()
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
