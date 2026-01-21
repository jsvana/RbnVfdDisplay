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
