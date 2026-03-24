use std::fs;
use std::process::Command;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::widgets::{SelectList, TextInput};

pub enum DialogResult {
    Continue,
    Close,
}

const CONTINENTS: &[&str] = &[
    "Africa", "America", "Antarctica", "Asia", "Atlantic",
    "Australia", "Europe", "Pacific",
];

fn cities_for(continent: &str) -> Vec<String> {
    let list: &[&str] = match continent {
        "Africa"     => &["Abidjan","Accra","Cairo","Casablanca","Johannesburg","Lagos","Nairobi","Tunis"],
        "America"    => &["Anchorage","Argentina/Buenos_Aires","Bogota","Chicago","Denver","Los_Angeles","Mexico_City","New_York","Sao_Paulo","Toronto","Vancouver"],
        "Antarctica" => &["Casey","Davis","McMurdo","South_Pole","Syowa"],
        "Asia"       => &["Bangkok","Dhaka","Dubai","Hong_Kong","Jakarta","Karachi","Kolkata","Seoul","Shanghai","Singapore","Tokyo"],
        "Atlantic"   => &["Azores","Canary","Reykjavik"],
        "Australia"  => &["Adelaide","Brisbane","Darwin","Melbourne","Perth","Sydney"],
        "Europe"     => &["Amsterdam","Athens","Berlin","Brussels","Bucharest","Budapest","Copenhagen","Dublin","Helsinki","Istanbul","Lisbon","London","Madrid","Moscow","Oslo","Paris","Rome","Stockholm","Vienna","Warsaw","Zurich"],
        "Pacific"    => &["Auckland","Fiji","Guam","Honolulu","Noumea","Port_Moresby","Tahiti"],
        _            => &[],
    };
    list.iter().map(|s| s.to_string()).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Continent,
    City,
    Ntp,
    NtpServer,
}

impl Field {
    fn next(self) -> Self {
        match self {
            Field::Continent  => Field::City,
            Field::City       => Field::Ntp,
            Field::Ntp        => Field::NtpServer,
            Field::NtpServer  => Field::Continent,
        }
    }
    fn prev(self) -> Self {
        match self {
            Field::Continent  => Field::NtpServer,
            Field::City       => Field::Continent,
            Field::Ntp        => Field::City,
            Field::NtpServer  => Field::Ntp,
        }
    }
}

pub struct Dialog {
    continent_list: SelectList,
    city_list: SelectList,
    ntp_list: SelectList,
    ntp_server: TextInput,
    focus: Field,
    current_continent_idx: usize,
    message: Option<(String, bool)>,
    current_time: String,
}

impl Dialog {
    pub fn new() -> Self {
        let continent_items: Vec<String> = CONTINENTS.iter().map(|s| s.to_string()).collect();
        let default_idx = CONTINENTS.iter().position(|&c| c == "Europe").unwrap_or(6);

        let mut continent_list = SelectList::new("Continent:", continent_items);
        continent_list.selected = default_idx;
        continent_list.focused = true;

        let city_items = cities_for(CONTINENTS[default_idx]);
        let city_list = SelectList::new("City:", city_items);

        let ntp_list = SelectList::new(
            "NTP:",
            vec!["Enabled".to_string(), "Disabled".to_string()],
        );

        let mut ntp_server = TextInput::new("NTP Server:");
        ntp_server.value = "pool.ntp.org".to_string();
        ntp_server.cursor = ntp_server.value.chars().count();

        let current_time = get_current_time();

        Self {
            continent_list,
            city_list,
            ntp_list,
            ntp_server,
            focus: Field::Continent,
            current_continent_idx: default_idx,
            message: None,
            current_time,
        }
    }

    fn refresh_cities(&mut self) {
        let idx = self.continent_list.selected;
        if idx != self.current_continent_idx {
            self.current_continent_idx = idx;
            self.city_list.items = cities_for(CONTINENTS[idx]);
            self.city_list.selected = 0;
        }
    }

    fn timezone_string(&self) -> String {
        let continent = CONTINENTS[self.continent_list.selected];
        let cities = cities_for(continent);
        if cities.is_empty() {
            return continent.to_string();
        }
        let city = &cities[self.city_list.selected.min(cities.len().saturating_sub(1))];
        format!("{}/{}", continent, city)
    }

    fn set_focus(&mut self, f: Field) {
        self.continent_list.focused = f == Field::Continent;
        self.city_list.focused = f == Field::City;
        self.ntp_list.focused = f == Field::Ntp;
        self.ntp_server.focused = f == Field::NtpServer;
        self.focus = f;
    }

    fn save(&mut self) {
        let tz = self.timezone_string();
        let ntp_enabled = self.ntp_list.selected == 0;
        let ntp_server = {
            let s = self.ntp_server.value.trim().to_string();
            if s.is_empty() { "pool.ntp.org".to_string() } else { s }
        };

        // Write /etc/timezone
        let _ = fs::write("/etc/timezone", format!("{}\n", tz));

        // Symlink /etc/localtime
        let tz_link = format!("/usr/share/zoneinfo/{}", tz);
        let _ = Command::new("ln")
            .args(["-sf", &tz_link, "/etc/localtime"])
            .status();

        // Write chrony.conf
        let chrony_conf = if ntp_enabled {
            format!(
                "# CoreVM chrony configuration\nserver {} iburst\n\ndriftfile /var/lib/chrony/drift\nmakestep 1.0 3\nrtcsync\nlogdir /var/log/chrony\n",
                ntp_server
            )
        } else {
            "# CoreVM chrony configuration\n# NTP disabled\n\ndriftfile /var/lib/chrony/drift\nmakestep 1.0 3\nrtcsync\nlogdir /var/log/chrony\n".to_string()
        };

        if let Some(parent) = std::path::Path::new("/etc/chrony/chrony.conf").parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write("/etc/chrony/chrony.conf", chrony_conf);

        // Restart chronyd
        let _ = Command::new("systemctl").args(["restart", "chronyd"]).status();

        // Sync now if NTP enabled
        if ntp_enabled {
            let _ = Command::new("chronyc").arg("makestep").status();
        }

        self.current_time = get_current_time();
        self.message = Some((format!("Timezone set to {}. Chrony restarted.", tz), false));
    }

    fn sync_now(&mut self) {
        match Command::new("chronyc").arg("makestep").status() {
            Ok(s) if s.success() => {
                self.current_time = get_current_time();
                self.message = Some(("Time synchronised with NTP.".to_string(), false));
            }
            _ => {
                self.message = Some(("chronyc makestep failed.".to_string(), true));
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
            KeyCode::Tab => {
                let next = self.focus.next();
                self.set_focus(next);
            }
            KeyCode::BackTab => {
                let prev = self.focus.prev();
                self.set_focus(prev);
            }
            KeyCode::Char('s') | KeyCode::Char('S') => self.sync_now(),
            KeyCode::Enter => {
                if self.focus == Field::NtpServer {
                    self.save();
                } else {
                    let next = self.focus.next();
                    self.set_focus(next);
                }
            }
            _ => match self.focus {
                Field::Continent => {
                    self.continent_list.handle_key(key);
                    self.refresh_cities();
                }
                Field::City => self.city_list.handle_key(key),
                Field::Ntp => self.ntp_list.handle_key(key),
                Field::NtpServer => self.ntp_server.handle_key(key),
            },
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = area.width.min(65).max(45);
        let popup_height = 30u16.min(area.height);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let block = Block::default()
            .title(" Time & NTP (F6) ")
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
                Constraint::Length(1), // time display
                Constraint::Length(1), // spacer
                Constraint::Length(5), // continent
                Constraint::Length(5), // city
                Constraint::Length(4), // ntp toggle
                Constraint::Length(3), // ntp server
                Constraint::Min(1),    // spacer
                Constraint::Length(1), // help
            ])
            .split(inner);

        Paragraph::new(format!("Current time: {}", self.current_time))
            .style(Style::default().fg(Color::Green))
            .render(chunks[0], buf);

        self.continent_list.render(chunks[2], buf);
        self.city_list.render(chunks[3], buf);
        self.ntp_list.render(chunks[4], buf);
        self.ntp_server.render(chunks[5], buf);

        Paragraph::new("[Tab] Next  [Enter on NTP Server] Save  [S] Sync Now  [Esc] Cancel")
            .style(Style::default().fg(Color::DarkGray))
            .render(chunks[7], buf);
    }
}

fn get_current_time() -> String {
    Command::new("date")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
