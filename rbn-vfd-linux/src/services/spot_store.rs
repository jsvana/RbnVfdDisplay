use crate::models::{AggregatedSpot, RawSpot};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Thread-safe store for aggregated spots
#[derive(Clone)]
pub struct SpotStore {
    spots: Arc<Mutex<HashMap<String, AggregatedSpot>>>,
    min_snr: Arc<Mutex<i32>>,
    max_age: Arc<Mutex<Duration>>,
}

impl SpotStore {
    pub fn new(min_snr: i32, max_age_minutes: u32) -> Self {
        Self {
            spots: Arc::new(Mutex::new(HashMap::new())),
            min_snr: Arc::new(Mutex::new(min_snr)),
            max_age: Arc::new(Mutex::new(Duration::from_secs(max_age_minutes as u64 * 60))),
        }
    }

    /// Set minimum SNR filter
    pub fn set_min_snr(&self, snr: i32) {
        if let Ok(mut min) = self.min_snr.lock() {
            *min = snr;
        }
    }

    /// Set maximum age for spots
    pub fn set_max_age_minutes(&self, minutes: u32) {
        if let Ok(mut age) = self.max_age.lock() {
            *age = Duration::from_secs(minutes as u64 * 60);
        }
    }

    /// Add or update a spot
    pub fn add_spot(&self, raw: RawSpot) {
        // Check SNR filter
        let min_snr = self.min_snr.lock().map(|m| *m).unwrap_or(0);
        if raw.snr < min_snr {
            return;
        }

        let center_freq = raw.frequency_khz.round();
        let key = format!("{}|{:.0}", raw.spotted_callsign, center_freq);

        if let Ok(mut spots) = self.spots.lock() {
            if let Some(existing) = spots.get_mut(&key) {
                existing.update(&raw);
            } else {
                let spot = AggregatedSpot::from_raw(&raw);
                spots.insert(key, spot);
            }
        }
    }

    /// Remove spots older than max age
    pub fn purge_old_spots(&self) {
        let max_age = self.max_age.lock().map(|m| *m).unwrap_or(Duration::from_secs(600));
        let cutoff = Instant::now() - max_age;

        if let Ok(mut spots) = self.spots.lock() {
            spots.retain(|_, spot| spot.last_spotted >= cutoff);
        }
    }

    /// Get all spots sorted by frequency
    pub fn get_spots_by_frequency(&self) -> Vec<AggregatedSpot> {
        if let Ok(spots) = self.spots.lock() {
            let mut result: Vec<_> = spots.values().cloned().collect();
            result.sort_by(|a, b| a.frequency_khz.partial_cmp(&b.frequency_khz).unwrap());
            result
        } else {
            Vec::new()
        }
    }

    /// Get all spots sorted by recency
    #[allow(dead_code)]
    pub fn get_spots_by_recency(&self) -> Vec<AggregatedSpot> {
        if let Ok(spots) = self.spots.lock() {
            let mut result: Vec<_> = spots.values().cloned().collect();
            result.sort_by(|a, b| b.last_spotted.cmp(&a.last_spotted));
            result
        } else {
            Vec::new()
        }
    }

    /// Get spot count
    pub fn count(&self) -> usize {
        self.spots.lock().map(|s| s.len()).unwrap_or(0)
    }

    /// Clear all spots
    #[allow(dead_code)]
    pub fn clear(&self) {
        if let Ok(mut spots) = self.spots.lock() {
            spots.clear();
        }
    }
}
