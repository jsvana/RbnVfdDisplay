# Radio Control Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add radio control to tune to selected RBN spots via OmniRig (Windows) or rigctld (macOS/Linux).

**Architecture:** A `RadioController` trait abstracts platform-specific backends. The UI adds clickable spot rows, a Tune button with connection indicator, and a settings dialog. Mode mapping converts RBN modes to radio commands.

**Tech Stack:** Rust, egui, std::net::TcpStream (rigctld), windows crate (OmniRig COM on Windows)

---

## Task 1: Add RadioController Trait and Types

**Files:**
- Create: `src/services/radio/mod.rs`

**Step 1: Create the radio module with trait and types**

```rust
//! Radio controller abstraction for CAT control

mod noop;
mod rigctld;

#[cfg(target_os = "windows")]
mod omnirig;

pub use noop::NoOpController;
pub use rigctld::RigctldController;

#[cfg(target_os = "windows")]
pub use omnirig::OmniRigController;

/// Radio operating mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioMode {
    Cw,
    Usb,
    Lsb,
    Rtty,
    Am,
    Fm,
}

impl RadioMode {
    /// Convert RBN mode string to RadioMode
    pub fn from_rbn_mode(mode: &str) -> Self {
        match mode.to_uppercase().as_str() {
            "CW" => RadioMode::Cw,
            "RTTY" => RadioMode::Rtty,
            "FT8" | "FT4" | "PSK31" | "PSK63" | "JT65" | "JT9" | "WSPR" => RadioMode::Usb,
            "SSB" => RadioMode::Usb, // Default to USB for SSB
            _ => RadioMode::Cw,      // Default to CW for unknown modes
        }
    }

    /// Convert to rigctld mode string
    pub fn to_rigctld_mode(&self) -> &'static str {
        match self {
            RadioMode::Cw => "CW",
            RadioMode::Usb => "USB",
            RadioMode::Lsb => "LSB",
            RadioMode::Rtty => "RTTY",
            RadioMode::Am => "AM",
            RadioMode::Fm => "FM",
        }
    }
}

/// Result type for radio operations
pub type RadioResult<T> = Result<T, RadioError>;

/// Radio controller errors
#[derive(Debug, Clone)]
pub enum RadioError {
    NotConnected,
    ConnectionFailed(String),
    CommandFailed(String),
    Timeout,
    NotConfigured,
}

impl std::fmt::Display for RadioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RadioError::NotConnected => write!(f, "Radio not connected"),
            RadioError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            RadioError::CommandFailed(msg) => write!(f, "Command failed: {}", msg),
            RadioError::Timeout => write!(f, "Radio not responding"),
            RadioError::NotConfigured => write!(f, "Radio not configured"),
        }
    }
}

impl std::error::Error for RadioError {}

/// Trait for radio controllers
pub trait RadioController: Send {
    /// Check if connected to the radio
    fn is_connected(&self) -> bool;

    /// Attempt to connect to the radio
    fn connect(&mut self) -> RadioResult<()>;

    /// Disconnect from the radio
    fn disconnect(&mut self);

    /// Tune to a frequency (in kHz) and mode
    fn tune(&mut self, frequency_khz: f64, mode: RadioMode) -> RadioResult<()>;

    /// Get a description of the backend
    fn backend_name(&self) -> &'static str;
}

/// Factory function to create the appropriate controller
#[cfg(target_os = "windows")]
pub fn create_controller(config: &crate::config::RadioConfig) -> Box<dyn RadioController> {
    if !config.enabled {
        return Box::new(NoOpController::new());
    }
    match config.backend.as_str() {
        "omnirig" => Box::new(OmniRigController::new(config.omnirig_rig)),
        "rigctld" => Box::new(RigctldController::new(
            config.rigctld_host.clone(),
            config.rigctld_port,
        )),
        _ => Box::new(NoOpController::new()),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn create_controller(config: &crate::config::RadioConfig) -> Box<dyn RadioController> {
    if !config.enabled {
        return Box::new(NoOpController::new());
    }
    Box::new(RigctldController::new(
        config.rigctld_host.clone(),
        config.rigctld_port,
    ))
}
```

**Step 2: Verify it compiles (will fail - dependencies not yet created)**

Run: `cargo check 2>&1 | head -20`
Expected: Errors about missing modules (noop, rigctld)

**Step 3: Commit the trait definition**

```bash
git add src/services/radio/mod.rs
git commit -m "feat(radio): add RadioController trait and types"
```

---

## Task 2: Implement NoOpController

**Files:**
- Create: `src/services/radio/noop.rs`

**Step 1: Create NoOpController**

```rust
//! No-op radio controller for when radio control is disabled

use super::{RadioController, RadioError, RadioMode, RadioResult};

/// A no-op controller that does nothing (used when radio is disabled)
pub struct NoOpController;

impl NoOpController {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoOpController {
    fn default() -> Self {
        Self::new()
    }
}

impl RadioController for NoOpController {
    fn is_connected(&self) -> bool {
        false
    }

    fn connect(&mut self) -> RadioResult<()> {
        Err(RadioError::NotConfigured)
    }

    fn disconnect(&mut self) {
        // No-op
    }

    fn tune(&mut self, _frequency_khz: f64, _mode: RadioMode) -> RadioResult<()> {
        Err(RadioError::NotConfigured)
    }

    fn backend_name(&self) -> &'static str {
        "None"
    }
}
```

**Step 2: Commit**

```bash
git add src/services/radio/noop.rs
git commit -m "feat(radio): add NoOpController for disabled state"
```

---

## Task 3: Implement RigctldController

**Files:**
- Create: `src/services/radio/rigctld.rs`

**Step 1: Create RigctldController**

```rust
//! rigctld (Hamlib) radio controller via TCP

use super::{RadioController, RadioError, RadioMode, RadioResult};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Controller for rigctld (Hamlib network daemon)
pub struct RigctldController {
    host: String,
    port: u16,
    stream: Option<TcpStream>,
}

impl RigctldController {
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            stream: None,
        }
    }

    fn send_command(&mut self, command: &str) -> RadioResult<String> {
        let stream = self.stream.as_mut().ok_or(RadioError::NotConnected)?;

        // Send command
        writeln!(stream, "{}", command).map_err(|e| RadioError::CommandFailed(e.to_string()))?;
        stream
            .flush()
            .map_err(|e| RadioError::CommandFailed(e.to_string()))?;

        // Read response
        let mut reader = BufReader::new(stream.try_clone().map_err(|e| {
            RadioError::CommandFailed(format!("Failed to clone stream: {}", e))
        })?);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .map_err(|e| RadioError::CommandFailed(e.to_string()))?;

        let response = response.trim().to_string();

        // Check for error response (rigctld returns "RPRT <error_code>" on failure)
        if response.starts_with("RPRT") {
            let parts: Vec<&str> = response.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(code) = parts[1].parse::<i32>() {
                    if code != 0 {
                        return Err(RadioError::CommandFailed(format!(
                            "rigctld error code: {}",
                            code
                        )));
                    }
                }
            }
        }

        Ok(response)
    }
}

impl RadioController for RigctldController {
    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    fn connect(&mut self) -> RadioResult<()> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream = TcpStream::connect_timeout(
            &addr.parse().map_err(|e| {
                RadioError::ConnectionFailed(format!("Invalid address: {}", e))
            })?,
            Duration::from_secs(3),
        )
        .map_err(|e| {
            RadioError::ConnectionFailed(format!(
                "Cannot connect to rigctld at {}. Is rigctld running? ({})",
                addr, e
            ))
        })?;

        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| RadioError::ConnectionFailed(e.to_string()))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| RadioError::ConnectionFailed(e.to_string()))?;

        self.stream = Some(stream);
        Ok(())
    }

    fn disconnect(&mut self) {
        self.stream = None;
    }

    fn tune(&mut self, frequency_khz: f64, mode: RadioMode) -> RadioResult<()> {
        if self.stream.is_none() {
            return Err(RadioError::NotConnected);
        }

        // Convert kHz to Hz for rigctld
        let frequency_hz = (frequency_khz * 1000.0) as u64;

        // Set frequency: F <freq_hz>
        self.send_command(&format!("F {}", frequency_hz))?;

        // Set mode: M <mode> <passband>
        // Using 0 for passband lets rigctld use the radio's default
        self.send_command(&format!("M {} 0", mode.to_rigctld_mode()))?;

        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "rigctld"
    }
}
```

**Step 2: Commit**

```bash
git add src/services/radio/rigctld.rs
git commit -m "feat(radio): add RigctldController for Hamlib integration"
```

---

## Task 4: Implement OmniRigController (Windows-only stub)

**Files:**
- Create: `src/services/radio/omnirig.rs`

**Step 1: Create OmniRigController stub**

For now, create a stub that returns errors. Full COM implementation requires the `windows` crate and Windows-specific testing.

```rust
//! OmniRig radio controller for Windows (COM interop)

#![cfg(target_os = "windows")]

use super::{RadioController, RadioError, RadioMode, RadioResult};

/// Controller for OmniRig (Windows COM server)
pub struct OmniRigController {
    rig_number: u8,
    connected: bool,
}

impl OmniRigController {
    pub fn new(rig_number: u8) -> Self {
        Self {
            rig_number,
            connected: false,
        }
    }
}

impl RadioController for OmniRigController {
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn connect(&mut self) -> RadioResult<()> {
        // TODO: Implement COM interop with OmniRig
        // For now, return an error indicating OmniRig is not yet implemented
        Err(RadioError::ConnectionFailed(
            "OmniRig support not yet implemented. Please use rigctld.".to_string(),
        ))
    }

    fn disconnect(&mut self) {
        self.connected = false;
    }

    fn tune(&mut self, _frequency_khz: f64, _mode: RadioMode) -> RadioResult<()> {
        Err(RadioError::NotConnected)
    }

    fn backend_name(&self) -> &'static str {
        "OmniRig"
    }
}
```

**Step 2: Commit**

```bash
git add src/services/radio/omnirig.rs
git commit -m "feat(radio): add OmniRigController stub for Windows"
```

---

## Task 5: Add Radio Configuration

**Files:**
- Modify: `src/config.rs`

**Step 1: Add RadioConfig struct and update Config**

Add after line 15 (after the Config struct definition ends):

```rust
/// Radio control settings
#[derive(Debug, Clone)]
pub struct RadioConfig {
    pub enabled: bool,
    pub backend: String,
    pub rigctld_host: String,
    pub rigctld_port: u16,
    pub omnirig_rig: u8,
}

impl Default for RadioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: if cfg!(target_os = "windows") {
                "omnirig".to_string()
            } else {
                "rigctld".to_string()
            },
            rigctld_host: "localhost".to_string(),
            rigctld_port: 4532,
            omnirig_rig: 1,
        }
    }
}
```

**Step 2: Add radio field to Config struct**

Modify Config struct to add:
```rust
pub radio: RadioConfig,
```

**Step 3: Update Config::default() to include radio**

Add to the Default impl:
```rust
radio: RadioConfig::default(),
```

**Step 4: Update Config::load() to load radio settings**

Add after loading other settings (before the closing `Self {`):
```rust
let radio = RadioConfig {
    enabled: ini
        .getbool("radio", "enabled")
        .ok()
        .flatten()
        .unwrap_or(false),
    backend: ini
        .get("radio", "backend")
        .unwrap_or_else(|| {
            if cfg!(target_os = "windows") {
                "omnirig".to_string()
            } else {
                "rigctld".to_string()
            }
        }),
    rigctld_host: ini
        .get("radio", "rigctld_host")
        .unwrap_or_else(|| "localhost".to_string()),
    rigctld_port: ini
        .getint("radio", "rigctld_port")
        .ok()
        .flatten()
        .unwrap_or(4532) as u16,
    omnirig_rig: ini
        .getint("radio", "omnirig_rig")
        .ok()
        .flatten()
        .unwrap_or(1) as u8,
};
```

And add `radio,` to the returned Self.

**Step 5: Update Config::save() to save radio settings**

Add before `ini.write(&path)`:
```rust
ini.set("radio", "enabled", Some(self.radio.enabled.to_string()));
ini.set("radio", "backend", Some(self.radio.backend.clone()));
ini.set("radio", "rigctld_host", Some(self.radio.rigctld_host.clone()));
ini.set("radio", "rigctld_port", Some(self.radio.rigctld_port.to_string()));
ini.set("radio", "omnirig_rig", Some(self.radio.omnirig_rig.to_string()));
```

**Step 6: Verify compilation**

Run: `cargo check`
Expected: Success (or errors about unused imports we'll fix next)

**Step 7: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add radio control settings"
```

---

## Task 6: Export Radio Module from Services

**Files:**
- Modify: `src/services/mod.rs`

**Step 1: Add radio module export**

Add to `src/services/mod.rs`:
```rust
pub mod radio;
```

**Step 2: Verify compilation**

Run: `cargo check`
Expected: Success

**Step 3: Commit**

```bash
git add src/services/mod.rs
git commit -m "feat(services): export radio module"
```

---

## Task 7: Add Spot Selection to UI

**Files:**
- Modify: `src/app.rs`

**Step 1: Add selected_spot field to RbnVfdApp**

Add to the struct (after `raw_data_log`):
```rust
/// Currently selected spot for tuning
selected_spot: Option<crate::models::spot::AggregatedSpot>,
```

**Step 2: Initialize selected_spot in RbnVfdApp::new()**

Add to the Self initialization:
```rust
selected_spot: None,
```

**Step 3: Make spot rows selectable**

Replace the spot row rendering loop (lines 523-563) with selectable rows:

```rust
for spot in &spots {
    let is_selected = self
        .selected_spot
        .as_ref()
        .map(|s| s.callsign == spot.callsign && (s.frequency_khz - spot.frequency_khz).abs() < 0.5)
        .unwrap_or(false);

    let response = ui.horizontal(|ui| {
        // Highlight selected row
        if is_selected {
            ui.painter().rect_filled(
                ui.max_rect(),
                0.0,
                egui::Color32::from_rgb(40, 60, 80),
            );
        }

        ui.label(
            egui::RichText::new(format!("{:>10.1}", spot.frequency_khz))
                .monospace(),
        );
        ui.label(
            egui::RichText::new(format!("{:<10}", spot.callsign))
                .monospace(),
        );
        ui.label(
            egui::RichText::new(format!("{:>4}", spot.highest_snr))
                .monospace(),
        );
        ui.label(
            egui::RichText::new(format!(
                "{:>5}",
                spot.average_speed.round() as i32
            ))
            .monospace(),
        );
        ui.label(
            egui::RichText::new(format!("{:>5}", spot.spot_count))
                .monospace(),
        );

        // Age display
        let age_secs = spot.age_seconds();
        let age_text = if age_secs < 60 {
            format!("{:>3}s", age_secs)
        } else {
            format!("{:>3}m", age_secs / 60)
        };
        ui.label(egui::RichText::new(age_text).monospace());

        // Ring indicator
        let max_age =
            Duration::from_secs(self.config.max_age_minutes as u64 * 60);
        let fraction = spot.age_fraction(max_age);
        draw_age_ring(ui, fraction);
    });

    // Handle click to select
    if response.response.interact(egui::Sense::click()).clicked() {
        self.selected_spot = Some(spot.clone());
    }

    // Handle double-click to tune
    if response.response.interact(egui::Sense::click()).double_clicked() {
        self.selected_spot = Some(spot.clone());
        // TODO: Trigger tune action
    }
}
```

**Step 4: Verify compilation**

Run: `cargo check`
Expected: Success

**Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(ui): add spot selection with click and double-click"
```

---

## Task 8: Add Radio Controller to App

**Files:**
- Modify: `src/app.rs`

**Step 1: Add imports**

Add at the top of the file:
```rust
use crate::services::radio::{self, RadioController, RadioMode};
```

**Step 2: Add radio_controller field to RbnVfdApp**

Add to the struct:
```rust
/// Radio controller for CAT control
radio_controller: Box<dyn RadioController>,
/// Error message to show in popup
radio_error: Option<String>,
```

**Step 3: Initialize radio_controller in RbnVfdApp::new()**

Add after config loading:
```rust
let radio_controller = radio::create_controller(&config.radio);
```

And add to Self:
```rust
radio_controller,
radio_error: None,
```

**Step 4: Add tune method to RbnVfdApp**

```rust
/// Tune the radio to the selected spot
fn tune_to_selected(&mut self) {
    let Some(spot) = &self.selected_spot else {
        return;
    };

    // Get mode from the spot (we need to store mode in AggregatedSpot)
    // For now, default to CW since RBN is primarily CW
    let mode = RadioMode::Cw;

    match self.radio_controller.tune(spot.frequency_khz, mode) {
        Ok(()) => {
            self.status_message = format!(
                "Tuned to {:.1} kHz {}",
                spot.frequency_khz,
                mode.to_rigctld_mode()
            );
        }
        Err(e) => {
            self.radio_error = Some(e.to_string());
        }
    }
}
```

**Step 5: Verify compilation**

Run: `cargo check`
Expected: Success (may have warnings)

**Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add radio controller integration"
```

---

## Task 9: Add Mode to AggregatedSpot

**Files:**
- Modify: `src/models/spot.rs`

**Step 1: Add mode field to AggregatedSpot**

Add to the struct:
```rust
pub mode: String,
```

**Step 2: Update from_raw to set mode**

In `AggregatedSpot::from_raw`:
```rust
mode: raw.mode.clone(),
```

**Step 3: Update update method to keep mode from most recent spot**

In `AggregatedSpot::update`:
```rust
self.mode = raw.mode.clone();
```

**Step 4: Commit**

```bash
git add src/models/spot.rs
git commit -m "feat(models): add mode field to AggregatedSpot"
```

---

## Task 10: Update Tune Method to Use Spot Mode

**Files:**
- Modify: `src/app.rs`

**Step 1: Update tune_to_selected to use spot mode**

```rust
fn tune_to_selected(&mut self) {
    let Some(spot) = &self.selected_spot else {
        return;
    };

    let mode = RadioMode::from_rbn_mode(&spot.mode);

    match self.radio_controller.tune(spot.frequency_khz, mode) {
        Ok(()) => {
            self.status_message = format!(
                "Tuned to {:.1} kHz {}",
                spot.frequency_khz,
                mode.to_rigctld_mode()
            );
        }
        Err(e) => {
            self.radio_error = Some(e.to_string());
        }
    }
}
```

**Step 2: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): use spot mode when tuning"
```

---

## Task 11: Add Tune Button and Connection Indicator

**Files:**
- Modify: `src/app.rs`

**Step 1: Add tune UI section after the spot list header**

Add after the "Active Spots" heading section (around line 474):

```rust
// Tune controls
ui.horizontal(|ui| {
    // Connection indicator
    let connected = self.radio_controller.is_connected();
    let indicator_color = if connected {
        egui::Color32::from_rgb(0, 200, 0)
    } else {
        egui::Color32::from_rgb(200, 0, 0)
    };
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 5.0, indicator_color);

    // Tune button
    let can_tune = connected && self.selected_spot.is_some();
    if ui.add_enabled(can_tune, egui::Button::new("Tune")).clicked() {
        self.tune_to_selected();
    }

    // Show selected spot info
    if let Some(spot) = &self.selected_spot {
        ui.label(format!("{} @ {:.1} kHz", spot.callsign, spot.frequency_khz));
    }
});
```

**Step 2: Add error popup rendering**

Add at the end of the update method, before the closing braces:

```rust
// Error popup
if let Some(error) = &self.radio_error.clone() {
    egui::Window::new("Radio Error")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(error);
            if ui.button("OK").clicked() {
                self.radio_error = None;
            }
        });
}
```

**Step 3: Update double-click to call tune**

In the spot row double-click handler:
```rust
if response.response.interact(egui::Sense::click()).double_clicked() {
    self.selected_spot = Some(spot.clone());
    self.tune_to_selected();
}
```

**Step 4: Verify compilation**

Run: `cargo check`
Expected: Success

**Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(ui): add tune button with connection indicator"
```

---

## Task 12: Add Radio Settings Button

**Files:**
- Modify: `src/app.rs`

**Step 1: Add show_radio_settings field**

Add to RbnVfdApp struct:
```rust
/// Whether to show radio settings dialog
show_radio_settings: bool,
```

Initialize in new():
```rust
show_radio_settings: false,
```

**Step 2: Add Radio Settings button near VFD port section**

Add after the VFD port section (around line 290):

```rust
ui.add_space(4.0);

// Radio settings button
ui.horizontal(|ui| {
    ui.label("Radio:");
    ui.label(if self.radio_controller.is_connected() {
        format!("{} connected", self.radio_controller.backend_name())
    } else if self.config.radio.enabled {
        format!("{} disconnected", self.radio_controller.backend_name())
    } else {
        "Not configured".to_string()
    });
    if ui.button("Settings...").clicked() {
        self.show_radio_settings = true;
    }
});
```

**Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat(ui): add radio settings button and status"
```

---

## Task 13: Implement Radio Settings Dialog

**Files:**
- Modify: `src/app.rs`

**Step 1: Add temporary config fields for dialog**

Add to RbnVfdApp struct:
```rust
/// Temporary radio config for settings dialog
temp_radio_config: Option<crate::config::RadioConfig>,
```

Initialize in new():
```rust
temp_radio_config: None,
```

**Step 2: Add settings dialog rendering**

Add after the error popup code:

```rust
// Radio settings dialog
if self.show_radio_settings {
    // Initialize temp config if needed
    if self.temp_radio_config.is_none() {
        self.temp_radio_config = Some(self.config.radio.clone());
    }

    let mut open = true;
    egui::Window::new("Radio Settings")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            if let Some(ref mut temp) = self.temp_radio_config {
                ui.checkbox(&mut temp.enabled, "Enable radio control");

                ui.add_space(8.0);

                #[cfg(target_os = "windows")]
                {
                    ui.label("Backend:");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut temp.backend, "omnirig".to_string(), "OmniRig");
                        ui.radio_value(&mut temp.backend, "rigctld".to_string(), "rigctld");
                    });
                }

                #[cfg(not(target_os = "windows"))]
                {
                    ui.label("Backend: rigctld");
                }

                ui.add_space(8.0);

                #[cfg(target_os = "windows")]
                if temp.backend == "omnirig" {
                    ui.horizontal(|ui| {
                        ui.label("OmniRig Rig:");
                        ui.radio_value(&mut temp.omnirig_rig, 1, "Rig 1");
                        ui.radio_value(&mut temp.omnirig_rig, 2, "Rig 2");
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Host:");
                        ui.text_edit_singleline(&mut temp.rigctld_host);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Port:");
                        let mut port_str = temp.rigctld_port.to_string();
                        if ui.text_edit_singleline(&mut port_str).changed() {
                            if let Ok(port) = port_str.parse() {
                                temp.rigctld_port = port;
                            }
                        }
                    });
                }

                #[cfg(not(target_os = "windows"))]
                {
                    ui.horizontal(|ui| {
                        ui.label("Host:");
                        ui.text_edit_singleline(&mut temp.rigctld_host);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Port:");
                        let mut port_str = temp.rigctld_port.to_string();
                        if ui.text_edit_singleline(&mut port_str).changed() {
                            if let Ok(port) = port_str.parse() {
                                temp.rigctld_port = port;
                            }
                        }
                    });
                }

                ui.add_space(8.0);

                // Test connection button
                if temp.enabled {
                    if ui.button("Test Connection").clicked() {
                        let mut test_controller = radio::create_controller(temp);
                        match test_controller.connect() {
                            Ok(()) => {
                                self.status_message = "Radio connection successful!".to_string();
                            }
                            Err(e) => {
                                self.radio_error = Some(e.to_string());
                            }
                        }
                    }
                }

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        // Apply settings
                        self.config.radio = temp.clone();
                        self.radio_controller = radio::create_controller(&self.config.radio);
                        if self.config.radio.enabled {
                            let _ = self.radio_controller.connect();
                        }
                        self.show_radio_settings = false;
                        self.temp_radio_config = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_radio_settings = false;
                        self.temp_radio_config = None;
                    }
                });
            }
        });

    if !open {
        self.show_radio_settings = false;
        self.temp_radio_config = None;
    }
}
```

**Step 3: Verify compilation**

Run: `cargo check`
Expected: Success

**Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(ui): implement radio settings dialog"
```

---

## Task 14: Update Status Line with Radio Status

**Files:**
- Modify: `src/app.rs`

**Step 1: Update status line display**

Replace the status line section (around lines 294-305) with:

```rust
// Status line
ui.horizontal(|ui| {
    ui.label("Status:");
    ui.label(&self.status_message);
});

// VFD and Radio status
ui.horizontal(|ui| {
    if self.vfd_display.is_open() {
        ui.label(format!("VFD: {}", self.vfd_display.port_name()));
    } else {
        ui.label("VFD: Closed");
    }
    ui.separator();
    if self.config.radio.enabled {
        if self.radio_controller.is_connected() {
            ui.label(format!("Radio: {} connected", self.radio_controller.backend_name()));
        } else {
            ui.label(format!("Radio: {} disconnected", self.radio_controller.backend_name()));
        }
    } else {
        ui.label("Radio: Not configured");
    }
});
```

**Step 2: Commit**

```bash
git add src/app.rs
git commit -m "feat(ui): update status line with radio status"
```

---

## Task 15: Run Clippy and Fix Issues

**Files:**
- Various

**Step 1: Run clippy**

Run: `cargo clippy 2>&1`
Expected: May have warnings to fix

**Step 2: Fix any clippy warnings**

Address each warning as needed.

**Step 3: Commit fixes**

```bash
git add -A
git commit -m "style: fix clippy warnings"
```

---

## Task 16: Test Build and Manual Verification

**Step 1: Build release**

Run: `cargo build --release`
Expected: Success

**Step 2: Run and verify**

Run: `cargo run --release`

Manual verification checklist:
- [ ] App starts without errors
- [ ] Radio Settings button appears
- [ ] Settings dialog opens and shows correct controls
- [ ] Clicking a spot row highlights it
- [ ] Tune button is disabled when no spot selected
- [ ] Tune button is disabled when radio not connected
- [ ] Status line shows radio status

**Step 3: Commit any final fixes**

```bash
git add -A
git commit -m "chore: final build verification"
```

---

## Summary

This plan implements radio control in 16 tasks:

1. **Tasks 1-4**: RadioController trait and implementations (NoOp, rigctld, OmniRig stub)
2. **Tasks 5-6**: Configuration for radio settings
3. **Tasks 7-10**: Spot selection and mode handling
4. **Tasks 11-14**: UI components (tune button, indicator, settings dialog, status)
5. **Tasks 15-16**: Cleanup and verification

The rigctld implementation is complete and functional. The OmniRig implementation is a stub that can be completed later with Windows COM interop when Windows testing is available.
