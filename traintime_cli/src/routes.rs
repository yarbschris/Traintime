//! Route badges: a short label and the line's colour, per transit system.
//!
//! The server library does not expose `routes.txt` colours, so the well-known
//! line colours are tabled here, keyed by (system, route id). Unknown routes
//! get a stable colour derived from their id.

use traintime_server::config::SupportedTransitSystem as Sys;

pub struct Badge {
    pub label: String,
    pub color: u32,
}

impl Badge {
    /// Whether dark text reads better than white on this badge's colour.
    pub fn dark_text(&self) -> bool {
        let r = ((self.color >> 16) & 0xff) as f32;
        let g = ((self.color >> 8) & 0xff) as f32;
        let b = (self.color & 0xff) as f32;
        0.299 * r + 0.587 * g + 0.114 * b > 150.0
    }
}

const FALLBACK: [u32; 8] = [
    0x3b82f6, 0x8b5cf6, 0xec4899, 0x14b8a6, 0xf97316, 0x84cc16, 0x06b6d4, 0xe11d48,
];

fn fallback_color(route: &str) -> u32 {
    let hash = route
        .bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    FALLBACK[hash as usize % FALLBACK.len()]
}

fn clip(label: &str) -> String {
    label.chars().take(12).collect()
}

pub fn badge(system: Sys, route: &str) -> Badge {
    let (label, color) = match system {
        Sys::NycSubway => nyc(route),
        Sys::BostonTransit => boston(route),
        Sys::MtaMetroNorth => metro_north(route),
    };
    Badge {
        label: clip(&label),
        color,
    }
}

fn nyc(route: &str) -> (String, u32) {
    let color = match route {
        "1" | "2" | "3" => 0xee352e,
        "4" | "5" | "6" | "6X" => 0x00933c,
        "7" | "7X" => 0xb933ad,
        "A" | "C" | "E" => 0x0039a6,
        "B" | "D" | "F" | "FX" | "M" => 0xff6319,
        "G" => 0x6cbe45,
        "J" | "Z" => 0x996633,
        "L" => 0xa7a9ac,
        "N" | "Q" | "R" | "W" => 0xfccc0a,
        "GS" | "FS" | "H" | "S" => 0x808183,
        "SI" | "SIR" => 0x0039a6,
        other => fallback_color(other),
    };
    let label = match route {
        "GS" | "FS" | "H" => "S",
        "SI" => "SIR",
        other => other,
    };
    (label.to_string(), color)
}

fn boston(route: &str) -> (String, u32) {
    if let Some(branch) = route.strip_prefix("Green-") {
        return (format!("GL·{branch}"), 0x00843d);
    }
    if let Some(line) = route.strip_prefix("CR-") {
        return (line.to_string(), 0x80276c);
    }
    if let Some(boat) = route.strip_prefix("Boat-") {
        return (boat.to_string(), 0x008eaa);
    }
    if let Some(shuttle) = route.strip_prefix("Shuttle-") {
        return (shuttle.to_string(), 0x7c878e);
    }
    match route {
        "Red" => ("RL".to_string(), 0xda291c),
        "Mattapan" => ("M".to_string(), 0xda291c),
        "Orange" => ("OL".to_string(), 0xed8b00),
        "Blue" => ("BL".to_string(), 0x003da5),
        // Silver Line routes are numbered 741-751 in the feed
        "741" | "742" | "743" | "746" | "749" | "751" => {
            let name = match route {
                "741" => "SL1",
                "742" => "SL2",
                "743" => "SL3",
                "746" => "SLW",
                "749" => "SL5",
                _ => "SL4",
            };
            (name.to_string(), 0x7c878e)
        }
        // Everything else is a bus
        other => (other.to_string(), 0xffc72c),
    }
}

fn metro_north(route: &str) -> (String, u32) {
    let (label, color) = match route {
        "1" => ("Hudson", 0x009b3a),
        "2" => ("Harlem", 0x0039a6),
        "3" => ("New Haven", 0xee0034),
        "4" => ("New Canaan", 0xee0034),
        "5" => ("Danbury", 0xee0034),
        "6" => ("Waterbury", 0xee0034),
        other => (other, fallback_color(other)),
    };
    (label.to_string(), color)
}

/// Sort key that orders numbered routes numerically and before lettered ones.
pub fn sort_key(route: &str) -> (u8, u32, String) {
    match route.parse::<u32>() {
        Ok(n) => (0, n, String::new()),
        Err(_) => (1, 0, route.to_string()),
    }
}
