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
