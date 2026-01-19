# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RBN VFD Display (Linux) - A Rust/egui application that displays amateur radio spots from the Reverse Beacon Network on an ELO 20x2 VFD customer-facing display. This is the Linux port of the Windows WPF application in the parent directory.

## Build Commands

```bash
cargo build --release
cargo run --release
cargo clippy              # Lint
```

## Architecture

```
RBN Telnet Server → RbnClient → SpotStore → RbnVfdApp (egui) → VfdDisplay → ELO VFD
     (tokio)         (mpsc)                                      (serial)
```

**Data Flow:**
- `RbnClient` runs tokio async in a separate thread, communicates via mpsc channels
- `SpotStore` aggregates spots by callsign + center frequency (1 kHz threshold)
- `VfdDisplay` writes to serial port (9600/8/N/1), no ANSI escape sequences

**Models** (`src/models/spot.rs`):
- `RawSpot`: Incoming RBN telnet data (spotter, spotted callsign, frequency, SNR, speed, mode)
- `AggregatedSpot`: Grouped by callsign + frequency, tracks highest SNR, uses incremental averaging for speed/frequency

**Services** (`src/services/`):
- `rbn_client.rs`: Async telnet to rbn.telegraphy.de:7000, regex parsing, handles prompts without trailing newlines
- `spot_store.rs`: Thread-safe storage with purging, SNR filtering
- `vfd_display.rs`: Serial output with time-based random character duty cycle for idle mode

**Config** (`src/config.rs`):
- XDG paths via `directories` crate: `~/.config/rbn-vfd-display/settings.ini`
- Persists: callsign, serial port, min SNR, max age, scroll interval, random duty cycle

## Display Format

20 characters per line: `"{freq:7.1} {wpm:2} {call:<9}"`

Example:
```
14033.0 22 WO6W
 3500.0 18 K6ABC
```

Frequency aligned at decimal point, WPM right-aligned, callsign left-aligned.

## Key Patterns

- **Thread isolation**: Tokio runtime in dedicated thread, `blocking_send` for commands from UI
- **Incremental averaging**: `new_avg = old_avg + (new_value - old_avg) / count`
- **Duty cycle**: Random mode shows character for first N% of each second (not random chance)
- **VFD protocol**: Simple clear (0x0C) + write 40 chars, no escape sequences

## Dependencies

- eframe/egui for GUI
- tokio for async telnet
- serialport for VFD communication
- configparser + directories for XDG config
