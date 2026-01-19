# RBN VFD Display - Rust/Linux Port Design

## Overview

Port the RBN VFD Display application from Windows/WPF/C# to Linux/Rust/egui. The application displays amateur radio spots from the Reverse Beacon Network on an ELO 20x2 VFD customer-facing display.

## Project Structure

```
rbn-vfd-linux/
├── Cargo.toml
├── src/
│   ├── main.rs              # Entry point, eframe setup
│   ├── app.rs               # Main App struct, egui UI logic
│   ├── models/
│   │   ├── mod.rs
│   │   └── spot.rs          # RawSpot, AggregatedSpot
│   ├── services/
│   │   ├── mod.rs
│   │   ├── rbn_client.rs    # Telnet connection to RBN
│   │   ├── spot_store.rs    # Thread-safe spot storage
│   │   └── vfd_display.rs   # Serial port VFD output
│   └── config.rs            # Settings load/save (.ini)
```

## Dependencies

- `eframe` - egui wrapper for desktop apps
- `tokio` - async runtime for telnet
- `serialport` - cross-platform serial port access
- `configparser` - .ini file handling
- `directories` - XDG paths on Linux
- `rand` - random character display

## Data Models

### RawSpot

Incoming telnet data:

```rust
struct RawSpot {
    spotter_callsign: String,
    spotted_callsign: String,
    frequency_khz: f64,
    snr: i32,
    speed_wpm: i32,
    mode: String,
    timestamp: Instant,
}
```

### AggregatedSpot

Stored/displayed data with incremental averaging:

```rust
struct AggregatedSpot {
    callsign: String,
    frequency_khz: f64,        // Running average (incremental)
    center_frequency_khz: f64, // Rounded to nearest kHz (grouping key)
    highest_snr: i32,
    average_speed: f64,        // Running average (incremental)
    spot_count: u32,
    last_spotted: Instant,
}
```

### Aggregation Logic

- Key = `"{callsign}|{center_freq}"` where center_freq = round(frequency)
- Spots within 1 kHz of each other aggregate together
- Keep highest SNR
- Use incremental averaging for speed and frequency:
  ```rust
  spot_count += 1;
  average_speed += (new_speed - average_speed) / spot_count as f64;
  frequency_khz += (new_freq - frequency_khz) / spot_count as f64;
  ```
- Update `last_spotted` on each new spot

## Services Architecture

### Communication Pattern

```
[tokio task: RbnClient] --mpsc--> [main thread: App]
                                       |
                                       v
                                  [SpotStore]
                                       |
                                       v
                                  [VfdDisplay]
```

### RbnClient

Async telnet connection:

- Connects to `rbn.telegraphy.de:7000`
- Sends callsign when prompted ("Please enter your call")
- Parses spot lines with regex: `DX de (\S+):\s+(\d+\.?\d*)\s+(\S+)\s+(\w+)\s+(\d+)\s+dB\s+(\d+)\s+WPM`
- Runs in tokio task, sends parsed spots via `mpsc` channel to main thread
- Supports connect/disconnect commands via another channel

### SpotStore

Thread-safe storage:

- `Arc<Mutex<HashMap<String, AggregatedSpot>>>`
- Filters incoming spots by minimum SNR
- Purges spots older than max age (checked on timer or when adding)
- Provides sorted spot lists (by frequency or recency)

### VfdDisplay

Serial output:

- Opens port with 9600/8/N/1 settings
- Writes ESC/POS commands for cursor positioning
- Scroll timer advances through spots when count > 2
- Random character mode when no spots (20% duty cycle)

## User Interface

```
┌─────────────────────────────────────────────────────┐
│  RBN VFD Display                                    │
├─────────────────────────────────────────────────────┤
│  Callsign: [________] [Connect] [Disconnect]        │
│  Serial Port: [/dev/ttyUSB0 ▼] [Open] [Close]       │
│  Status: Connected as W6JSV | VFD: /dev/ttyUSB0     │
├─────────────────────────────────────────────────────┤
│  Filters:                                           │
│  Min SNR:    [━━━━━●━━━━━━━━] 15 dB  (0-50 slider)  │
│  Max Age:    ○5   ○10  ○15  ○30 min                 │
│  Scroll:     ○1   ○3   ○5   ○10  ○30 sec            │
├─────────────────────────────────────────────────────┤
│  ☐ Force random character mode (testing)            │
│                                    [Restore Defaults]│
├─────────────────────────────────────────────────────┤
│  VFD Preview:                                       │
│  ┌────────────────────┐                             │
│  │14033.0 WO6W 24     │  <- green on black          │
│  │7025.3 K3LR 32      │                             │
│  └────────────────────┘                             │
├─────────────────────────────────────────────────────┤
│  Active Spots (12):                                 │
│  ┌─────────────────────────────────────────────┐    │
│  │ 14033.0  WO6W    24 dB  28 WPM  (3 spots)   │    │
│  │ 7025.3   K3LR    32 dB  25 WPM  (1 spot)    │    │
│  │ ...scrollable list...                       │    │
│  └─────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────┘
```

### UI Elements

- Callsign input with connect/disconnect buttons
- Serial port dropdown populated from system, with open/close
- Min SNR slider (0-50 dB)
- Radio buttons for max age (5, 10, 15, 30 minutes)
- Radio buttons for scroll interval (1, 3, 5, 10, 30 seconds)
- Checkbox to force random character mode for testing
- Restore Defaults button
- VFD preview panel (monospace, green-on-black style, 20 chars wide)
- Scrollable list showing all active spots with details

## Configuration Persistence

### File Location

`~/.config/rbn-vfd-display/settings.ini`

### File Format

```ini
[connection]
callsign =

[display]
serial_port = /dev/ttyUSB0

[filters]
min_snr = 10
max_age_minutes = 10
scroll_interval_seconds = 3
```

### Defaults

- Callsign: empty (required before connecting)
- Min SNR: `10`
- Max age: `10` minutes
- Scroll interval: `3` seconds
- Serial port: empty (must be selected)

### Behavior

- Load on startup; use defaults if file doesn't exist
- Save automatically on application exit
- "Restore Defaults" button resets all settings to defaults
- App won't connect to RBN until callsign is entered
- Create config directory if it doesn't exist

## Random Character Display

When no active spots (or when "Force random character mode" is checked):

- Display a single random printable ASCII character (A-Z, 0-9)
- Position randomly on the 20x2 grid (column 0-19, row 0-1)
- Update at the user-selected scroll interval
- 20% duty cycle: character visible ~20% of the time, blank ~80%

```rust
struct RandomCharState {
    next_update: Instant,
    showing_char: bool,  // true = show char, false = blank
}

fn update_random_display(&mut self) {
    if now >= self.next_update {
        self.showing_char = rand::random::<f32>() < 0.2;  // 20% chance
        if self.showing_char {
            let ch = random_alphanumeric();
            let col = rand::gen_range(0..20);
            let row = rand::gen_range(0..2);
            write_char_at(row, col, ch);
        } else {
            clear_display();
        }
        self.next_update = now + scroll_interval;
    }
}
```

## Error Handling

### RBN Connection

- Connection failures show error in status bar, allow retry
- Disconnections detected and reported; user can reconnect
- Invalid callsign format: prevent connect, show validation message

### Serial Port

- Port open failures show error, allow selecting different port
- Write errors logged, don't crash the app
- Port disappearing (USB unplug): detect, update status, allow reopening

### Configuration

- Corrupt/unreadable .ini: log warning, use defaults
- Missing config directory: create it automatically
- Write failures on exit: log warning, don't block exit

### UI Responsiveness

- Telnet runs in background tokio task, never blocks UI
- Serial writes are quick but wrapped in non-blocking handling
- Long spot lists use virtual scrolling if needed

## Key Specifications

| Item | Value |
|------|-------|
| Telnet server | rbn.telegraphy.de:7000 |
| VFD dimensions | 20x2 characters |
| Serial settings | 9600/8/N/1 |
| Config path | ~/.config/rbn-vfd-display/settings.ini |
| Default min SNR | 10 dB |
| Default max age | 10 minutes |
| Default scroll | 3 seconds |
