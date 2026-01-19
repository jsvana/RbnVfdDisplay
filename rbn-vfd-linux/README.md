# RBN VFD Display (Linux)

A Linux application that displays amateur radio spots from the Reverse Beacon Network on an ELO 20x2 VFD customer-facing display.

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run --release
```

## Configuration

Settings are stored in `~/.config/rbn-vfd-display/settings.ini` and are automatically saved on exit.

## Features

- Connects to RBN telnet server (rbn.telegraphy.de:7000)
- Aggregates spots by callsign and frequency (within 1 kHz)
- Displays spots on ELO 20x2 VFD via serial port
- Configurable SNR filter, max age, and scroll interval
- Random character display mode when idle
- Settings persist between sessions

## License

MIT
