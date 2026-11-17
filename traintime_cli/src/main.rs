//! A terminal UI for traintime: pick a system, pick a station, watch the
//! arrivals board update.

mod app;
mod logger;
mod routes;
mod ui;

use std::process::ExitCode;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use tokio::sync::mpsc;
use traintime_server::config::Selection;
use traintime_server::types::gtfs::StationName;

use app::{App, Cmd, SYSTEMS};

const HELP: &str = "\
traintime: live transit arrivals in your terminal

USAGE:
  traintime_cli [OPTIONS]

OPTIONS:
  -t, --system SYSTEM     Start on a system: NycSubway, MtaMetroNorth or BostonTransit
  -s, --station STATION   Start on a station's arrivals board (needs --system)
  -h, --help              Show this help

With no options, pick the system and station interactively.
";

struct Args {
    system: Option<usize>,
    station: Option<String>,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut args = Args {
        system: None,
        station: None,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "-t" | "--system" => {
                let value = argv.next().ok_or("--system needs a value")?;
                let index = SYSTEMS
                    .iter()
                    .position(|s| s.id.eq_ignore_ascii_case(&value))
                    .ok_or_else(|| {
                        let valid: Vec<&str> = SYSTEMS.iter().map(|s| s.id).collect();
                        format!("unknown system '{value}' (valid: {})", valid.join(", "))
                    })?;
                args.system = Some(index);
            }
            "-s" | "--station" => {
                args.station = Some(argv.next().ok_or("--station needs a value")?);
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    if args.station.is_some() && args.system.is_none() {
        return Err("--station needs --system".to_string());
    }
    Ok(Some(args))
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{HELP}");
            return ExitCode::FAILURE;
        }
    };

    let truecolor = std::env::var("COLORTERM")
        .is_ok_and(|v| v.contains("truecolor") || v.contains("24bit"));
    ui::set_truecolor(truecolor);
    logger::init();

    let channels = match traintime_server::start(None).await {
        Ok(channels) => channels,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, args, channels).await;
    ratatui::restore();

    if let Err(e) = result {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    // Exit directly rather than waiting on the server's in-flight requests
    std::process::exit(0);
}

type Channels = (
    tokio::sync::watch::Receiver<std::sync::Arc<[traintime_server::types::traintime_packet::TraintimePacket]>>,
    tokio::sync::watch::Sender<Option<Selection>>,
    tokio::sync::watch::Receiver<traintime_server::types::status::ServerStatus>,
    tokio::sync::watch::Receiver<Option<traintime_server::types::static_data::StaticData>>,
);

async fn run(
    terminal: &mut DefaultTerminal,
    args: Args,
    (mut rx_packets, tx_selection, mut rx_status, mut rx_static): Channels,
) -> std::io::Result<()> {
    let send = |cmd: Cmd| -> bool {
        match cmd {
            Cmd::Quit => true,
            Cmd::Select { system, station } => {
                if let Some(info) = SYSTEMS.get(system) {
                    tx_selection.send_replace(Some(Selection {
                        transit_system: info.sys,
                        station_name: station.map(StationName),
                    }));
                }
                false
            }
        }
    };

    let mut app = App::default();
    app.on_status(rx_status.borrow_and_update().clone());
    if let Some(system) = args.system {
        let cmd = app.choose_system(system);
        let cmd = match args.station {
            Some(station) => app.choose_station(station),
            None => cmd,
        };
        if let Some(cmd) = cmd {
            send(cmd);
        }
    }

    // Terminal events are read on a plain thread: a blocking read on the
    // runtime would keep it from shutting down
    let (tx_events, mut rx_events) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if tx_events.send(ev).is_err() {
                break;
            }
        }
    });

    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let (mut packets_open, mut status_open, mut static_open) = (true, true, true);

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        tokio::select! {
            ev = rx_events.recv() => match ev {
                Some(Event::Key(key)) if key.kind != KeyEventKind::Release => {
                    if let Some(cmd) = app.on_key(key)
                        && send(cmd)
                    {
                        return Ok(());
                    }
                }
                Some(_) => {} // resize and friends: the redraw handles them
                None => return Ok(()),
            },
            r = rx_status.changed(), if status_open => match r {
                Ok(()) => app.on_status(rx_status.borrow_and_update().clone()),
                Err(_) => status_open = false,
            },
            r = rx_static.changed(), if static_open => match r {
                Ok(()) => app.on_static(rx_static.borrow_and_update().as_ref()),
                Err(_) => static_open = false,
            },
            r = rx_packets.changed(), if packets_open => match r {
                Ok(()) => {
                    // The status for a fetch is published just before its packets
                    app.on_status(rx_status.borrow().clone());
                    let packets = rx_packets.borrow_and_update().clone();
                    app.on_packets(&packets);
                }
                Err(_) => packets_open = false,
            },
            _ = tick.tick() => app.tick = app.tick.wrapping_add(1),
        }
    }
}
