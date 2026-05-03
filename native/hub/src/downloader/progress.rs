use std::{collections::VecDeque, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransferStats {
    pub bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed: u64,
}

#[derive(Debug)]
pub(crate) struct TransferSpeedTracker {
    samples: VecDeque<TransferSpeedSample>,
    window_millis: u128,
}

impl TransferSpeedTracker {
    pub(crate) fn new(window: Duration) -> Self {
        let mut samples = VecDeque::new();
        samples.push_back(TransferSpeedSample { bytes: 0, elapsed_millis: 0 });
        Self { samples, window_millis: window.as_millis() }
    }

    pub(crate) fn record(&mut self, bytes: u64, elapsed_millis: u128) -> u64 {
        self.samples.push_back(TransferSpeedSample { bytes, elapsed_millis });
        let cutoff = elapsed_millis.saturating_sub(self.window_millis);

        while self.samples.len() > 1
            && self.samples.get(1).is_some_and(|sample| sample.elapsed_millis <= cutoff)
        {
            self.samples.pop_front();
        }

        let baseline = self.samples.front().expect("speed tracker must retain a baseline sample");
        speed_bytes_per_sec(
            bytes.saturating_sub(baseline.bytes),
            elapsed_millis.saturating_sub(baseline.elapsed_millis),
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct TransferSpeedSample {
    bytes: u64,
    elapsed_millis: u128,
}

fn speed_bytes_per_sec(downloaded_bytes: u64, elapsed_millis: u128) -> u64 {
    if elapsed_millis == 0 {
        return 0;
    }
    ((downloaded_bytes as u128 * 1000) / elapsed_millis) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_speed_tracker_uses_rolling_window() {
        let mut tracker = TransferSpeedTracker::new(Duration::from_secs(2));

        assert_eq!(tracker.record(1_000, 1_000), 1_000);
        assert_eq!(tracker.record(2_000, 2_000), 1_000);
        assert_eq!(tracker.record(3_000, 3_000), 1_000);
        assert_eq!(tracker.record(13_000, 4_000), 5_500);
    }

    #[test]
    fn transfer_speed_tracker_handles_non_advancing_bytes() {
        let mut tracker = TransferSpeedTracker::new(Duration::from_secs(2));

        assert_eq!(tracker.record(4_000, 1_000), 4_000);
        assert_eq!(tracker.record(4_000, 2_000), 2_000);
        assert_eq!(tracker.record(4_000, 3_000), 0);
    }
}
