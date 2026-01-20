use crate::models::{AggregatedSpot, RawSpot};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Thread-safe store for aggregated spots
#[derive(Clone)]
pub struct SpotStore {
    spots: Arc<Mutex<HashMap<String, AggregatedSpot>>>,
}

impl SpotStore {
    pub fn new() -> Self {
        Self {
            spots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add or update a spot (stores all spots, filtering happens at retrieval)
    pub fn add_spot(&self, raw: RawSpot) {
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

    /// Remove spots older than 30 minutes (hard limit for memory management)
    pub fn purge_old_spots(&self) {
        let cutoff = Instant::now() - Duration::from_secs(30 * 60);

        if let Ok(mut spots) = self.spots.lock() {
            spots.retain(|_, spot| spot.last_spotted >= cutoff);
        }
    }

    /// Get spots filtered by min_snr and max_age, sorted by frequency
    pub fn get_filtered_spots(&self, min_snr: i32, max_age: Duration) -> Vec<AggregatedSpot> {
        let cutoff = Instant::now() - max_age;

        if let Ok(spots) = self.spots.lock() {
            let mut result: Vec<_> = spots
                .values()
                .filter(|spot| spot.highest_snr >= min_snr && spot.last_spotted >= cutoff)
                .cloned()
                .collect();
            result.sort_by(|a, b| a.frequency_khz.partial_cmp(&b.frequency_khz).unwrap());
            result
        } else {
            Vec::new()
        }
    }

    /// Get all spots sorted by frequency (no filtering, used internally)
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
