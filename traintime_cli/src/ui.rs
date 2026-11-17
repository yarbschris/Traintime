//! Rendering. `draw` is a pure function of `App` (plus the clock and the log
//! buffer), so every screen can be rendered into a test backend.

use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use traintime_server::config::SupportedTransitSystem as Sys;
use traintime_server::types::status::ServerStatus;

use crate::app::{App, Arrival, REFRESH_SECS, SYSTEMS, Screen};
use crate::{logger, routes};

// ---- palette ---------------------------------------------------------------

const BG: u32 = 0x0e1016;
const PANEL: u32 = 0x161a24;
const SURFACE: u32 = 0x222838;
const BORDER: u32 = 0x343b50;
const TEXT: u32 = 0xe6e9f0;
const DIM: u32 = 0x8b93a7;
const FAINT: u32 = 0x555d72;
const ACCENT: u32 = 0xffb000;
const GREEN: u32 = 0x74c991;
const RED: u32 = 0xec7a6a;

static TRUECOLOR: AtomicBool = AtomicBool::new(true);

/// Terminals without 24-bit colour (macOS Terminal.app) get the nearest
/// 256-colour palette entry instead.
pub fn set_truecolor(enabled: bool) {
    TRUECOLOR.store(enabled, Ordering::Relaxed);
}

fn c(hex: u32) -> Color {
    let (r, g, b) = ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    if TRUECOLOR.load(Ordering::Relaxed) {
        return Color::Rgb(r, g, b);
    }
    // xterm's 6×6×6 cube uses these levels, not an even ramp
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |v: u8| {
        (0..6u8)
            .min_by_key(|&i| (LEVELS[i as usize] as i32 - v as i32).abs())
            .unwrap_or(0)
    };
    let distance = |cr: u8, cg: u8, cb: u8| {
        let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
        d(cr, r) + d(cg, g) + d(cb, b)
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube_distance = distance(
        LEVELS[ri as usize],
        LEVELS[gi as usize],
        LEVELS[bi as usize],
    );
    // The 24-step grey ramp (232..=255) runs 8, 18, .. 238
    let mean = (r as u16 + g as u16 + b as u16) / 3;
    let step = ((mean.saturating_sub(8) + 5) / 10).min(23) as u8;
    let grey = 8 + 10 * step;
    if distance(grey, grey, grey) < cube_distance {
        Color::Indexed(232 + step)
    } else {
        Color::Indexed(16 + 36 * ri + 6 * gi + bi)
    }
}

fn fg(hex: u32) -> Style {
    Style::new().fg(c(hex))
}

fn bold(hex: u32) -> Style {
    fg(hex).add_modifier(Modifier::BOLD)
}

// ---- small helpers ---------------------------------------------------------

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn spinner(app: &App) -> &'static str {
    SPINNER[app.tick as usize % SPINNER.len()]
}

/// Truncates to `width` cells, ending in an ellipsis when cut.
fn fit(text: &str, width: usize) -> String {
    if Line::raw(text).width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        if Line::raw(out.as_str()).width() > width.saturating_sub(1) {
            out.pop();
            break;
        }
    }
    out.push('…');
    out
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// The main content column: a side margin, capped so wide terminals stay readable.
fn column(area: Rect) -> Rect {
    let area = area.inner(Margin::new(2, 0));
    centered(area, 110, area.height)
}

fn row(area: Rect, offset: u16) -> Rect {
    Rect {
        x: area.x,
        y: area.y.saturating_add(offset),
        width: area.width,
        height: if offset < area.height { 1 } else { 0 },
    }
}

fn rounded(title: &str) -> Block<'_> {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(BORDER));
    if title.is_empty() {
        block
    } else {
        block.title(Span::styled(format!(" {title} "), fg(DIM)))
    }
}

/// A route bullet: the label on the line's colour, with half-block end caps.
fn badge(system: Sys, route: &str, wide: bool) -> Vec<Span<'static>> {
    let badge = routes::badge(system, route);
    let color = c(badge.color);
    let text = if badge.dark_text() {
        Color::Black
    } else {
        Color::White
    };
    let label = if wide {
        format!(" {} ", badge.label)
    } else {
        badge.label.clone()
    };
    vec![
        Span::styled("▐", Style::new().fg(color)),
        Span::styled(
            label,
            Style::new()
                .fg(text)
                .bg(color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("▌", Style::new().fg(color)),
    ]
}

fn badge_width(system: Sys, route: &str, wide: bool) -> usize {
    Line::from(badge(system, route, wide)).width()
}

/// As many badges as fit in `max_width`, then a "+n" for the rest.
fn badge_row(system: Sys, route_ids: &[String], max_width: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut used = 0;
    for (i, route) in route_ids.iter().enumerate() {
        let width = badge_width(system, route, false);
        let rest = route_ids.len() - i;
        // Leave room for the overflow marker unless this is the last badge
        let reserve = if rest > 1 { 4 } else { 0 };
        if used + width + reserve > max_width {
            spans.push(Span::styled(format!("+{rest}"), fg(DIM)));
            break;
        }
        spans.extend(badge(system, route, false));
        used += width;
    }
    spans
}

fn urgency(mins: i64) -> u32 {
    match mins {
        ..=1 => GREEN,
        2..=5 => ACCENT,
        _ => TEXT,
    }
}

fn delay_text(delay: Option<i32>) -> Option<(String, u32)> {
    let secs = delay?;
    match secs {
        60.. => Some((format!("{} min late", secs / 60), RED)),
        1..=59 => Some((format!("{secs}s late"), DIM)),
        ..=-60 => Some((format!("{} min early", -secs / 60), GREEN)),
        _ => None,
    }
}

// ---- entry point -----------------------------------------------------------

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    f.render_widget(
        Block::new().style(Style::new().bg(c(BG)).fg(c(TEXT))),
        area,
    );
    if area.width < 34 || area.height < 9 {
        let msg = Paragraph::new("traintime needs a larger window")
            .style(fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(msg, centered(area, area.width, 1));
        return;
    }

    let log_height = if app.show_logs {
        (area.height / 3).clamp(4, 10)
    } else {
        0
    };
    let [header, body, logs, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(log_height),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(f, app, header);
    match app.screen {
        Screen::Systems => draw_systems(f, app, body),
        Screen::Stations => draw_stations(f, app, body),
        Screen::Board => draw_board(f, app, body),
    }
    if app.show_logs {
        draw_logs(f, column(logs));
    }
    draw_footer(f, app, footer);
    if app.show_help {
        draw_help(f, area);
    }
}

// ---- chrome ----------------------------------------------------------------

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Block::new().style(Style::new().bg(c(PANEL))), area);

    let clock = chrono::Local::now().format("%H:%M:%S").to_string();
    let mut spans = vec![
        Span::styled(" ◆ ", fg(ACCENT)),
        Span::styled("traintime", bold(TEXT)),
    ];
    let crumb_width = (area.width as usize).saturating_sub(13 + clock.len() + 6);
    if app.screen != Screen::Systems
        && let Some(info) = app.system_info()
    {
        let mut crumb = info.name.to_string();
        if let Some(station) = &app.station {
            crumb.push_str(" › ");
            crumb.push_str(station);
        }
        spans.push(Span::styled("  │  ", fg(FAINT)));
        spans.push(Span::styled(fit(&crumb, crumb_width), fg(DIM)));
    }
    f.render_widget(Line::from(spans), area);
    f.render_widget(
        Line::from(Span::styled(format!("{clock} "), fg(DIM))).right_aligned(),
        area,
    );
}

/// The status pill: a glyph, a short message and a colour.
fn status_pill(app: &App) -> (String, String, u32) {
    let spin = spinner(app).to_string();
    let loading_schedule = app.system.is_some() && app.stations.is_none();
    match &app.status {
        ServerStatus::UnknownStation(_) => ("✕".into(), "Unknown station".into(), RED),
        ServerStatus::NoEndpointsForStation => ("✕".into(), "No realtime feed".into(), RED),
        ServerStatus::FetchFailed => ("✕".into(), "Fetch failed · retrying".into(), RED),
        _ if loading_schedule => (spin, "Loading schedule".into(), DIM),
        ServerStatus::Starting => (spin, "Starting".into(), DIM),
        ServerStatus::LoadingStaticData => (spin, "Loading schedule".into(), DIM),
        ServerStatus::Resolving => (spin, "Resolving".into(), DIM),
        ServerStatus::NoSelection => ("○".into(), "Choose a system".into(), DIM),
        ServerStatus::NoStationSelected => match app.screen {
            Screen::Board => (spin, "Resolving".into(), DIM),
            _ => ("○".into(), "Choose a station".into(), DIM),
        },
        ServerStatus::Ready if app.awaiting => (spin, "Fetching arrivals".into(), DIM),
        ServerStatus::Ready => ("●".into(), "Live".into(), GREEN),
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Block::new().style(Style::new().bg(c(PANEL))), area);

    let (glyph, message, color) = status_pill(app);
    let left = Line::from(vec![
        Span::styled(format!(" {glyph} "), fg(color)),
        Span::styled(message, fg(color)),
    ]);
    let left_width = left.width();
    f.render_widget(left, area);

    let hints: &[(&str, &str)] = match app.screen {
        Screen::Systems => &[("↑↓", "move"), ("⏎", "select"), ("l", "logs"), ("q", "quit")],
        Screen::Stations => &[
            ("type", "filter"),
            ("↑↓", "move"),
            ("⏎", "select"),
            ("esc", "back"),
        ],
        Screen::Board => &[
            ("esc", "stations"),
            ("r", "refresh"),
            ("l", "logs"),
            ("?", "help"),
            ("q", "quit"),
        ],
    };
    // Drop hints from the front until the rest fits beside the status
    let room = (area.width as usize).saturating_sub(left_width + 3);
    for start in 0..hints.len() {
        let mut spans = Vec::new();
        for (key, what) in &hints[start..] {
            spans.push(Span::styled(*key, bold(TEXT)));
            spans.push(Span::styled(format!(" {what}   "), fg(DIM)));
        }
        let line = Line::from(spans);
        if line.width() <= room {
            let hint_area = Rect {
                width: area.width.saturating_sub(1),
                ..area
            };
            f.render_widget(line.right_aligned(), hint_area);
            break;
        }
    }
}

fn draw_logs(f: &mut Frame, area: Rect) {
    let block = rounded("Server log").style(Style::new().bg(c(BG)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines: Vec<Line> = logger::tail(inner.height as usize)
        .into_iter()
        .map(|entry| {
            let color = match entry.level {
                log::Level::Error => RED,
                log::Level::Warn => ACCENT,
                _ => FAINT,
            };
            Line::from(vec![
                Span::styled(format!(" {} ", entry.time), fg(FAINT)),
                Span::styled(format!("{:<5} ", entry.level), fg(color)),
                Span::styled(entry.message, fg(DIM)),
            ])
        })
        .collect();
    if lines.is_empty() {
        f.render_widget(
            Line::from(Span::styled(" Nothing logged yet", fg(FAINT))),
            row(inner, 0),
        );
    } else {
        f.render_widget(Paragraph::new(lines), inner);
    }
}

fn draw_help(f: &mut Frame, area: Rect) {
    let keys: [(&str, &str); 9] = [
        ("↑ ↓  j k", "Move / scroll"),
        ("⏎  →", "Select"),
        ("esc  ←", "Go back"),
        ("type", "Filter stations"),
        ("ctrl-u", "Clear the filter"),
        ("r", "Refresh arrivals now"),
        ("l  F2  ctrl-l", "Toggle the server log"),
        ("?  F1", "This help"),
        ("q  ctrl-c", "Quit"),
    ];
    let popup = centered(area, 48, keys.len() as u16 + 4);
    f.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(BORDER))
        .title(Span::styled(" Keys ", fg(DIM)))
        .padding(Padding::new(2, 2, 1, 1))
        .style(Style::new().bg(c(PANEL)).fg(c(TEXT)));
    let lines: Vec<Line> = keys
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!("{key:<16}"), bold(TEXT)),
                Span::styled(*what, fg(DIM)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

/// A centred spinner (or glyph) with a headline and a dimmer second line.
fn draw_notice(f: &mut Frame, area: Rect, glyph: &str, color: u32, title: &str, detail: &str) {
    let lines = vec![
        Line::from(Span::styled(glyph.to_string(), fg(color))),
        Line::raw(""),
        Line::from(Span::styled(title.to_string(), bold(TEXT))),
        Line::from(Span::styled(
            fit(detail, area.width.saturating_sub(4) as usize),
            fg(DIM),
        )),
    ];
    let height = lines.len() as u16;
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        centered(area, area.width, height),
    );
}

// ---- system picker ---------------------------------------------------------

fn draw_systems(f: &mut Frame, app: &App, area: Rect) {
    let roomy = area.height >= 22;
    let card_height: u16 = if area.height >= 16 { 4 } else { 2 };
    let logo_height: u16 = if roomy { 6 } else { 0 };
    let cards_height = card_height * SYSTEMS.len() as u16;
    let column = centered(area, 56, logo_height + cards_height);

    let [logo, cards] =
        Layout::vertical([Constraint::Length(logo_height), Constraint::Min(0)]).areas(column);

    if roomy {
        // A stretch of track with three stations on it
        let rail = Style::new().fg(c(BORDER));
        let stop = |hex: u32| Span::styled("●", fg(hex));
        let track = Line::from(vec![
            Span::styled("╺━━━━━━━", rail),
            stop(0xee352e),
            Span::styled("━━━━━━━━━━━", rail),
            stop(ACCENT),
            Span::styled("━━━━━━━━━━━", rail),
            stop(0x0039a6),
            Span::styled("━━━━━━━╸", rail),
        ]);
        let lines = vec![
            track,
            Line::raw(""),
            Line::from(Span::styled("traintime", bold(TEXT))),
            Line::from(Span::styled("live arrivals for one station", fg(DIM))),
        ];
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), logo);
    }

    for (i, info) in SYSTEMS.iter().enumerate() {
        let card = Rect {
            x: cards.x,
            y: cards.y + card_height * i as u16,
            width: cards.width,
            height: card_height.min(cards.height.saturating_sub(card_height * i as u16)),
        };
        if card.height == 0 {
            break;
        }
        let selected = i == app.system_cursor;
        let marker = if selected {
            Span::styled("▸ ", fg(ACCENT))
        } else {
            Span::raw("  ")
        };
        let name_style = if selected { bold(TEXT) } else { fg(TEXT) };
        let title = Line::from(vec![
            marker,
            Span::styled(format!("{}  ", i + 1), fg(FAINT)),
            Span::styled(info.name, name_style),
        ]);
        let sample_ids: Vec<String> = info.sample_routes.iter().map(|r| r.to_string()).collect();
        let sample_room = (card.width as usize).saturating_sub(title.width() + 6);
        let samples = badge_row(info.sys, &sample_ids, sample_room);
        let blurb = Line::from(Span::styled(format!("     {}", info.blurb), fg(DIM)));

        if card_height == 4 {
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(if selected { fg(ACCENT) } else { fg(BORDER) })
                .padding(Padding::horizontal(1));
            let inner = block.inner(card);
            f.render_widget(block, card);
            f.render_widget(title, row(inner, 0));
            f.render_widget(blurb, row(inner, 1));
            f.render_widget(Line::from(samples).right_aligned(), row(inner, 0));
        } else {
            if selected {
                f.buffer_mut()
                    .set_style(row(card, 0), Style::new().bg(c(SURFACE)));
            }
            f.render_widget(title, row(card, 0));
            f.render_widget(Line::from(samples).right_aligned(), row(card, 0));
        }
    }
}

// ---- station picker --------------------------------------------------------

fn draw_stations(f: &mut Frame, app: &App, area: Rect) {
    let Some(info) = app.system_info() else {
        return;
    };
    let area = column(area);
    let [_, search, list] = Layout::vertical([
        Constraint::Length(if area.height > 12 { 1 } else { 0 }),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .areas(area);

    let Some(stations) = app.stations.as_deref() else {
        let last_log = logger::tail(1).pop();
        let (glyph, color, detail) = match last_log {
            Some(entry) if entry.level <= log::Level::Warn => ("✕", RED, entry.message),
            Some(entry) => (spinner(app), DIM, entry.message),
            None => (spinner(app), DIM, "Downloading the GTFS schedule".into()),
        };
        let title = format!("Loading the {} schedule", info.name);
        draw_notice(f, area, glyph, color, &title, &detail);
        return;
    };

    // Search box
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(BORDER))
        .title(Span::styled(format!(" {} stations ", info.name), fg(DIM)));
    let inner = block.inner(search);
    f.render_widget(block, search);
    let count = format!("{} of {} ", app.filtered.len(), stations.len());
    let mut input = vec![Span::styled(" ⌕ ", fg(DIM))];
    if app.filter.is_empty() {
        input.push(Span::styled("▏", fg(ACCENT)));
        input.push(Span::styled("Type to filter", fg(FAINT)));
    } else {
        let room = (inner.width as usize).saturating_sub(count.len() + 6);
        input.push(Span::styled(fit(&app.filter, room), fg(TEXT)));
        input.push(Span::styled("▏", fg(ACCENT)));
    }
    f.render_widget(Line::from(input), inner);
    f.render_widget(
        Line::from(Span::styled(count, fg(DIM))).right_aligned(),
        inner,
    );

    if app.filtered.is_empty() {
        let detail = format!("Nothing matches “{}”", app.filter);
        draw_notice(f, list, "∅", DIM, "No stations", &detail);
        return;
    }

    // List, with the cursor kept near the middle
    let height = list.height as usize;
    if height == 0 {
        return;
    }
    let total = app.filtered.len();
    let cursor = app.station_cursor.min(total - 1);
    let offset = cursor
        .saturating_sub(height / 2)
        .min(total.saturating_sub(height));
    let rows = Rect {
        width: list.width.saturating_sub(2),
        ..list
    };
    for (line, &index) in app.filtered[offset..].iter().take(height).enumerate() {
        let Some(station) = stations.get(index) else {
            continue;
        };
        let line_area = row(rows, line as u16);
        let selected = offset + line == cursor;
        if selected {
            f.buffer_mut()
                .set_style(line_area, Style::new().bg(c(SURFACE)));
        }
        let width = line_area.width as usize;
        let badge_room = (width / 2).min(width.saturating_sub(16));
        let badges = Line::from(badge_row(info.sys, &station.routes, badge_room));
        let name_room = width.saturating_sub(badges.width() + 6);
        let name = Line::from(vec![
            if selected {
                Span::styled("  ▸ ", fg(ACCENT))
            } else {
                Span::raw("    ")
            },
            Span::styled(
                fit(&station.name, name_room),
                if selected { bold(TEXT) } else { fg(TEXT) },
            ),
        ]);
        f.render_widget(name, line_area);
        f.render_widget(badges.right_aligned(), line_area);
    }

    if total > height {
        let mut state = ScrollbarState::new(total - height).position(offset);
        let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .track_style(fg(BORDER))
            .thumb_style(fg(DIM));
        f.render_stateful_widget(bar, list, &mut state);
    }
}

// ---- arrivals board --------------------------------------------------------

fn draw_board(f: &mut Frame, app: &App, area: Rect) {
    let Some(info) = app.system_info() else {
        return;
    };
    let station = app.station.as_deref().unwrap_or_default();
    let area = column(area);
    let spacious = area.height >= 14;
    let [_, title, _, content] = Layout::vertical([
        Constraint::Length(spacious as u16),
        Constraint::Length(2),
        Constraint::Length(spacious as u16),
        Constraint::Min(0),
    ])
    .areas(area);

    // Freshness: how old the data is, and progress toward the next poll
    let mut right_width = 0;
    if let Some(at) = app.arrivals_at {
        let age = at.elapsed().as_secs();
        let stale = age > REFRESH_SECS * 2 || app.status == ServerStatus::FetchFailed;
        let text = Line::from(Span::styled(
            format!("updated {age}s ago"),
            fg(if stale { RED } else { DIM }),
        ));
        right_width = text.width().max(16);
        f.render_widget(text.right_aligned(), row(title, 0));

        const CELLS: u64 = 16;
        let filled = (age.min(REFRESH_SECS) * CELLS / REFRESH_SECS) as usize;
        let gauge = Line::from(vec![
            Span::styled("━".repeat(filled), fg(if stale { RED } else { ACCENT })),
            Span::styled("━".repeat(CELLS as usize - filled), fg(BORDER)),
        ]);
        f.render_widget(gauge.right_aligned(), row(title, 1));
    }
    let left_room = (title.width as usize).saturating_sub(right_width + 2);
    f.render_widget(
        Line::from(Span::styled(fit(station, left_room), bold(TEXT))),
        row(title, 0),
    );
    f.render_widget(
        Line::from(badge_row(info.sys, app.station_routes(), left_room)),
        row(title, 1),
    );

    // States with nothing to list
    match &app.status {
        ServerStatus::UnknownStation(name) => {
            let detail = format!("“{name}” is not in the schedule · esc to pick another");
            return draw_notice(f, content, "✕", RED, "Unknown station", &detail);
        }
        ServerStatus::NoEndpointsForStation => {
            return draw_notice(
                f,
                content,
                "✕",
                RED,
                "No realtime feed covers this station",
                "esc to pick another",
            );
        }
        _ => {}
    }
    if app.stations.is_none() {
        let title = format!("Loading the {} schedule", info.name);
        return draw_notice(f, content, spinner(app), DIM, &title, "Arrivals follow once it lands");
    }
    if app.awaiting {
        return draw_notice(
            f,
            content,
            spinner(app),
            DIM,
            "Fetching live arrivals",
            "Waiting for the realtime feed",
        );
    }
    if app.arrivals.is_empty() {
        return if app.status == ServerStatus::FetchFailed {
            draw_notice(
                f,
                content,
                "✕",
                RED,
                "Couldn't reach the realtime feed",
                "Retrying every 30 seconds · l shows the log",
            )
        } else {
            draw_notice(
                f,
                content,
                "◌",
                DIM,
                "No upcoming trains",
                "Nothing is scheduled to stop here right now",
            )
        };
    }

    let hero = content.height >= 17 && content.width >= 56;
    if hero && app.arrivals.len() == 1 {
        app.board_max_scroll.set(0);
        let only = Rect {
            height: 9,
            ..content
        };
        return draw_hero(f, info.sys, &app.arrivals, only);
    }
    let [hero_area, list_area] =
        Layout::vertical([Constraint::Length(if hero { 9 } else { 0 }), Constraint::Min(0)])
            .areas(content);
    if hero {
        draw_hero(f, info.sys, &app.arrivals, hero_area);
        draw_arrivals(f, app, info.sys, &app.arrivals[1..], "Following", list_area);
    } else {
        draw_arrivals(f, app, info.sys, &app.arrivals, "Arrivals", list_area);
    }
}

/// 3×5 pixel glyphs for the next-train countdown.
fn glyph(ch: char) -> [&'static str; 5] {
    match ch {
        '0' | 'O' => ["###", "# #", "# #", "# #", "###"],
        '1' => [" # ", "## ", " # ", " # ", "###"],
        '2' => ["###", "  #", "###", "#  ", "###"],
        '3' => ["###", "  #", "###", "  #", "###"],
        '4' => ["# #", "# #", "###", "  #", "  #"],
        '5' => ["###", "#  ", "###", "  #", "###"],
        '6' => ["###", "#  ", "###", "# #", "###"],
        '7' => ["###", "  #", "  #", "  #", "  #"],
        '8' => ["###", "# #", "###", "# #", "###"],
        '9' => ["###", "# #", "###", "  #", "###"],
        'N' => ["#   #", "##  #", "# # #", "#  ##", "#   #"],
        'W' => ["#   #", "#   #", "# # #", "## ##", "#   #"],
        _ => ["   ", "   ", "   ", "   ", "   "],
    }
}

/// Five rows of block text, each pixel two cells wide.
fn big_text(text: &str) -> [String; 5] {
    let mut rows: [String; 5] = Default::default();
    for (i, ch) in text.chars().enumerate() {
        let glyph = glyph(ch);
        for (row, pixels) in rows.iter_mut().zip(glyph) {
            if i > 0 {
                row.push_str("  ");
            }
            for pixel in pixels.chars() {
                row.push_str(if pixel == '#' { "██" } else { "  " });
            }
        }
    }
    rows
}

fn draw_hero(f: &mut Frame, system: Sys, arrivals: &[Arrival], area: Rect) {
    let Some(next) = arrivals.first() else {
        return;
    };
    let color = urgency(next.mins);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(BORDER))
        .title(Span::styled(" Next train ", fg(DIM)))
        .padding(Padding::new(3, 3, 1, 1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let countdown = if next.mins <= 0 {
        "NOW".to_string()
    } else {
        next.mins.min(999).to_string()
    };
    let big = big_text(&countdown);
    let big_width = Line::raw(big[0].as_str()).width() as u16;
    let unit_width: u16 = if next.mins <= 0 { 0 } else { 4 };
    for (i, line) in big.iter().enumerate() {
        f.render_widget(
            Line::from(Span::styled(line.clone(), fg(color))),
            row(inner, i as u16),
        );
    }
    if unit_width > 0 {
        let unit_area = Rect {
            x: inner.x + big_width + 1,
            width: inner.width.saturating_sub(big_width + 1),
            ..row(inner, 4)
        };
        f.render_widget(Line::from(Span::styled("min", fg(DIM))), unit_area);
    }

    // Right of the countdown: where it's going and what follows it
    let text_x = big_width + unit_width + 5;
    let text = Rect {
        x: inner.x + text_x.min(inner.width),
        width: inner.width.saturating_sub(text_x),
        ..inner
    };
    let room = text.width as usize;
    let mut heading = badge(system, &next.route, true);
    let used = Line::from(heading.clone()).width() + 2;
    heading.push(Span::raw("  "));
    heading.push(Span::styled(
        fit(&next.headsign, room.saturating_sub(used)),
        bold(TEXT),
    ));
    f.render_widget(Line::from(heading), row(text, 1));

    // "on time" only when the feed says so; many feeds report no delay at all
    let detail = match (delay_text(next.delay), next.delay) {
        (Some((delay, delay_color)), _) => Some(Span::styled(delay, fg(delay_color))),
        (None, Some(_)) => Some(Span::styled("on time", fg(FAINT))),
        (None, None) => None,
    };
    if let Some(detail) = detail {
        f.render_widget(Line::from(detail), row(text, 2));
    }

    let same_train: Vec<String> = arrivals[1..]
        .iter()
        .filter(|a| a.route == next.route && a.headsign == next.headsign)
        .take(3)
        .map(|a| a.mins.to_string())
        .collect();
    if !same_train.is_empty() {
        let then = format!("then {} min", same_train.join(" · "));
        f.render_widget(
            Line::from(Span::styled(fit(&then, room), fg(DIM))),
            row(text, 4),
        );
    }
}

fn draw_arrivals(
    f: &mut Frame,
    app: &App,
    system: Sys,
    arrivals: &[Arrival],
    title: &str,
    area: Rect,
) {
    if area.height < 4 {
        return;
    }
    let mut block = rounded(title).padding(Padding::horizontal(1));
    let inner = block.inner(area);
    let avail = (inner.height as usize).saturating_sub(1);

    // Blank lines between rows when everything fits that way
    let spacing = if arrivals.len() * 2 <= avail + 1 { 2 } else { 1 };
    let visible = if spacing == 2 {
        arrivals.len()
    } else {
        avail.min(arrivals.len())
    };
    let max_scroll = arrivals.len() - visible;
    app.board_max_scroll.set(max_scroll);
    let scroll = app.board_scroll.min(max_scroll);
    let below = arrivals.len() - visible - scroll;
    if below > 0 {
        block = block.title_bottom(
            Line::from(Span::styled(format!(" ↓ {below} more "), fg(DIM))).right_aligned(),
        );
    }
    if scroll > 0 {
        block = block.title_top(
            Line::from(Span::styled(format!(" ↑ {scroll} more "), fg(DIM))).right_aligned(),
        );
    }
    f.render_widget(block, area);
    if inner.width < 20 || arrivals.is_empty() {
        return;
    }

    let badge_col = arrivals
        .iter()
        .map(|a| badge_width(system, &a.route, true))
        .max()
        .unwrap_or(5)
        + 2;
    let width = inner.width as usize;
    let show_delay = width >= 60;

    let header = Line::from(vec![
        Span::styled(format!("{:<badge_col$}", "Line"), fg(FAINT)),
        Span::styled("Destination", fg(FAINT)),
    ]);
    f.render_widget(header, row(inner, 0));
    f.render_widget(
        Line::from(Span::styled("Arrives", fg(FAINT))).right_aligned(),
        row(inner, 0),
    );

    for (i, arrival) in arrivals[scroll..].iter().take(visible).enumerate() {
        let line_area = row(inner, (1 + i * spacing) as u16);
        if line_area.height == 0 {
            break;
        }
        let color = urgency(arrival.mins);
        let eta = if arrival.mins <= 0 {
            Span::styled("now", bold(color))
        } else {
            Span::styled(format!("{} min", arrival.mins), bold(color))
        };
        let mut right = Vec::new();
        if show_delay && let Some((delay, delay_color)) = delay_text(arrival.delay) {
            right.push(Span::styled(delay, fg(delay_color)));
        }
        // Pad the ETA to a fixed column so the delay notes line up
        right.push(Span::raw(" ".repeat(9usize.saturating_sub(eta.width()))));
        right.push(eta);
        let right = Line::from(right);

        let mut left = badge(system, &arrival.route, true);
        let pad = badge_col.saturating_sub(Line::from(left.clone()).width());
        left.push(Span::raw(" ".repeat(pad)));
        let room = width.saturating_sub(badge_col + right.width() + 1);
        left.push(Span::styled(fit(&arrival.headsign, room), fg(TEXT)));

        f.render_widget(Line::from(left), line_area);
        f.render_widget(right.right_aligned(), line_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::fixtures::{arrival, with_stations};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::time::{Duration, Instant};

    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        println!("--- {width}x{height} ---\n{out}");
        out
    }

    fn board() -> App {
        let mut app = with_stations();
        app.choose_station("Delancey St-Essex St".to_string());
        app.status = ServerStatus::Ready;
        app.awaiting = false;
        app.arrivals_at = Some(Instant::now() - Duration::from_secs(12));
        app.arrivals = vec![
            arrival("F", "Coney Island-Stillwell Av", 0, None),
            arrival("M", "Middle Village-Metropolitan Av", 3, Some(120)),
            arrival("J", "Jamaica Center-Parsons/Archer", 4, Some(-90)),
            arrival("F", "Coney Island-Stillwell Av", 7, Some(30)),
            arrival("F", "Jamaica-179 St", 9, None),
            arrival("Z", "Broad St", 12, None),
            arrival("M", "Forest Hills-71 Av", 14, None),
            arrival("F", "Coney Island-Stillwell Av", 18, None),
        ];
        app
    }

    const SIZES: [(u16, u16); 4] = [(80, 24), (40, 12), (140, 45), (34, 9)];

    #[test]
    fn systems_screen_lists_every_system() {
        for (w, h) in SIZES {
            let out = render(&App::default(), w, h);
            for info in &SYSTEMS {
                assert!(out.contains(info.name), "{} missing at {w}x{h}", info.name);
            }
        }
    }

    #[test]
    fn stations_screen_shows_loading_until_the_schedule_lands() {
        let mut app = App::default();
        app.choose_system(0);
        for (w, h) in SIZES {
            assert!(render(&app, w, h).contains("Loading the NYC Subway"));
        }
    }

    #[test]
    fn stations_screen_lists_and_filters() {
        let mut app = with_stations();
        for (w, h) in SIZES {
            let out = render(&app, w, h);
            assert!(out.contains("14 St-Union Sq"), "at {w}x{h}");
            assert!(out.contains("6 of 6"), "at {w}x{h}");
        }
        app.filter = "zzz".to_string();
        app.filtered.clear();
        assert!(render(&app, 80, 24).contains("No stations"));
    }

    #[test]
    fn board_shows_arrivals_at_every_size() {
        let app = board();
        for (w, h) in SIZES {
            let out = render(&app, w, h);
            assert!(out.contains("Delancey"), "at {w}x{h}");
            assert!(out.contains("3 min"), "at {w}x{h}");
        }
        let out = render(&app, 100, 40);
        assert!(out.contains("Next train"));
        assert!(out.contains("2 min late"));
        assert!(out.contains("updated 12s ago"));
    }

    #[test]
    fn board_states_without_arrivals() {
        let mut app = board();
        app.arrivals.clear();
        assert!(render(&app, 80, 24).contains("No upcoming trains"));

        app.status = ServerStatus::FetchFailed;
        assert!(render(&app, 80, 24).contains("Couldn't reach"));

        app.awaiting = true;
        app.status = ServerStatus::Ready;
        assert!(render(&app, 80, 24).contains("Fetching live arrivals"));

        app.status = ServerStatus::UnknownStation("Nowhere".to_string());
        assert!(render(&app, 80, 24).contains("Unknown station"));
    }

    #[test]
    fn single_arrival_shows_only_the_hero() {
        let mut app = board();
        app.arrivals.truncate(1);
        let out = render(&app, 100, 40);
        assert!(out.contains("Next train"));
        assert!(!out.contains("Following"));
        assert!(render(&app, 40, 12).contains("now"));
    }

    #[test]
    fn board_waits_for_the_schedule_before_the_feed() {
        let mut app = board();
        app.stations = None;
        app.awaiting = true;
        assert!(render(&app, 80, 24).contains("Loading the NYC Subway schedule"));
    }

    #[test]
    fn overlays_render() {
        let mut app = board();
        app.show_help = true;
        app.show_logs = true;
        for (w, h) in SIZES {
            render(&app, w, h);
        }
        assert!(render(&app, 80, 24).contains("Server log"));
    }

    #[test]
    fn indexed_colour_fallback_stays_in_range() {
        set_truecolor(false);
        // Dark neutrals must land on the grey ramp, not a tinted cube entry
        assert_eq!(c(BG), Color::Indexed(233));
        assert_eq!(c(PANEL), Color::Indexed(234));
        assert_eq!(c(0xffffff), Color::Indexed(231));
        assert_eq!(c(0xee352e), Color::Indexed(202));
        assert_eq!(c(0x0039a6), Color::Indexed(25));
        set_truecolor(true);
    }
}
