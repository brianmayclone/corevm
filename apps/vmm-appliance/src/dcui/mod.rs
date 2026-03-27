mod status;
mod network;
mod passwords;
mod ports;
mod api_access;
mod certs;
mod services;
mod time;
mod logs;
mod update;
mod shell;
mod reboot;
mod diagnostics;
mod reset;

use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use ratatui::Frame;

use status::StatusBar;

// ---------------------------------------------------------------------------
// DialogResult — shared type used by all dialog modules
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum DialogResult {
    Continue,
    Close,
}

// ---------------------------------------------------------------------------
// Active dialog state
// ---------------------------------------------------------------------------

enum ActiveDialog {
    None,
    Network(network::Dialog),
    Passwords(passwords::Dialog),
    Ports(ports::Dialog),
    ApiAccess(api_access::Dialog),
    Certs(certs::Dialog),
    Services(services::Dialog),
    Time(time::Dialog),
    Logs(logs::Dialog),
    Update(update::Dialog),
    Reboot(reboot::Dialog),
    Diagnostics(diagnostics::Dialog),
    Reset(reset::Dialog),
}

impl ActiveDialog {
    fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        match self {
            ActiveDialog::None          => DialogResult::Close,
            ActiveDialog::Network(d)    => network(d.handle_key(key)),
            ActiveDialog::Passwords(d)  => passwords(d.handle_key(key)),
            ActiveDialog::Ports(d)      => ports(d.handle_key(key)),
            ActiveDialog::ApiAccess(d)  => api_access(d.handle_key(key)),
            ActiveDialog::Certs(d)      => certs(d.handle_key(key)),
            ActiveDialog::Services(d)   => services(d.handle_key(key)),
            ActiveDialog::Time(d)       => time(d.handle_key(key)),
            ActiveDialog::Logs(d)       => logs(d.handle_key(key)),
            ActiveDialog::Update(d)     => update(d.handle_key(key)),
            ActiveDialog::Reboot(d)     => reboot(d.handle_key(key)),
            ActiveDialog::Diagnostics(d)=> diagnostics(d.handle_key(key)),
            ActiveDialog::Reset(d)      => reset(d.handle_key(key)),
        }
    }

    fn render(&self, frame: &mut Frame) {
        match self {
            ActiveDialog::None          => {}
            ActiveDialog::Network(d)    => d.render(frame),
            ActiveDialog::Passwords(d)  => d.render(frame),
            ActiveDialog::Ports(d)      => d.render(frame),
            ActiveDialog::ApiAccess(d)  => d.render(frame),
            ActiveDialog::Certs(d)      => d.render(frame),
            ActiveDialog::Services(d)   => d.render(frame),
            ActiveDialog::Time(d)       => d.render(frame),
            ActiveDialog::Logs(d)       => d.render(frame),
            ActiveDialog::Update(d)     => d.render(frame),
            ActiveDialog::Reboot(d)     => d.render(frame),
            ActiveDialog::Diagnostics(d)=> d.render(frame),
            ActiveDialog::Reset(d)      => d.render(frame),
        }
    }

    fn is_none(&self) -> bool {
        matches!(self, ActiveDialog::None)
    }
}

macro_rules! impl_map {
    ($mod:ident) => {
        fn $mod(r: $mod::DialogResult) -> DialogResult {
            match r {
                $mod::DialogResult::Continue => DialogResult::Continue,
                $mod::DialogResult::Close    => DialogResult::Close,
            }
        }
    };
}

impl_map!(network);
impl_map!(passwords);
impl_map!(ports);
impl_map!(api_access);
impl_map!(certs);
impl_map!(services);
impl_map!(time);
impl_map!(logs);
impl_map!(update);
impl_map!(shell);
impl_map!(reboot);
impl_map!(diagnostics);
impl_map!(reset);

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

pub fn run() -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let result = run_inner(&mut terminal);
    ratatui::restore();
    result
}

fn run_inner(terminal: &mut ratatui::DefaultTerminal) -> anyhow::Result<()> {
    let mut status_bar = StatusBar::new();
    let mut active_dialog = ActiveDialog::None;

    loop {
        status_bar.refresh();

        terminal.draw(|frame| {
            let area = frame.area();

            // Layout: status (7) + spacer (1) + menu (min 14) + help (1)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(7),
                    Constraint::Length(1),
                    Constraint::Min(14),
                    Constraint::Length(1),
                ])
                .split(area);

            // Render status bar
            status_bar.render(chunks[0], frame.buffer_mut());

            // Render menu
            render_menu(chunks[2], frame.buffer_mut());

            // Help line
            let help = Paragraph::new(Line::from(vec![
                Span::styled(" F10", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(":Shell  "),
                Span::styled("F11", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(":Reboot  "),
                Span::styled("F12", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(":Diag  "),
                Span::styled(" r", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(":Reset"),
            ]));
            help.render(chunks[3], frame.buffer_mut());

            // Overlay active dialog on top
            if !active_dialog.is_none() {
                active_dialog.render(frame);
            }
        })?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if !active_dialog.is_none() {
                    let result = active_dialog.handle_key(key);
                    if result == DialogResult::Close {
                        active_dialog = ActiveDialog::None;
                    }
                } else {
                    match key.code {
                        KeyCode::F(1)  => active_dialog = ActiveDialog::Network(network::Dialog::new()),
                        KeyCode::F(2)  => active_dialog = ActiveDialog::Passwords(passwords::Dialog::new()),
                        KeyCode::F(3)  => active_dialog = ActiveDialog::Ports(ports::Dialog::new()),
                        KeyCode::F(4)  => active_dialog = ActiveDialog::Certs(certs::Dialog::new()),
                        KeyCode::F(5)  => active_dialog = ActiveDialog::Services(services::Dialog::new()),
                        KeyCode::F(6)  => active_dialog = ActiveDialog::Time(time::Dialog::new()),
                        KeyCode::F(7)  => active_dialog = ActiveDialog::Logs(logs::Dialog::new()),
                        KeyCode::F(8)  => active_dialog = ActiveDialog::Update(update::Dialog::new()),
                        KeyCode::F(9)  => active_dialog = ActiveDialog::ApiAccess(api_access::Dialog::new()),
                        KeyCode::F(10) => {
                            // Special: restore terminal, spawn bash, re-init on return
                            ratatui::restore();
                            let _ = std::process::Command::new("bash").status();
                            *terminal = ratatui::init();
                        }
                        KeyCode::F(11) => active_dialog = ActiveDialog::Reboot(reboot::Dialog::new()),
                        KeyCode::F(12) => active_dialog = ActiveDialog::Diagnostics(diagnostics::Dialog::new()),
                        KeyCode::Char('r') => active_dialog = ActiveDialog::Reset(reset::Dialog::new()),
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Menu rendering
// ---------------------------------------------------------------------------

struct MenuEntry {
    key: &'static str,
    label: &'static str,
    color: Color,
}

fn menu_entries() -> Vec<MenuEntry> {
    vec![
        MenuEntry { key: "F1",  label: "Network",     color: Color::Cyan },
        MenuEntry { key: "F2",  label: "Passwords",   color: Color::Cyan },
        MenuEntry { key: "F3",  label: "Ports",       color: Color::Cyan },
        MenuEntry { key: "F4",  label: "Certs",       color: Color::Cyan },
        MenuEntry { key: "F5",  label: "Services",    color: Color::Cyan },
        MenuEntry { key: "F6",  label: "Time/NTP",    color: Color::Cyan },
        MenuEntry { key: "F7",  label: "Logs",        color: Color::Cyan },
        MenuEntry { key: "F8",  label: "Update",      color: Color::Cyan },
        MenuEntry { key: "F9",  label: "API Access",  color: Color::Cyan },
        MenuEntry { key: "F10", label: "Shell",       color: Color::Yellow },
        MenuEntry { key: "F11", label: "Reboot",      color: Color::Yellow },
        MenuEntry { key: "F12", label: "Diagnostics", color: Color::Green },
    ]
}

fn render_menu(area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .title(" Management Menu ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White));

    let inner = block.inner(area);
    block.render(area, buf);

    let entries = menu_entries();
    // Two columns
    let col_width = inner.width / 2;
    let rows = (entries.len() + 1) / 2;

    for (i, entry) in entries.iter().enumerate() {
        let col = (i / rows) as u16;
        let row = (i % rows) as u16;

        let x = inner.x + col * col_width;
        let y = inner.y + row + 1; // +1 for top padding

        if y >= inner.y + inner.height {
            break;
        }

        let line = Line::from(vec![
            Span::styled(
                format!(" {:>3} ", entry.key),
                Style::default().fg(entry.color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                entry.label,
                Style::default().fg(Color::White),
            ),
        ]);

        let cell_area = Rect {
            x,
            y,
            width: col_width,
            height: 1,
        };

        Paragraph::new(line).render(cell_area, buf);
    }
}
