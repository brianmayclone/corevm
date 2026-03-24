use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::common::widgets::{SelectList, TextInput};
use super::{InstallConfig, ScreenResult};

// ---------------------------------------------------------------------------
// Continent / city data
// ---------------------------------------------------------------------------

const CONTINENTS: &[&str] = &[
    "Africa",
    "America",
    "Antarctica",
    "Asia",
    "Atlantic",
    "Australia",
    "Europe",
    "Pacific",
];

fn cities_for(continent: &str) -> Vec<String> {
    let list: &[&str] = match continent {
        "Africa" => &[
            "Abidjan", "Accra", "Cairo", "Casablanca", "Johannesburg",
            "Khartoum", "Lagos", "Nairobi", "Tripoli", "Tunis",
        ],
        "America" => &[
            "Anchorage", "Argentina/Buenos_Aires", "Bogota", "Chicago",
            "Denver", "Halifax", "Los_Angeles", "Mexico_City",
            "New_York", "Phoenix", "Santiago", "Sao_Paulo",
            "Toronto", "Vancouver",
        ],
        "Antarctica" => &[
            "Casey", "Davis", "DumontDUrville", "Mawson", "McMurdo",
            "Palmer", "Rothera", "South_Pole", "Syowa", "Troll",
        ],
        "Asia" => &[
            "Bangkok", "Dhaka", "Dubai", "Hong_Kong", "Jakarta",
            "Jerusalem", "Karachi", "Kolkata", "Kuala_Lumpur",
            "Manila", "Riyadh", "Seoul", "Shanghai", "Singapore",
            "Taipei", "Tehran", "Tokyo",
        ],
        "Atlantic" => &[
            "Azores", "Bermuda", "Canary", "Cape_Verde", "Faroe",
            "Madeira", "Reykjavik", "South_Georgia", "St_Helena",
        ],
        "Australia" => &[
            "Adelaide", "Brisbane", "Darwin", "Hobart", "Lord_Howe",
            "Melbourne", "Perth", "Sydney",
        ],
        "Europe" => &[
            "Amsterdam", "Athens", "Belgrade", "Berlin", "Brussels",
            "Bucharest", "Budapest", "Copenhagen", "Dublin",
            "Helsinki", "Istanbul", "Lisbon", "London", "Madrid",
            "Moscow", "Oslo", "Paris", "Rome", "Stockholm",
            "Vienna", "Warsaw", "Zurich",
        ],
        "Pacific" => &[
            "Auckland", "Fiji", "Guam", "Honolulu", "Midway",
            "Noumea", "Pago_Pago", "Port_Moresby", "Tahiti",
            "Tarawa", "Tongatapu",
        ],
        _ => &[],
    };
    list.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Focus enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Field {
    Continent,
    City,
    Ntp,
    NtpServer,
}

impl Field {
    fn next(self) -> Self {
        match self {
            Field::Continent => Field::City,
            Field::City      => Field::Ntp,
            Field::Ntp       => Field::NtpServer,
            Field::NtpServer => Field::Continent,
        }
    }
    fn prev(self) -> Self {
        match self {
            Field::Continent => Field::NtpServer,
            Field::City      => Field::Continent,
            Field::Ntp       => Field::City,
            Field::NtpServer => Field::Ntp,
        }
    }
}

// ---------------------------------------------------------------------------
// TimezoneState
// ---------------------------------------------------------------------------

pub struct TimezoneState {
    continent_list: SelectList,
    city_list: SelectList,
    ntp_list: SelectList,
    ntp_server: TextInput,
    focus: Field,
    current_continent_idx: usize,
}

impl TimezoneState {
    pub fn new() -> Self {
        let continent_items: Vec<String> = CONTINENTS.iter().map(|s| s.to_string()).collect();
        let mut continent_list = SelectList::new("Continent:", continent_items);
        continent_list.focused = true;
        // Default to Europe
        let default_continent_idx = CONTINENTS.iter().position(|&c| c == "Europe").unwrap_or(6);
        continent_list.selected = default_continent_idx;

        let city_items = cities_for(CONTINENTS[default_continent_idx]);
        let mut city_list = SelectList::new("City:", city_items);
        // Default London
        city_list.selected = 0;

        let ntp_list = SelectList::new(
            "NTP:",
            vec!["Enabled".to_string(), "Disabled".to_string()],
        );

        let mut ntp_server = TextInput::new("NTP Server:");
        ntp_server.value = "pool.ntp.org".to_string();
        ntp_server.cursor = ntp_server.value.chars().count();

        Self {
            continent_list,
            city_list,
            ntp_list,
            ntp_server,
            focus: Field::Continent,
            current_continent_idx: default_continent_idx,
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

    fn set_focus(&mut self, field: Field) {
        self.continent_list.focused = false;
        self.city_list.focused = false;
        self.ntp_list.focused = false;
        self.ntp_server.focused = false;
        match field {
            Field::Continent => self.continent_list.focused = true,
            Field::City      => self.city_list.focused = true,
            Field::Ntp       => self.ntp_list.focused = true,
            Field::NtpServer => self.ntp_server.focused = true,
        }
        self.focus = field;
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        match key.code {
            KeyCode::Esc => return ScreenResult::Prev,
            KeyCode::Tab => {
                let next = self.focus.next();
                self.set_focus(next);
            }
            KeyCode::BackTab => {
                let prev = self.focus.prev();
                self.set_focus(prev);
            }
            KeyCode::Enter => {
                config.timezone = self.timezone_string();
                config.ntp_enabled = self.ntp_list.selected == 0;
                config.ntp_server = self.ntp_server.value.trim().to_string();
                if config.ntp_server.is_empty() {
                    config.ntp_server = "pool.ntp.org".to_string();
                }
                return ScreenResult::Next;
            }
            _ => {
                match self.focus {
                    Field::Continent => {
                        self.continent_list.handle_key(key);
                        self.refresh_cities();
                    }
                    Field::City      => self.city_list.handle_key(key),
                    Field::Ntp       => self.ntp_list.handle_key(key),
                    Field::NtpServer => self.ntp_server.handle_key(key),
                }
            }
        }
        ScreenResult::Continue
    }

    pub fn render(&self, frame: &mut Frame, _config: &InstallConfig) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        ratatui::widgets::Block::default()
            .style(Style::default().bg(Color::Black))
            .render(area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(1),   // 0: title
                Constraint::Length(1),   // 1: gap
                Constraint::Length(6),   // 2: continent
                Constraint::Length(1),   // 3: gap
                Constraint::Length(8),   // 4: city
                Constraint::Length(1),   // 5: gap
                Constraint::Length(4),   // 6: ntp toggle
                Constraint::Length(3),   // 7: ntp server
                Constraint::Min(0),      // 8: spacer
                Constraint::Length(1),   // 9: help
            ])
            .split(area);

        Paragraph::new("Timezone & NTP")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .render(chunks[0], buf);

        let col = centered_horizontal(area, 60);

        macro_rules! col_rect {
            ($chunk:expr) => {
                Rect { y: $chunk.y, height: $chunk.height, x: col.x, width: col.width }
            };
        }

        self.continent_list.render(col_rect!(chunks[2]), buf);
        self.city_list.render(col_rect!(chunks[4]), buf);
        self.ntp_list.render(col_rect!(chunks[6]), buf);
        self.ntp_server.render(col_rect!(chunks[7]), buf);

        // Preview
        let tz = self.timezone_string();
        Paragraph::new(format!("Selected timezone: {}", tz))
            .style(Style::default().fg(Color::Green))
            .alignment(Alignment::Center)
            .render(chunks[8], buf);

        Paragraph::new("[Tab] Next field  [↑↓] Select  [Enter] Continue  [Esc] Back")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .render(chunks[9], buf);
    }
}

fn centered_horizontal(area: Rect, percent: u16) -> Rect {
    let width = (area.width * percent / 100).max(40).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect { x, width, ..area }
}
