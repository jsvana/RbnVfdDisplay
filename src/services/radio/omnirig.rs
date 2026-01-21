//! OmniRig radio controller for Windows (COM interop)

#![cfg(target_os = "windows")]

use super::{RadioController, RadioError, RadioMode, RadioResult};
use winsafe::{self as w, co, prelude::*};

/// OmniRig mode constants (from OmniRig type library)
#[allow(dead_code)]
mod omnirig_modes {
    pub const PM_CW_U: i32 = 0x00800000;
    pub const PM_CW_L: i32 = 0x01000000;
    pub const PM_SSB_U: i32 = 0x02000000;
    pub const PM_SSB_L: i32 = 0x04000000;
    pub const PM_DIG_U: i32 = 0x08000000;
    pub const PM_DIG_L: i32 = 0x10000000;
    pub const PM_AM: i32 = 0x20000000;
    pub const PM_FM: i32 = 0x40000000;
}

/// Controller for OmniRig (Windows COM server)
pub struct OmniRigController {
    rig_number: u8,
    omnirig: Option<w::IDispatch>,
    rig: Option<w::IDispatch>,
}

impl OmniRigController {
    pub fn new(rig_number: u8) -> Self {
        Self {
            rig_number: rig_number.clamp(1, 2),
            omnirig: None,
            rig: None,
        }
    }

    /// Convert RadioMode to OmniRig mode constant
    fn mode_to_omnirig(mode: RadioMode) -> i32 {
        match mode {
            RadioMode::Cw => omnirig_modes::PM_CW_U,
            RadioMode::CwReverse => omnirig_modes::PM_CW_L,
            RadioMode::Usb => omnirig_modes::PM_SSB_U,
            RadioMode::Lsb => omnirig_modes::PM_SSB_L,
            RadioMode::Am => omnirig_modes::PM_AM,
            RadioMode::Fm => omnirig_modes::PM_FM,
            RadioMode::Rtty => omnirig_modes::PM_DIG_U,
            RadioMode::RttyReverse => omnirig_modes::PM_DIG_L,
            RadioMode::Data => omnirig_modes::PM_DIG_U,
        }
    }

    /// Get the rig property name based on rig number
    fn rig_property_name(&self) -> &'static str {
        if self.rig_number == 2 {
            "Rig2"
        } else {
            "Rig1"
        }
    }
}

impl RadioController for OmniRigController {
    fn is_connected(&self) -> bool {
        self.rig.is_some()
    }

    fn connect(&mut self) -> RadioResult<()> {
        // Initialize COM if not already done
        let _com_guard =
            w::CoInitializeEx(co::COINIT::APARTMENTTHREADED | co::COINIT::DISABLE_OLE1DDE)
                .map_err(|e| {
                    RadioError::ConnectionFailed(format!("Failed to initialize COM: {}", e))
                })?;

        // Get CLSID for OmniRig
        let clsid = w::CLSIDFromProgID("Omnirig.OmnirigX").map_err(|e| {
            RadioError::ConnectionFailed(format!(
                "OmniRig not found. Is it installed? Error: {}",
                e
            ))
        })?;

        // Create OmniRig instance
        let omnirig: w::IDispatch =
            w::CoCreateInstance(&clsid, None::<&w::IUnknown>, co::CLSCTX::LOCAL_SERVER).map_err(
                |e| {
                    RadioError::ConnectionFailed(format!(
                        "Failed to create OmniRig instance. Is OmniRig running? Error: {}",
                        e
                    ))
                },
            )?;

        // Get the rig object (Rig1 or Rig2)
        let rig_name = self.rig_property_name();
        let rig_variant = omnirig.invoke_get(rig_name, &[]).map_err(|e| {
            RadioError::ConnectionFailed(format!("Failed to get {}: {}", rig_name, e))
        })?;

        // Extract IDispatch from Variant
        let rig = match rig_variant {
            w::Variant::Dispatch(disp) => disp,
            _ => {
                return Err(RadioError::ConnectionFailed(format!(
                    "Failed to get {} interface: not an IDispatch",
                    rig_name
                )))
            }
        };

        self.omnirig = Some(omnirig);
        self.rig = Some(rig);

        Ok(())
    }

    fn disconnect(&mut self) {
        self.rig = None;
        self.omnirig = None;
    }

    fn tune(&mut self, frequency_khz: f64, mode: RadioMode) -> RadioResult<()> {
        let rig = self.rig.as_ref().ok_or(RadioError::NotConnected)?;

        // Convert frequency from kHz to Hz
        let freq_hz = (frequency_khz * 1000.0) as i32;

        // Set frequency (FreqA property)
        let freq_variant = w::Variant::I4(freq_hz);
        rig.invoke_put("FreqA", &freq_variant)
            .map_err(|e| RadioError::CommandFailed(format!("Failed to set frequency: {}", e)))?;

        // Set mode
        let mode_value = Self::mode_to_omnirig(mode);
        let mode_variant = w::Variant::I4(mode_value);
        rig.invoke_put("Mode", &mode_variant)
            .map_err(|e| RadioError::CommandFailed(format!("Failed to set mode: {}", e)))?;

        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "OmniRig"
    }
}
