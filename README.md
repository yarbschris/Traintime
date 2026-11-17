# traintime

A small, self-contained GTFS server that drives a live transit arrival sign.

traintime pulls a transit agency's GTFS static schedule and GTFS-Realtime
feeds, works out which trains are coming to one station, and produces a short,
sorted list of upcoming arrivals for a display client. It is built to run
unattended on a Raspberry Pi Zero 2 W and drive a HUB75 RGB LED matrix you can
read from across the room.

> **Status:** early development. The server pipeline and a terminal client
> work today. Server features are frozen while the display side is built:
> a wire protocol, a serial link and a Raspberry Pi Pico display client with
> hand-written display drivers.

## Goals

- **Runs on small hardware.** The target is a Pi Zero 2 W (512 MB RAM). Memory
  use matters more than throughput.
- **Runs unattended.** Network failures and bad feed data are logged and
  retried, not fatal. The library forbids `unwrap`, `expect` and `panic!`.
- **Shows one station, well.** traintime serves arrivals to a sign, not to the
  network. Plenty of existing transit servers already do that.

## Supported systems

| System          | Identifier      |
| --------------- | --------------- |
| NYC Subway      | `NycSubway`     |
| MTA Metro-North | `MtaMetroNorth` |
| MBTA (Boston)   | `BostonTransit` |

Each system is described by a YAML file in
[`traintime_server/system_configs/`](traintime_server/system_configs). The file
gives the static feed URL and the realtime feed URLs, with the routes each
realtime feed covers. Adding a system starts with adding one of these files.

## Repository layout

This is a Cargo workspace with two crates:

| Crate              | What it is                                                                                               |
| ------------------ | -------------------------------------------------------------------------------------------------------- |
| `traintime_server` | The library: static and realtime data pipelines, station resolution, and the channels clients listen on. |
| `traintime_cli`    | An interactive terminal UI for picking a station and watching its arrivals board.                        |

## Getting started

### Prerequisites

- Rust, with 2024 edition support (1.85 or later)
- `protoc`, the Protocol Buffers compiler, to build the GTFS-Realtime bindings
  - macOS: `brew install protobuf`
  - Debian/Ubuntu: `sudo apt-get install protobuf-compiler`

### Build and test

```sh
cargo build --release
cargo test
```

### Run the CLI

`traintime_cli` is an interactive terminal UI. Start it with no arguments to
pick a system, then filter and pick a station, then watch its arrivals board
update:

```sh
cargo run -p traintime_cli
```

Choosing a system downloads its static schedule, so the station list takes a
moment to appear the first time. Press `?` for key bindings, `l` to show the
server's log, and `q` to quit.

To skip straight to a station's board:

```sh
cargo run -p traintime_cli -- --system NycSubway --station "East Broadway"
```

| Flag                      | Description                                   |
| ------------------------- | --------------------------------------------- |
| `-t`, `--system SYSTEM`   | Start on a system                             |
| `-s`, `--station STATION` | Start on a station's board (needs `--system`) |
| `-h`, `--help`            | Show help                                     |

## How it works

`traintime_server::start` spawns background tasks onto the caller's tokio
runtime and hands back a set of `tokio::sync::watch` channels:

```text
 selection ──▶ resolve system config ──▶ fetch + build static data
    │                                          │
    └────────────▶ resolve station ◀───────────┘
                         │
                         ▼
             poll GTFS-Realtime feeds ──▶ arrivals ──▶ client
                         │
                         └──────────────▶ status   ──▶ client
```

- **Selection in.** A client sends a `Selection` (a system, optionally a
  station). The selection is the only input; everything else is derived from
  it. Changing the system triggers a full static rebuild. Changing the station
  only re-resolves against the data already in memory.
- **Arrivals out.** On each tick, the server decodes the realtime feeds that
  serve the station's routes. It drops canceled trips, skipped stops, trains
  that have already left, and trains that terminate at the station. What is
  left is a list of arrivals: route, headsign, minutes away and delay.
- **Status out.** A separate channel reports what the server is doing
  (waiting for a selection, loading static data, resolving the station, ready,
  or a failure such as an unknown station or a failed fetch), so the
  sign can say something useful when there are no arrivals to show.

Stations are grouped the way a rider thinks of them. Platforms of one station
are merged, and same-name stations linked in `transfers.txt` become one entry,
so a station served by several lines shows all of its trains.

## Hardware

The end goal is a **Raspberry Pi Zero 2 W** driving a **64×32 HUB75 RGB LED
matrix**, running unattended. The panel is driven either from the Pi directly
or by a microcontroller over UART.

While the Pi Zero 2 W is out of stock, the server runs on a laptop or home
server. It sends frames over a UART link to a **Raspberry Pi Pico**, which
draws them. The serial device path is configured at runtime, so the same
server works unchanged when it moves to the Pi and the link becomes the Pi's
GPIO UART.

The Pico's display drivers are written from scratch: first an SPI display,
then the HUB75 panel driven through the RP2350's PIO blocks and DMA.

## Roadmap

Now, for the Pico display:

- [ ] Shared `no_std` protocol crate: message types, COBS framing, CRC,
      version byte
- [ ] Display layout as a host-tested `no_std` crate
- [ ] Serial sink in the server, and a host tool that plays the Pico
- [ ] Pico firmware: UART receive, decode, layout
- [ ] SPI display driver
- [ ] HUB75 driver (PIO + DMA), on the wall

Later, toward the Pi Zero 2 W:

- [ ] Select stations by stable ID instead of display name
- [ ] Persist the last selection so the sign recovers after a power cut
- [ ] Memory profiling and tuning for 512 MB
- [ ] Cross-compiling and deploying to the Pi
- [ ] A settings page on the home network, for changing the station from a phone

## License

traintime is licensed under the GNU General Public License v3.0. See
[LICENSE.md](LICENSE.md).
