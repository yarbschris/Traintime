//! Application state and input handling. No terminal or network access here:
//! key presses come in, state changes, and at most one `Cmd` goes out for the
//! event loop to carry out.

use std::cell::Cell;
use std::time::Instant;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use traintime_server::config::SupportedTransitSystem as Sys;
use traintime_server::types::static_data::StaticData;
use traintime_server::types::status::ServerStatus;
use traintime_server::types::traintime_packet::TraintimePacket;

use crate::routes;

pub struct SystemInfo {
    pub sys: Sys,
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub sample_routes: &'static [&'static str],
}

pub const SYSTEMS: [SystemInfo; 3] = [
    SystemInfo {
        sys: Sys::NycSubway,
        id: "NycSubway",
        name: "NYC Subway",
        blurb: "New York City · MTA",
        sample_routes: &["1", "4", "7", "A", "F", "G", "L", "Q"],
    },
    SystemInfo {
        sys: Sys::MtaMetroNorth,
        id: "MtaMetroNorth",
        name: "Metro-North",
        blurb: "New York & Connecticut · MTA",
        sample_routes: &["1", "2", "3"],
    },
    SystemInfo {
        sys: Sys::BostonTransit,
        id: "BostonTransit",
        name: "MBTA",
        blurb: "Greater Boston",
        sample_routes: &["Red", "Orange", "Blue", "Green-B"],
    },
];

/// How often the server polls the realtime feeds.
pub const REFRESH_SECS: u64 = 30;

pub struct Station {
    pub name: String,
    lower: String,
    pub routes: Vec<String>,
}

pub struct Arrival {
    pub route: String,
    pub headsign: String,
    pub mins: i64,
    /// Seconds behind schedule (negative when early), when the feed reports it
    pub delay: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Systems,
    Stations,
    Board,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cmd {
    Quit,
    /// Send this selection to the server
    Select {
        system: usize,
        station: Option<String>,
    },
}

pub struct App {
    pub screen: Screen,
    /// Highlighted row on the system screen
    pub system_cursor: usize,
    /// The system the server has been told to load, as an index into `SYSTEMS`
    pub system: Option<usize>,
    /// Stations of `system`; `None` until its schedule has loaded
    pub stations: Option<Vec<Station>>,
    pub filter: String,
    /// Indices into `stations` that match `filter`
    pub filtered: Vec<usize>,
    pub station_cursor: usize,
    pub station: Option<String>,
    pub arrivals: Vec<Arrival>,
    pub arrivals_at: Option<Instant>,
    /// A station was just selected and its first arrivals have not landed yet
    pub awaiting: bool,
    pub board_scroll: usize,
    /// Furthest the board can scroll, as measured by the last draw
    pub board_max_scroll: Cell<usize>,
    pub status: ServerStatus,
    pub show_logs: bool,
    pub show_help: bool,
    pub tick: u64,
}

impl Default for App {
    fn default() -> Self {
        App {
            screen: Screen::Systems,
            system_cursor: 0,
            system: None,
            stations: None,
            filter: String::new(),
            filtered: Vec::new(),
            station_cursor: 0,
            station: None,
            arrivals: Vec::new(),
            arrivals_at: None,
            awaiting: false,
            board_scroll: 0,
            board_max_scroll: Cell::new(0),
            status: ServerStatus::Starting,
            show_logs: false,
            show_help: false,
            tick: 0,
        }
    }
}

impl App {
    pub fn system_info(&self) -> Option<&'static SystemInfo> {
        self.system.and_then(|i| SYSTEMS.get(i))
    }

    /// Routes serving the selected station.
    pub fn station_routes(&self) -> &[String] {
        let Some(name) = self.station.as_deref() else {
            return &[];
        };
        self.stations
            .as_deref()
            .and_then(|stations| stations.iter().find(|s| s.name == name))
            .map_or(&[], |s| s.routes.as_slice())
    }

    // ---- server updates -------------------------------------------------

    pub fn on_status(&mut self, status: ServerStatus) {
        self.status = status;
    }

    pub fn on_static(&mut self, static_data: Option<&StaticData>) {
        let Some(info) = self.system_info() else {
            return;
        };
        // After a system switch the channel can still hold the previous system's data
        let Some(data) = static_data.filter(|d| d.built_from_system == info.sys) else {
            self.stations = None;
            self.filtered.clear();
            return;
        };

        let mut stations: Vec<Station> = data
            .route_lookup
            .iter()
            .filter(|(_, route_ids)| !route_ids.is_empty())
            .map(|(name, route_ids)| {
                let mut routes: Vec<String> = route_ids.iter().map(|r| r.0.clone()).collect();
                routes.sort_by_key(|r| routes::sort_key(r));
                Station {
                    name: name.0.clone(),
                    lower: name.0.to_lowercase(),
                    routes,
                }
            })
            .collect();
        // Numbered streets in numeric order ("2 Av" before "14 St"), then by name
        stations.sort_by_cached_key(|s| {
            let digits: String = s.lower.chars().take_while(char::is_ascii_digit).collect();
            (digits.parse::<u32>().unwrap_or(u32::MAX), s.lower.clone())
        });
        self.stations = Some(stations);
        self.refilter();
    }

    pub fn on_packets(&mut self, packets: &[TraintimePacket]) {
        if self.station.is_none() {
            return;
        }
        // A failed fetch still publishes an empty list; keep showing what we had
        if packets.is_empty() && self.status == ServerStatus::FetchFailed && !self.awaiting {
            return;
        }
        let mut arrivals: Vec<Arrival> = packets
            .iter()
            .map(|p| Arrival {
                route: p.route_id.0.clone(),
                headsign: p.trip_headsign.0.clone(),
                mins: p.mins_until_arrival,
                delay: p.delay,
            })
            .collect();
        arrivals.sort_by_key(|a| a.mins);
        self.arrivals = arrivals;
        self.arrivals_at = Some(Instant::now());
        self.awaiting = false;
    }

    // ---- actions --------------------------------------------------------

    pub fn choose_system(&mut self, index: usize) -> Option<Cmd> {
        if index >= SYSTEMS.len() {
            return None;
        }
        self.system_cursor = index;
        self.screen = Screen::Stations;
        self.clear_station();
        self.filter.clear();
        self.station_cursor = 0;
        if self.system != Some(index) {
            self.system = Some(index);
            self.stations = None;
        }
        self.refilter();
        Some(Cmd::Select {
            system: index,
            station: None,
        })
    }

    pub fn choose_station(&mut self, name: String) -> Option<Cmd> {
        let system = self.system?;
        self.clear_station();
        self.station = Some(name.clone());
        self.awaiting = true;
        self.screen = Screen::Board;
        Some(Cmd::Select {
            system,
            station: Some(name),
        })
    }

    fn clear_station(&mut self) {
        self.station = None;
        self.arrivals.clear();
        self.arrivals_at = None;
        self.awaiting = false;
        self.board_scroll = 0;
    }

    fn back_to_stations(&mut self) -> Option<Cmd> {
        let system = self.system?;
        self.clear_station();
        self.screen = Screen::Stations;
        Some(Cmd::Select {
            system,
            station: None,
        })
    }

    fn scroll_board(&mut self, by: isize) {
        let max = self.board_max_scroll.get();
        self.board_scroll = self.board_scroll.min(max).saturating_add_signed(by).min(max);
    }

    fn refilter(&mut self) {
        let Some(stations) = self.stations.as_deref() else {
            self.filtered.clear();
            self.station_cursor = 0;
            return;
        };
        let needle = self.filter.to_lowercase();
        let tokens: Vec<&str> = needle.split_whitespace().collect();
        let mut matches: Vec<usize> = stations
            .iter()
            .enumerate()
            .filter(|(_, s)| tokens.iter().all(|t| s.lower.contains(t)))
            .map(|(i, _)| i)
            .collect();
        // Names that start with the query come first; the sort is stable, so each
        // half stays alphabetical
        let trimmed = needle.trim();
        if !trimmed.is_empty() {
            matches.sort_by_key(|&i| !stations[i].lower.starts_with(trimmed));
        }
        self.filtered = matches;
        self.station_cursor = self
            .station_cursor
            .min(self.filtered.len().saturating_sub(1));
    }

    // ---- input ----------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) -> Option<Cmd> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return Some(Cmd::Quit);
        }
        if self.show_help {
            self.show_help = false;
            return None;
        }
        match key.code {
            KeyCode::F(1) => {
                self.show_help = true;
                return None;
            }
            KeyCode::F(2) => {
                self.show_logs = !self.show_logs;
                return None;
            }
            KeyCode::Char('l') if ctrl => {
                self.show_logs = !self.show_logs;
                return None;
            }
            _ => {}
        }
        match self.screen {
            Screen::Systems => self.on_key_systems(key),
            Screen::Stations => self.on_key_stations(key, ctrl),
            Screen::Board => self.on_key_board(key),
        }
    }

    /// Keys shared by the screens that have no text input.
    fn on_key_common(&mut self, key: KeyEvent) -> Option<Cmd> {
        match key.code {
            KeyCode::Char('q') => return Some(Cmd::Quit),
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('l') => self.show_logs = !self.show_logs,
            _ => {}
        }
        None
    }

    fn on_key_systems(&mut self, key: KeyEvent) -> Option<Cmd> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.system_cursor = self.system_cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.system_cursor = (self.system_cursor + 1).min(SYSTEMS.len() - 1);
            }
            KeyCode::Enter | KeyCode::Right => return self.choose_system(self.system_cursor),
            KeyCode::Char(c @ '1'..='3') => {
                return self.choose_system(c as usize - '1' as usize);
            }
            KeyCode::Esc => return Some(Cmd::Quit),
            _ => return self.on_key_common(key),
        }
        None
    }

    fn on_key_stations(&mut self, key: KeyEvent, ctrl: bool) -> Option<Cmd> {
        let last = self.filtered.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => {
                if self.filter.is_empty() {
                    self.screen = Screen::Systems;
                } else {
                    self.filter.clear();
                    self.station_cursor = 0;
                    self.refilter();
                }
            }
            KeyCode::Left if self.filter.is_empty() => self.screen = Screen::Systems,
            KeyCode::Up => self.station_cursor = self.station_cursor.saturating_sub(1),
            KeyCode::Down => self.station_cursor = (self.station_cursor + 1).min(last),
            KeyCode::PageUp => self.station_cursor = self.station_cursor.saturating_sub(10),
            KeyCode::PageDown => self.station_cursor = (self.station_cursor + 10).min(last),
            KeyCode::Home => self.station_cursor = 0,
            KeyCode::End => self.station_cursor = last,
            KeyCode::Enter | KeyCode::Right => {
                let name = self
                    .filtered
                    .get(self.station_cursor)
                    .and_then(|&i| self.stations.as_ref()?.get(i))
                    .map(|s| s.name.clone());
                if let Some(name) = name {
                    return self.choose_station(name);
                }
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.station_cursor = 0;
                self.refilter();
            }
            KeyCode::Char('u') if ctrl => {
                self.filter.clear();
                self.station_cursor = 0;
                self.refilter();
            }
            KeyCode::Char(c) if !ctrl && self.filter.chars().count() < 40 => {
                self.filter.push(c);
                self.station_cursor = 0;
                self.refilter();
            }
            _ => {}
        }
        None
    }

    fn on_key_board(&mut self, key: KeyEvent) -> Option<Cmd> {
        match key.code {
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Left | KeyCode::Char('s') => {
                return self.back_to_stations();
            }
            KeyCode::Char('r') => {
                // Re-sending the selection makes the server fetch immediately
                let system = self.system?;
                return Some(Cmd::Select {
                    system,
                    station: self.station.clone(),
                });
            }
            KeyCode::Up | KeyCode::Char('k') => self.scroll_board(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_board(1),
            KeyCode::PageUp => self.scroll_board(-10),
            KeyCode::PageDown => self.scroll_board(10),
            KeyCode::End => self.scroll_board(isize::MAX),
            KeyCode::Home => self.board_scroll = 0,
            _ => return self.on_key_common(key),
        }
        None
    }
}

#[cfg(test)]
pub mod fixtures {
    use super::*;

    pub fn station(name: &str, routes: &[&str]) -> Station {
        Station {
            name: name.to_string(),
            lower: name.to_lowercase(),
            routes: routes.iter().map(|r| r.to_string()).collect(),
        }
    }

    pub fn arrival(route: &str, headsign: &str, mins: i64, delay: Option<i32>) -> Arrival {
        Arrival {
            route: route.to_string(),
            headsign: headsign.to_string(),
            mins,
            delay,
        }
    }

    /// An app on the station picker with a handful of NYC stations loaded.
    pub fn with_stations() -> App {
        let mut app = App::default();
        app.choose_system(0);
        app.stations = Some(vec![
            station("14 St-Union Sq", &["4", "5", "6", "L", "N", "Q", "R", "W"]),
            station("86 St (1)", &["1"]),
            station("86 St (4, 5, 6)", &["4", "5", "6", "6X"]),
            station("Delancey St-Essex St", &["F", "J", "M", "Z"]),
            station("East Broadway", &["F"]),
            station("Times Sq-42 St", &["1", "2", "3", "7", "GS", "N", "Q", "R", "W"]),
        ]);
        app.status = ServerStatus::NoStationSelected;
        app.refilter();
        app
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    fn press(app: &mut App, code: KeyCode) -> Option<Cmd> {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    #[test]
    fn choosing_a_system_selects_it_without_a_station() {
        let mut app = App::default();
        press(&mut app, KeyCode::Down);
        let cmd = press(&mut app, KeyCode::Enter);

        assert_eq!(
            cmd,
            Some(Cmd::Select {
                system: 1,
                station: None
            })
        );
        assert_eq!(app.screen, Screen::Stations);
    }

    #[test]
    fn typing_filters_stations_and_q_does_not_quit() {
        let mut app = with_stations();
        type_text(&mut app, "sq");

        let names: Vec<&str> = app
            .filtered
            .iter()
            .map(|&i| app.stations.as_ref().unwrap()[i].name.as_str())
            .collect();
        assert_eq!(names, ["14 St-Union Sq", "Times Sq-42 St"]);
    }

    #[test]
    fn prefix_matches_rank_first() {
        let mut app = with_stations();
        type_text(&mut app, "e");

        let first = app.filtered[0];
        assert_eq!(app.stations.as_ref().unwrap()[first].name, "East Broadway");
    }

    #[test]
    fn enter_selects_the_highlighted_station() {
        let mut app = with_stations();
        type_text(&mut app, "east");
        let cmd = press(&mut app, KeyCode::Enter);

        assert_eq!(
            cmd,
            Some(Cmd::Select {
                system: 0,
                station: Some("East Broadway".to_string())
            })
        );
        assert_eq!(app.screen, Screen::Board);
        assert!(app.awaiting);
    }

    #[test]
    fn escape_clears_the_filter_before_leaving() {
        let mut app = with_stations();
        type_text(&mut app, "east");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Stations);
        assert!(app.filter.is_empty());

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Systems);
    }

    #[test]
    fn leaving_the_board_drops_the_station_and_its_arrivals() {
        let mut app = with_stations();
        app.choose_station("East Broadway".to_string());
        app.arrivals = vec![arrival("F", "Jamaica-179 St", 3, None)];
        let cmd = press(&mut app, KeyCode::Esc);

        assert_eq!(
            cmd,
            Some(Cmd::Select {
                system: 0,
                station: None
            })
        );
        assert!(app.arrivals.is_empty());
        assert_eq!(app.screen, Screen::Stations);
    }

    #[test]
    fn failed_fetch_keeps_the_previous_arrivals() {
        let mut app = with_stations();
        app.choose_station("East Broadway".to_string());
        app.awaiting = false;
        app.arrivals = vec![arrival("F", "Jamaica-179 St", 3, None)];
        app.on_status(ServerStatus::FetchFailed);
        app.on_packets(&[]);

        assert_eq!(app.arrivals.len(), 1);
    }

    #[test]
    fn cursor_cannot_run_past_the_list() {
        let mut app = with_stations();
        for _ in 0..50 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.station_cursor, 5);
        type_text(&mut app, "zzz");
        assert_eq!(app.station_cursor, 0);
        assert_eq!(press(&mut app, KeyCode::Enter), None);
    }
}
