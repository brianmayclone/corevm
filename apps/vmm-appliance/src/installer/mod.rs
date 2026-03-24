use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::Frame;

use crate::common::config::ApplianceRole;

mod welcome;
mod disk;
mod network;
mod timezone;
mod users;
mod ports;
mod certs;
mod summary;
mod progress;

// ---------------------------------------------------------------------------
// InstallConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct InstallConfig {
    pub role: Option<ApplianceRole>,
    pub language: String,
    pub disk: Option<PathBuf>,
    pub network: Option<crate::common::network::NetworkConfig>,
    pub timezone: String,
    pub ntp_enabled: bool,
    pub ntp_server: String,
    pub root_password: String,
    pub username: String,
    pub user_password: String,
    pub server_port: u16,
    pub cluster_port: u16,
    pub self_signed_cert: bool,
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Screen enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Welcome,
    Disk,
    Network,
    Timezone,
    Users,
    Ports,
    Certs,
    Summary,
    Progress,
}

impl Screen {
    fn next(&self) -> Option<Screen> {
        match self {
            Screen::Welcome   => Some(Screen::Disk),
            Screen::Disk      => Some(Screen::Network),
            Screen::Network   => Some(Screen::Timezone),
            Screen::Timezone  => Some(Screen::Users),
            Screen::Users     => Some(Screen::Ports),
            Screen::Ports     => Some(Screen::Certs),
            Screen::Certs     => Some(Screen::Summary),
            Screen::Summary   => Some(Screen::Progress),
            Screen::Progress  => None,
        }
    }

    fn prev(&self) -> Option<Screen> {
        match self {
            Screen::Welcome   => None,
            Screen::Disk      => Some(Screen::Welcome),
            Screen::Network   => Some(Screen::Disk),
            Screen::Timezone  => Some(Screen::Network),
            Screen::Users     => Some(Screen::Timezone),
            Screen::Ports     => Some(Screen::Users),
            Screen::Certs     => Some(Screen::Ports),
            Screen::Summary   => Some(Screen::Certs),
            Screen::Progress  => Some(Screen::Summary),
        }
    }
}

// ---------------------------------------------------------------------------
// ScreenResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ScreenResult {
    Continue,
    Next,
    Prev,
    Quit,
}

// ---------------------------------------------------------------------------
// Per-screen state union
// ---------------------------------------------------------------------------

enum ScreenState {
    Welcome(welcome::WelcomeState),
    Disk(disk::DiskState),
    Network(network::NetworkState),
    Timezone(timezone::TimezoneState),
    Users(users::UsersState),
    Ports(ports::PortsState),
    Certs(certs::CertsState),
    Summary(summary::SummaryState),
    Progress(progress::ProgressState),
}

impl ScreenState {
    fn for_screen(screen: &Screen) -> Self {
        match screen {
            Screen::Welcome  => ScreenState::Welcome(welcome::WelcomeState::new()),
            Screen::Disk     => ScreenState::Disk(disk::DiskState::new()),
            Screen::Network  => ScreenState::Network(network::NetworkState::new()),
            Screen::Timezone => ScreenState::Timezone(timezone::TimezoneState::new()),
            Screen::Users    => ScreenState::Users(users::UsersState::new()),
            Screen::Ports    => ScreenState::Ports(ports::PortsState::new()),
            Screen::Certs    => ScreenState::Certs(certs::CertsState::new()),
            Screen::Summary  => ScreenState::Summary(summary::SummaryState::new()),
            Screen::Progress => ScreenState::Progress(progress::ProgressState::new()),
        }
    }

    fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        match self {
            ScreenState::Welcome(s)  => s.handle_key(key, config),
            ScreenState::Disk(s)     => s.handle_key(key, config),
            ScreenState::Network(s)  => s.handle_key(key, config),
            ScreenState::Timezone(s) => s.handle_key(key, config),
            ScreenState::Users(s)    => s.handle_key(key, config),
            ScreenState::Ports(s)    => s.handle_key(key, config),
            ScreenState::Certs(s)    => s.handle_key(key, config),
            ScreenState::Summary(s)  => s.handle_key(key, config),
            ScreenState::Progress(s) => s.handle_key(key, config),
        }
    }

    fn render(&mut self, frame: &mut Frame, config: &InstallConfig) {
        match self {
            ScreenState::Welcome(s)  => s.render(frame, config),
            ScreenState::Disk(s)     => s.render(frame, config),
            ScreenState::Network(s)  => s.render(frame, config),
            ScreenState::Timezone(s) => s.render(frame, config),
            ScreenState::Users(s)    => s.render(frame, config),
            ScreenState::Ports(s)    => s.render(frame, config),
            ScreenState::Certs(s)    => s.render(frame, config),
            ScreenState::Summary(s)  => s.render(frame, config),
            ScreenState::Progress(s) => s.render(frame, config),
        }
    }
}

// ---------------------------------------------------------------------------
// run()
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::Screen;

    #[test]
    fn test_screen_next_transitions() {
        assert_eq!(Screen::Welcome.next(),  Some(Screen::Disk));
        assert_eq!(Screen::Disk.next(),     Some(Screen::Network));
        assert_eq!(Screen::Network.next(),  Some(Screen::Timezone));
        assert_eq!(Screen::Timezone.next(), Some(Screen::Users));
        assert_eq!(Screen::Users.next(),    Some(Screen::Ports));
        assert_eq!(Screen::Ports.next(),    Some(Screen::Certs));
        assert_eq!(Screen::Certs.next(),    Some(Screen::Summary));
        assert_eq!(Screen::Summary.next(),  Some(Screen::Progress));
        assert_eq!(Screen::Progress.next(), None);
    }

    #[test]
    fn test_screen_prev_transitions() {
        assert_eq!(Screen::Welcome.prev(),  None);
        assert_eq!(Screen::Disk.prev(),     Some(Screen::Welcome));
        assert_eq!(Screen::Network.prev(),  Some(Screen::Disk));
        assert_eq!(Screen::Timezone.prev(), Some(Screen::Network));
        assert_eq!(Screen::Users.prev(),    Some(Screen::Timezone));
        assert_eq!(Screen::Ports.prev(),    Some(Screen::Users));
        assert_eq!(Screen::Certs.prev(),    Some(Screen::Ports));
        assert_eq!(Screen::Summary.prev(),  Some(Screen::Certs));
        assert_eq!(Screen::Progress.prev(), Some(Screen::Summary));
    }

    #[test]
    fn test_screen_full_forward_traversal() {
        let mut screen = Some(Screen::Welcome);
        let expected = vec![
            Screen::Welcome, Screen::Disk, Screen::Network, Screen::Timezone,
            Screen::Users, Screen::Ports, Screen::Certs, Screen::Summary, Screen::Progress,
        ];
        let mut visited = Vec::new();
        while let Some(s) = screen {
            screen = s.next();
            visited.push(s);
        }
        assert_eq!(visited, expected);
    }
}

pub fn run() -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let result = run_inner(&mut terminal);
    ratatui::restore();
    result
}

fn run_inner(terminal: &mut ratatui::DefaultTerminal) -> anyhow::Result<()> {
    let mut config = InstallConfig::default();
    let mut current_screen = Screen::Welcome;
    let mut state = ScreenState::for_screen(&current_screen);

    loop {
        terminal.draw(|frame| {
            state.render(frame, &config);
        })?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match state.handle_key(key, &mut config) {
                    ScreenResult::Continue => {}
                    ScreenResult::Next => {
                        if let Some(next) = current_screen.next() {
                            current_screen = next;
                            state = ScreenState::for_screen(&current_screen);
                        }
                    }
                    ScreenResult::Prev => {
                        if let Some(prev) = current_screen.prev() {
                            current_screen = prev;
                            state = ScreenState::for_screen(&current_screen);
                        }
                    }
                    ScreenResult::Quit => break,
                }
            }
        }
    }

    Ok(())
}
