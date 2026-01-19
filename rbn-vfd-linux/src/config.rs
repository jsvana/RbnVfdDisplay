use configparser::ini::Ini;
use directories::ProjectDirs;
use std::path::PathBuf;

/// Application settings
#[derive(Debug, Clone)]
pub struct Config {
    pub callsign: String,
    pub serial_port: String,
    pub min_snr: i32,
    pub max_age_minutes: u32,
    pub scroll_interval_seconds: u32,
    /// Percentage chance (0-100) to show random character when idle
    pub random_char_percent: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            callsign: String::new(),
            serial_port: String::new(),
            min_snr: 10,
            max_age_minutes: 10,
            scroll_interval_seconds: 3,
            random_char_percent: 20,
        }
    }
}

impl Config {
    /// Get the config file path
    fn config_path() -> Option<PathBuf> {
        ProjectDirs::from("com", "w6jsv", "rbn-vfd-display")
            .map(|dirs| dirs.config_dir().join("settings.ini"))
    }

    /// Load config from file, or return defaults if file doesn't exist
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };

        if !path.exists() {
            return Self::default();
        }

        let mut ini = Ini::new();
        if ini.load(&path).is_err() {
            return Self::default();
        }

        Self {
            callsign: ini.get("connection", "callsign").unwrap_or_default(),
            serial_port: ini.get("display", "serial_port").unwrap_or_default(),
            min_snr: ini
                .getint("filters", "min_snr")
                .ok()
                .flatten()
                .unwrap_or(10) as i32,
            max_age_minutes: ini
                .getint("filters", "max_age_minutes")
                .ok()
                .flatten()
                .unwrap_or(10) as u32,
            scroll_interval_seconds: ini
                .getint("filters", "scroll_interval_seconds")
                .ok()
                .flatten()
                .unwrap_or(3) as u32,
            random_char_percent: ini
                .getint("display", "random_char_percent")
                .ok()
                .flatten()
                .unwrap_or(20) as u32,
        }
    }

    /// Save config to file
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = Self::config_path() else {
            return Err("Could not determine config path".to_string());
        };

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let mut ini = Ini::new();
        ini.set("connection", "callsign", Some(self.callsign.clone()));
        ini.set("display", "serial_port", Some(self.serial_port.clone()));
        ini.set("filters", "min_snr", Some(self.min_snr.to_string()));
        ini.set(
            "filters",
            "max_age_minutes",
            Some(self.max_age_minutes.to_string()),
        );
        ini.set(
            "filters",
            "scroll_interval_seconds",
            Some(self.scroll_interval_seconds.to_string()),
        );
        ini.set(
            "display",
            "random_char_percent",
            Some(self.random_char_percent.to_string()),
        );

        ini.write(&path)
            .map_err(|e| format!("Failed to write config: {}", e))
    }

    /// Reset to defaults
    pub fn reset_to_defaults(&mut self) {
        let defaults = Self::default();
        self.min_snr = defaults.min_snr;
        self.max_age_minutes = defaults.max_age_minutes;
        self.scroll_interval_seconds = defaults.scroll_interval_seconds;
        self.random_char_percent = defaults.random_char_percent;
        // Keep callsign and serial_port as-is
    }
}
