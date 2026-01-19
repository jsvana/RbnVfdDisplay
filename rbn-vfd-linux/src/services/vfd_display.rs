use crate::models::AggregatedSpot;
use rand::Rng;
use serialport::SerialPort;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DISPLAY_WIDTH: usize = 20;
const DISPLAY_LINES: usize = 2;

// VFD commands - simple protocol without ANSI escape sequences
const CLEAR_DISPLAY: &[u8] = &[0x0C]; // Form feed - clear and home cursor

/// VFD Display controller
pub struct VfdDisplay {
    port: Option<Box<dyn SerialPort>>,
    port_name: String,
    scroll_index: usize,
    scroll_interval: Duration,
    last_update: Instant,
    force_random_mode: bool,
    random_char_percent: u32,
    random_state: RandomCharState,
    current_lines: [String; 2],
}

struct RandomCharState {
    showing_char: bool,
    char_col: usize,
    char_row: usize,
    character: char,
    last_second: u64,
}

impl Default for RandomCharState {
    fn default() -> Self {
        Self {
            showing_char: false,
            char_col: 0,
            char_row: 0,
            character: ' ',
            last_second: 0,
        }
    }
}

impl VfdDisplay {
    pub fn new() -> Self {
        Self {
            port: None,
            port_name: String::new(),
            scroll_index: 0,
            scroll_interval: Duration::from_secs(3),
            last_update: Instant::now(),
            force_random_mode: false,
            random_char_percent: 20,
            random_state: RandomCharState::default(),
            current_lines: [String::new(), String::new()],
        }
    }

    /// Get available serial ports
    pub fn available_ports() -> Vec<String> {
        serialport::available_ports()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.port_name)
            .collect()
    }

    /// Open a serial port
    pub fn open(&mut self, port_name: &str) -> Result<(), String> {
        self.close();

        let port = serialport::new(port_name, 9600)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .timeout(Duration::from_millis(1000))
            .open()
            .map_err(|e| format!("Failed to open {}: {}", port_name, e))?;

        self.port = Some(port);
        self.port_name = port_name.to_string();
        self.clear();
        Ok(())
    }

    /// Close the serial port
    pub fn close(&mut self) {
        if self.port.is_some() {
            self.clear();
        }
        self.port = None;
        self.port_name.clear();
    }

    /// Check if port is open
    pub fn is_open(&self) -> bool {
        self.port.is_some()
    }

    /// Get current port name
    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    /// Set scroll interval
    pub fn set_scroll_interval(&mut self, seconds: u32) {
        self.scroll_interval = Duration::from_secs(seconds as u64);
    }

    /// Set force random mode
    pub fn set_force_random_mode(&mut self, enabled: bool) {
        self.force_random_mode = enabled;
    }

    /// Set random character duty cycle percentage (0-100)
    pub fn set_random_char_percent(&mut self, percent: u32) {
        self.random_char_percent = percent.min(100);
    }

    /// Get current random char percent
    pub fn random_char_percent(&self) -> u32 {
        self.random_char_percent
    }

    /// Clear the display
    pub fn clear(&mut self) {
        if let Some(ref mut port) = self.port {
            let _ = port.write_all(CLEAR_DISPLAY);
        }
        self.current_lines = [String::new(), String::new()];
    }

    /// Pad or truncate text to exactly DISPLAY_WIDTH characters
    fn format_line(text: &str) -> String {
        format!("{:width$}", text, width = DISPLAY_WIDTH)
            .chars()
            .take(DISPLAY_WIDTH)
            .collect()
    }

    /// Write both lines to the display
    /// Uses simple protocol: clear, then write 40 chars (20 per line, auto-wraps)
    fn write_display(&mut self, line1: &str, line2: &str) {
        if let Some(ref mut port) = self.port {
            // Clear and home cursor
            let _ = port.write_all(CLEAR_DISPLAY);

            // Write line 1 (exactly 20 chars) - cursor auto-advances
            let padded1 = Self::format_line(line1);
            let _ = port.write_all(padded1.as_bytes());

            // Write line 2 (exactly 20 chars) - wraps to second line
            let padded2 = Self::format_line(line2);
            let _ = port.write_all(padded2.as_bytes());
        }
        self.current_lines[0] = line1.to_string();
        self.current_lines[1] = line2.to_string();
    }

    /// Write text to a specific line (0 or 1)
    fn write_line(&mut self, line: usize, text: &str) {
        // Update internal state and rewrite entire display
        self.current_lines[line] = text.to_string();
        let line1 = self.current_lines[0].clone();
        let line2 = self.current_lines[1].clone();
        self.write_display(&line1, &line2);
    }

    /// Update display with spots (call periodically)
    pub fn update(&mut self, spots: &[AggregatedSpot]) {
        if !self.is_open() {
            return;
        }

        // Random mode updates on its own timing (duty cycle within each second)
        if self.force_random_mode || spots.is_empty() {
            self.update_random_mode();
            return;
        }

        // Spot display uses scroll interval
        let now = Instant::now();
        if now.duration_since(self.last_update) < self.scroll_interval {
            return;
        }
        self.last_update = now;

        match spots.len() {
            1 => {
                self.write_line(0, &spots[0].to_display_string());
                self.write_line(1, "");
            }
            2 => {
                self.write_line(0, &spots[0].to_display_string());
                self.write_line(1, &spots[1].to_display_string());
            }
            _ => {
                // Scroll through spots
                let idx1 = self.scroll_index % spots.len();
                let idx2 = (self.scroll_index + 1) % spots.len();
                self.write_line(0, &spots[idx1].to_display_string());
                self.write_line(1, &spots[idx2].to_display_string());
                self.scroll_index = (self.scroll_index + 1) % spots.len();
            }
        }
    }

    fn update_random_mode(&mut self) {
        // Get current time info
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let current_second = now.as_secs();
        let ms_in_second = (now.as_millis() % 1000) as u32;

        // Calculate threshold: e.g., 20% duty cycle = first 200ms of each second
        let threshold_ms = self.random_char_percent * 10;
        let should_show = ms_in_second < threshold_ms && self.random_char_percent > 0;

        // Check if this is a new second - generate new random char and position
        if current_second != self.random_state.last_second {
            self.random_state.last_second = current_second;
            let mut rng = rand::thread_rng();
            // Generate random character (A-Z, 0-9)
            self.random_state.character = if rng.gen::<bool>() {
                rng.gen_range(b'A'..=b'Z') as char
            } else {
                rng.gen_range(b'0'..=b'9') as char
            };
            self.random_state.char_col = rng.gen_range(0..DISPLAY_WIDTH);
            self.random_state.char_row = rng.gen_range(0..DISPLAY_LINES);
        }

        // Handle transitions
        if should_show && !self.random_state.showing_char {
            // Transition to showing character
            self.random_state.showing_char = true;

            // Create display with single character
            let mut line0 = " ".repeat(DISPLAY_WIDTH);
            let mut line1 = " ".repeat(DISPLAY_WIDTH);

            if self.random_state.char_row == 0 {
                line0.replace_range(
                    self.random_state.char_col..self.random_state.char_col + 1,
                    &self.random_state.character.to_string(),
                );
            } else {
                line1.replace_range(
                    self.random_state.char_col..self.random_state.char_col + 1,
                    &self.random_state.character.to_string(),
                );
            }

            self.write_display(&line0, &line1);
        } else if !should_show && self.random_state.showing_char {
            // Transition to blank
            self.random_state.showing_char = false;
            self.clear();
        }
    }

    /// Get current display lines for preview
    pub fn get_preview(&self) -> [String; 2] {
        self.current_lines.clone()
    }

    /// Get random mode state for preview
    pub fn is_in_random_mode(&self) -> bool {
        self.force_random_mode
    }
}
