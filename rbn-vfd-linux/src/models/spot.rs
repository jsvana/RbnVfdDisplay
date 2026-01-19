use std::time::Instant;

/// Raw spot data as received from RBN telnet
#[derive(Debug, Clone)]
pub struct RawSpot {
    #[allow(dead_code)]
    pub spotter_callsign: String,
    pub spotted_callsign: String,
    pub frequency_khz: f64,
    pub snr: i32,
    pub speed_wpm: i32,
    #[allow(dead_code)]
    pub mode: String,
    #[allow(dead_code)]
    pub timestamp: Instant,
}

impl RawSpot {
    pub fn new(
        spotter_callsign: String,
        spotted_callsign: String,
        frequency_khz: f64,
        snr: i32,
        speed_wpm: i32,
        mode: String,
    ) -> Self {
        Self {
            spotter_callsign,
            spotted_callsign,
            frequency_khz,
            snr,
            speed_wpm,
            mode,
            timestamp: Instant::now(),
        }
    }
}

/// Aggregated spot data for display
#[derive(Debug, Clone)]
pub struct AggregatedSpot {
    pub callsign: String,
    pub frequency_khz: f64,
    #[allow(dead_code)]
    pub center_frequency_khz: f64,
    pub highest_snr: i32,
    pub average_speed: f64,
    pub spot_count: u32,
    pub last_spotted: Instant,
}

impl AggregatedSpot {
    /// Create a new aggregated spot from a raw spot
    pub fn from_raw(raw: &RawSpot) -> Self {
        Self {
            callsign: raw.spotted_callsign.clone(),
            frequency_khz: raw.frequency_khz,
            center_frequency_khz: raw.frequency_khz.round(),
            highest_snr: raw.snr,
            average_speed: raw.speed_wpm as f64,
            spot_count: 1,
            last_spotted: Instant::now(),
        }
    }

    /// Update this spot with new data using incremental averaging
    pub fn update(&mut self, raw: &RawSpot) {
        self.spot_count += 1;
        self.average_speed += (raw.speed_wpm as f64 - self.average_speed) / self.spot_count as f64;
        self.frequency_khz += (raw.frequency_khz - self.frequency_khz) / self.spot_count as f64;
        if raw.snr > self.highest_snr {
            self.highest_snr = raw.snr;
        }
        self.last_spotted = Instant::now();
    }

    /// Generate the unique key for this spot (callsign + center frequency)
    #[allow(dead_code)]
    pub fn key(&self) -> String {
        format!("{}|{:.0}", self.callsign, self.center_frequency_khz)
    }

    /// Format for VFD display (max 20 characters)
    /// Format: "14033.0 WO6W 24"
    pub fn to_display_string(&self) -> String {
        let freq_str = format!("{:.1}", self.frequency_khz);
        let speed_str = format!("{}", self.average_speed.round() as i32);

        // Calculate available space for callsign
        // Format: "FREQ.F CALL SPD"
        let used_chars = freq_str.len() + 1 + speed_str.len() + 1;
        let call_max_len = 20_usize.saturating_sub(used_chars);

        let call_str = if self.callsign.len() <= call_max_len {
            &self.callsign
        } else {
            &self.callsign[..call_max_len]
        };

        format!("{} {} {}", freq_str, call_str, speed_str)
    }
}
