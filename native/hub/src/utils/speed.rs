use std::time::{Duration, Instant};

/// Calculates average speed from measurements taken within a specified time window.
///
/// **Notes:**
/// - Measurements are accumulated and not discarded until `average()` is called.
/// - No idle time is considered.
#[derive(Debug)]
pub struct AverageSpeed {
    /// Time window over which average speed is calculated.
    pub time_window: Duration,
    /// Measurements of transferred bytes with timestamps.
    pub measurements: Vec<(u64, Instant)>,
    /// Total transferred bytes. Used for `add_from_total()`.
    total: u64,
}

impl AverageSpeed {
    pub fn new(time_window: Duration) -> Self {
        Self { time_window, measurements: Vec::new(), total: 0 }
    }

    /// Adds a new measurement to the average speed calculation.
    /// Existing measurements are discarded if they are older than the time window.
    pub fn add(&mut self, bytes: u64) {
        // self.measurements.retain(|(_, instant)| instant.elapsed() < self.time_window);

        self.measurements.push((bytes, Instant::now()));
        self.total += bytes;
    }

    /// Calculates and adds a new measurement from the given new total.
    pub fn add_from_total(&mut self, new_total: u64) {
        assert!(new_total >= self.total);
        self.add(new_total - self.total);
        self.total = new_total;
    }

    /// Calculates average speed per second over the time window.
    ///
    /// Returns 0 if no measurements have been added or less than 1 second has elapsed.
    pub fn average(&mut self) -> u64 {
        if self.measurements.is_empty() {
            return 0;
        }

        self.measurements.retain(|(_, instant)| instant.elapsed() < self.time_window);

        let total_bytes = self.measurements.iter().map(|(bytes, _)| *bytes).sum::<u64>();
        total_bytes / self.measurements.len() as u64
    }
}
