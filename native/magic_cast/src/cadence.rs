//! Adaptive buffer-occupancy and source-rate controller for live paced playback.

use std::collections::VecDeque;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Debug)]
pub struct PacingConfig {
    /// Initial source-rate estimate and upper bound for later estimates.
    pub initial_fps: f64,
    /// Length of the rolling source-rate estimation window.
    pub rate_window_seconds: f64,
    /// Smallest buffer target, in frames.
    pub min_buffer_frames: u32,
    /// Largest buffer target, in frames.
    pub max_buffer_frames: u32,
    /// Gaps at least this long restart the rate window while retaining the current estimate.
    pub stall_threshold_ms: f64,
    /// Playback-rate change applied for each frame of buffer occupancy error.
    pub occupancy_gain: f64,
    /// Largest fractional playback-rate change made by buffer occupancy correction.
    pub max_rate_adjustment: f64,
    /// Multiplier applied to the occupancy correction while the buffer is below target.
    ///
    /// Set this to `1.0` to use the same correction above and below the target.
    pub low_buffer_gain_multiplier: f64,
    /// Stable playback time before a temporary underflow reserve is removed.
    ///
    /// Another underflow during this period promotes the reserve into the retained buffer target.
    pub transient_recovery_seconds: f64,
    /// Stable playback time required before the first buffer-target reduction.
    pub stable_grace_seconds: f64,
    /// Time between buffer-target reductions after the stable grace period.
    pub target_decay_interval_seconds: f64,
    /// Age of the oldest buffered frame that triggers hard catch-up.
    pub hard_latency_ms: f64,
}

impl PacingConfig {
    /// Checks that the pacing settings can produce valid deadlines.
    pub fn validate(&self) -> Result<()> {
        if !self.initial_fps.is_finite() || self.initial_fps <= 0.0 {
            return Err("initial FPS must be greater than zero".into());
        }
        if !self.rate_window_seconds.is_finite() || self.rate_window_seconds <= 0.0 {
            return Err("rate window must be greater than zero".into());
        }
        if self.max_buffer_frames < self.min_buffer_frames {
            return Err("maximum buffer frames must not be smaller than the minimum".into());
        }
        if !self.stall_threshold_ms.is_finite() || self.stall_threshold_ms <= 0.0 {
            return Err("stall threshold must be greater than zero".into());
        }
        if !self.occupancy_gain.is_finite() || self.occupancy_gain < 0.0 {
            return Err("occupancy gain must not be negative".into());
        }
        if !self.max_rate_adjustment.is_finite() || !(0.0..1.0).contains(&self.max_rate_adjustment)
        {
            return Err("maximum rate adjustment must be at least zero and less than one".into());
        }
        if !self.low_buffer_gain_multiplier.is_finite() || self.low_buffer_gain_multiplier < 1.0 {
            return Err("low-buffer gain multiplier must be at least one".into());
        }
        if !self.transient_recovery_seconds.is_finite() || self.transient_recovery_seconds <= 0.0 {
            return Err("transient recovery period must be greater than zero".into());
        }
        if !self.stable_grace_seconds.is_finite() || self.stable_grace_seconds <= 0.0 {
            return Err("stable grace period must be greater than zero".into());
        }
        if !self.target_decay_interval_seconds.is_finite()
            || self.target_decay_interval_seconds <= 0.0
        {
            return Err("target decay interval must be greater than zero".into());
        }
        if !self.hard_latency_ms.is_finite() || self.hard_latency_ms <= 0.0 {
            return Err("hard latency must be greater than zero".into());
        }
        Ok(())
    }
}

/// Result of polling an [`AdaptivePacer`] at a presentation deadline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacingAction {
    /// No frame is due yet, or the pacer is filling its buffer.
    Wait,
    /// Present one buffered frame at this timestamp.
    Present { presentation_ns: u64 },
    /// Hold the last image until the buffer reaches `refill_buffer_frames`.
    Rebuffer {
        /// Desired buffer occupancy after playback resumes.
        target_buffer_frames: u32,
        /// Buffer size required to resume playback.
        refill_buffer_frames: u32,
    },
}

/// Statistics collected by [`AdaptivePacer`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PacingStats {
    pub underflow_events: usize,
    pub rebuffer_events: usize,
    pub transient_recovery_events: usize,
    pub target_promotions: usize,
    pub target_increases: usize,
    pub target_decreases: usize,
    pub catch_up_events: usize,
    pub catch_up_frames: usize,
}

/// Buffer occupancy and source-rate controller used by live paced playback.
#[derive(Debug)]
pub struct AdaptivePacer {
    config: PacingConfig,
    rate: RateEstimator,
    /// Buffer target retained after the short recovery period ends.
    target_buffer_frames: u32,
    /// Short-lived reserve used to test whether an underflow was isolated.
    transient_buffer_frames: u32,
    refill_buffer_frames: u32,
    filling: bool,
    next_deadline_ns: Option<u64>,
    last_arrival_ns: Option<u64>,
    stable_since_ns: Option<u64>,
    transient_until_ns: Option<u64>,
    last_target_decrease_ns: Option<u64>,
    stats: PacingStats,
}

impl AdaptivePacer {
    /// Creates a controller in its initial buffer-filling state.
    pub fn new(config: PacingConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            rate: RateEstimator::new(config.initial_fps, config.rate_window_seconds),
            target_buffer_frames: config.min_buffer_frames,
            transient_buffer_frames: 0,
            refill_buffer_frames: config.min_buffer_frames,
            config,
            filling: true,
            next_deadline_ns: None,
            last_arrival_ns: None,
            stable_since_ns: None,
            transient_until_ns: None,
            last_target_decrease_ns: None,
            stats: PacingStats::default(),
        })
    }

    /// Records one complete source frame.
    pub fn observe_frame(&mut self, arrival_ns: u64, timeline_index: u64) {
        let stalled = self.last_arrival_ns.is_some_and(|previous| {
            arrival_ns.saturating_sub(previous)
                >= (self.config.stall_threshold_ms * 1_000_000.0).round() as u64
        });
        if stalled {
            self.rate.reset_after_stall(arrival_ns);
        }
        self.rate.record_sample(arrival_ns, timeline_index);
        self.last_arrival_ns = Some(arrival_ns);
    }

    /// Starts or resumes presentation when the buffer has reached its refill threshold.
    ///
    /// `buffered_frames` is the number of frames currently held in the playback buffer.
    pub fn resume_if_ready(&mut self, now_ns: u64, buffered_frames: usize) -> bool {
        let required_frames = (self.refill_buffer_frames as usize).max(1);
        if !self.filling || buffered_frames < required_frames {
            return false;
        }
        self.filling = false;
        self.next_deadline_ns = Some(now_ns);
        self.stable_since_ns = Some(now_ns);
        self.transient_until_ns = (self.transient_buffer_frames > 0).then(|| {
            now_ns.saturating_add(
                (self.config.transient_recovery_seconds * 1_000_000_000.0).round() as u64,
            )
        });
        self.last_target_decrease_ns = None;
        true
    }

    /// Returns the next presentation deadline, if playback is active.
    pub fn next_deadline_ns(&self) -> Option<u64> {
        self.next_deadline_ns
    }

    /// Polls the controller and updates its next presentation deadline.
    pub fn poll(&mut self, now_ns: u64, buffered_frames: usize) -> PacingAction {
        let Some(deadline_ns) = self.next_deadline_ns else {
            return PacingAction::Wait;
        };
        if now_ns < deadline_ns {
            return PacingAction::Wait;
        }
        self.expire_transient_reserve(now_ns);
        if buffered_frames == 0 {
            let old_target = self.effective_target_buffer_frames();
            if self.transient_buffer_frames > 0 {
                self.stats.target_promotions += 1;
            }
            self.target_buffer_frames = self
                .target_buffer_frames
                .saturating_add(self.transient_buffer_frames)
                .min(self.config.max_buffer_frames);
            self.transient_buffer_frames = self
                .config
                .max_buffer_frames
                .saturating_sub(self.target_buffer_frames)
                .min(2);
            if self.transient_buffer_frames > 0 {
                self.stats.transient_recovery_events += 1;
            }
            let new_target = self.effective_target_buffer_frames();
            self.stats.underflow_events += 1;
            self.stats.rebuffer_events += 1;
            self.stats.target_increases += new_target.saturating_sub(old_target) as usize;
            self.refill_buffer_frames = self
                .effective_target_buffer_frames()
                .min(self.config.min_buffer_frames.max(4));
            self.filling = true;
            self.next_deadline_ns = None;
            self.stable_since_ns = None;
            self.transient_until_ns = None;
            self.last_target_decrease_ns = None;
            return PacingAction::Rebuffer {
                target_buffer_frames: new_target,
                refill_buffer_frames: self.refill_buffer_frames,
            };
        }

        let stable_grace_elapsed = self.stable_since_ns.is_some_and(|stable_since| {
            now_ns.saturating_sub(stable_since)
                >= (self.config.stable_grace_seconds * 1_000_000_000.0).round() as u64
        });
        let decay_interval_elapsed = self.last_target_decrease_ns.is_none_or(|last_decrease| {
            now_ns.saturating_sub(last_decrease)
                >= (self.config.target_decay_interval_seconds * 1_000_000_000.0).round() as u64
        });
        if self.target_buffer_frames > self.config.min_buffer_frames
            && stable_grace_elapsed
            && decay_interval_elapsed
        {
            self.target_buffer_frames -= 1;
            self.stats.target_decreases += 1;
            self.last_target_decrease_ns = Some(now_ns);
        }

        let occupancy_after_pop = buffered_frames.saturating_sub(1);
        let occupancy_error =
            occupancy_after_pop as isize - self.effective_target_buffer_frames() as isize;
        let occupancy_gain = if occupancy_error < 0 {
            self.config.occupancy_gain * self.config.low_buffer_gain_multiplier
        } else {
            self.config.occupancy_gain
        };
        let interval_ns = self.rate.interval_ns(
            occupancy_error,
            occupancy_gain,
            self.config.max_rate_adjustment,
        );
        self.next_deadline_ns = Some(now_ns.saturating_add(interval_ns));
        PacingAction::Present {
            presentation_ns: now_ns,
        }
    }

    /// Returns how many of the oldest buffered frames to present immediately, restoring the
    /// target buffer size.
    pub fn catch_up_frames(
        &self,
        now_ns: u64,
        buffered_frames: usize,
        oldest_arrival_ns: Option<u64>,
    ) -> usize {
        let Some(oldest_arrival_ns) = oldest_arrival_ns else {
            return 0;
        };
        let over_limit = now_ns.saturating_sub(oldest_arrival_ns)
            >= (self.config.hard_latency_ms * 1_000_000.0).round() as u64;
        if !over_limit {
            return 0;
        }
        buffered_frames.saturating_sub(self.effective_target_buffer_frames() as usize)
    }

    /// Records a catch-up operation.
    pub fn record_catch_up(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        self.stats.catch_up_events += 1;
        self.stats.catch_up_frames += frames;
    }

    /// Returns the current buffer target.
    pub fn target_buffer_frames(&self) -> u32 {
        self.effective_target_buffer_frames()
    }

    /// Returns the buffer size required to start or resume playback.
    pub fn refill_buffer_frames(&self) -> u32 {
        self.refill_buffer_frames
    }

    /// Returns the current rolling source-rate estimate.
    pub fn estimated_fps(&self) -> f64 {
        self.rate.fps
    }

    /// Returns whether presentation is waiting for the buffer to reach its target size.
    pub fn is_filling(&self) -> bool {
        self.filling
    }

    /// Returns the controller statistics collected so far.
    pub fn stats(&self) -> PacingStats {
        self.stats
    }

    /// Re-enters buffer filling after a dependency-safe resync.
    pub fn force_refill(&mut self) {
        self.refill_buffer_frames = self
            .effective_target_buffer_frames()
            .min(self.config.min_buffer_frames.max(4));
        self.filling = true;
        self.next_deadline_ns = None;
        self.stable_since_ns = None;
        self.transient_until_ns = None;
        self.last_target_decrease_ns = None;
    }

    fn effective_target_buffer_frames(&self) -> u32 {
        self.target_buffer_frames
            .saturating_add(self.transient_buffer_frames)
            .min(self.config.max_buffer_frames)
    }

    fn expire_transient_reserve(&mut self, now_ns: u64) {
        if self
            .transient_until_ns
            .is_none_or(|transient_until_ns| now_ns < transient_until_ns)
        {
            return;
        }
        self.stats.target_decreases += self.transient_buffer_frames as usize;
        self.transient_buffer_frames = 0;
        self.transient_until_ns = None;
    }
}

#[derive(Debug)]
struct RateEstimator {
    window_ns: u64,
    samples: VecDeque<(u64, u64)>,
    fps: f64,
    max_fps: f64,
    hold_until_ns: Option<u64>,
}

impl RateEstimator {
    fn new(initial_fps: f64, window_seconds: f64) -> Self {
        Self {
            window_ns: (window_seconds * 1_000_000_000.0).round() as u64,
            samples: VecDeque::new(),
            fps: initial_fps,
            max_fps: initial_fps,
            hold_until_ns: None,
        }
    }

    fn reset_after_stall(&mut self, arrival_ns: u64) {
        self.samples.clear();
        self.hold_until_ns = Some(arrival_ns.saturating_add(self.window_ns));
    }

    fn record_sample(&mut self, arrival_ns: u64, timeline_index: u64) {
        self.samples.push_back((arrival_ns, timeline_index));
        let cutoff = arrival_ns.saturating_sub(self.window_ns);
        while self.samples.len() >= 2 && self.samples[1].0 <= cutoff {
            self.samples.pop_front();
        }

        if self
            .hold_until_ns
            .is_some_and(|hold_until_ns| arrival_ns < hold_until_ns)
        {
            return;
        }
        self.hold_until_ns = None;

        let Some(&(first_ns, first_index)) = self.samples.front() else {
            return;
        };
        let elapsed_ns = arrival_ns.saturating_sub(first_ns);
        let minimum_span_ns = (self.window_ns / 4).min(500_000_000);
        let frame_span = timeline_index.saturating_sub(first_index);
        if elapsed_ns >= minimum_span_ns && frame_span >= 2 {
            let estimate = frame_span as f64 * 1_000_000_000.0 / elapsed_ns as f64;
            if (5.0..=240.0).contains(&estimate) {
                self.fps = estimate.min(self.max_fps);
            }
        }
    }

    fn interval_ns(
        &self,
        occupancy_error: isize,
        occupancy_gain: f64,
        max_rate_adjustment: f64,
    ) -> u64 {
        let correction = (occupancy_error as f64 * occupancy_gain)
            .clamp(-max_rate_adjustment, max_rate_adjustment);
        ((1_000_000_000.0 / self.fps) * (1.0 - correction)).round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pacing_config(initial_fps: f64, min_buffer_frames: u32) -> PacingConfig {
        PacingConfig {
            initial_fps,
            rate_window_seconds: 2.0,
            min_buffer_frames,
            max_buffer_frames: 10,
            stall_threshold_ms: 100.0,
            occupancy_gain: 0.03,
            max_rate_adjustment: 0.10,
            low_buffer_gain_multiplier: 2.0,
            transient_recovery_seconds: 1.0,
            stable_grace_seconds: 3.0,
            target_decay_interval_seconds: 1.0,
            hard_latency_ms: 500.0,
        }
    }

    #[test]
    fn adaptive_pacer_holds_and_grows_target_after_underflow() {
        let mut pacer = AdaptivePacer::new(pacing_config(60.0, 2)).expect("create pacer");
        pacer.observe_frame(0, 0);
        pacer.observe_frame(16_000_000, 1);
        assert!(pacer.resume_if_ready(16_000_000, 2));
        assert!(matches!(
            pacer.poll(16_000_000, 2),
            PacingAction::Present { .. }
        ));
        let deadline = pacer.next_deadline_ns().expect("next deadline");

        assert_eq!(
            pacer.poll(deadline, 0),
            PacingAction::Rebuffer {
                target_buffer_frames: 4,
                refill_buffer_frames: 4
            }
        );
        assert!(pacer.is_filling());
        assert!(!pacer.resume_if_ready(deadline, 3));
        assert!(pacer.resume_if_ready(deadline, 4));
        assert_eq!(pacer.stats().transient_recovery_events, 1);
        assert_eq!(pacer.stats().target_promotions, 0);
    }

    #[test]
    fn isolated_underflow_reserve_expires_quickly() {
        let mut pacer = AdaptivePacer::new(pacing_config(60.0, 2)).expect("create pacer");
        pacer.resume_if_ready(0, 2);
        pacer.poll(0, 1);
        let deadline = pacer.next_deadline_ns().expect("next deadline");
        pacer.poll(deadline, 0);
        assert_eq!(pacer.target_buffer_frames(), 4);
        pacer.resume_if_ready(deadline, 4);

        let stable = deadline + 1_100_000_000;
        assert!(matches!(
            pacer.poll(stable, 4),
            PacingAction::Present { .. }
        ));
        assert_eq!(pacer.target_buffer_frames(), 2);
    }

    #[test]
    fn adaptive_target_uses_configured_grace_and_decay_interval() {
        let mut config = pacing_config(60.0, 2);
        config.stable_grace_seconds = 5.0;
        config.target_decay_interval_seconds = 3.0;
        let mut pacer = AdaptivePacer::new(config).expect("create pacer");
        assert!(pacer.resume_if_ready(0, 2));
        pacer.poll(0, 1);
        let first_deadline = pacer.next_deadline_ns().expect("first deadline");
        pacer.poll(first_deadline, 0);
        assert_eq!(pacer.target_buffer_frames(), 4);
        assert!(pacer.resume_if_ready(first_deadline, 4));
        pacer.poll(first_deadline, 1);
        let second_deadline = pacer.next_deadline_ns().expect("second deadline");
        pacer.poll(second_deadline, 0);
        assert_eq!(pacer.target_buffer_frames(), 6);
        assert!(pacer.resume_if_ready(second_deadline, 4));

        pacer.poll(second_deadline + 1_100_000_000, 4);
        assert_eq!(pacer.target_buffer_frames(), 4);
        pacer.poll(second_deadline + 4_999_000_000, 4);
        assert_eq!(pacer.target_buffer_frames(), 4);
        pacer.poll(second_deadline + 5_100_000_000, 4);
        assert_eq!(pacer.target_buffer_frames(), 3);
        pacer.poll(second_deadline + 8_099_000_000, 3);
        assert_eq!(pacer.target_buffer_frames(), 3);
        pacer.poll(second_deadline + 8_200_000_000, 3);
        assert_eq!(pacer.target_buffer_frames(), 2);
    }

    #[test]
    fn adaptive_pacer_resumes_before_reaching_a_large_target() {
        let mut pacer = AdaptivePacer::new(pacing_config(60.0, 2)).expect("create pacer");
        assert!(pacer.resume_if_ready(0, 2));
        pacer.poll(0, 1);
        let first_deadline = pacer.next_deadline_ns().expect("first deadline");
        pacer.poll(first_deadline, 0);
        assert!(pacer.resume_if_ready(first_deadline, 4));
        pacer.poll(first_deadline, 1);
        let second_deadline = pacer.next_deadline_ns().expect("second deadline");

        assert_eq!(
            pacer.poll(second_deadline, 0),
            PacingAction::Rebuffer {
                target_buffer_frames: 6,
                refill_buffer_frames: 4
            }
        );
        assert!(!pacer.resume_if_ready(second_deadline, 3));
        assert!(pacer.resume_if_ready(second_deadline, 4));
        assert_eq!(pacer.target_buffer_frames(), 6);
        assert_eq!(pacer.refill_buffer_frames(), 4);
        assert_eq!(pacer.stats().transient_recovery_events, 2);
        assert_eq!(pacer.stats().target_promotions, 1);
    }

    #[test]
    fn hard_latency_compresses_only_frames_above_target() {
        let mut pacer = AdaptivePacer::new(pacing_config(60.0, 2)).expect("create pacer");

        assert_eq!(pacer.catch_up_frames(499_000_000, 8, Some(0)), 0);
        assert_eq!(pacer.catch_up_frames(500_000_000, 8, Some(0)), 6);
        pacer.record_catch_up(6);
        assert_eq!(pacer.stats().catch_up_events, 1);
        assert_eq!(pacer.stats().catch_up_frames, 6);
    }

    #[test]
    fn rolling_rate_estimator_holds_and_caps_after_long_stalls() {
        let mut config = pacing_config(60.0, 2);
        config.rate_window_seconds = 0.5;
        let mut pacer = AdaptivePacer::new(config).expect("create pacer");
        for index in 0..=7 {
            pacer.observe_frame(index * 20_000_000, index);
        }
        assert!((49.0..=51.0).contains(&pacer.estimated_fps()));

        pacer.observe_frame(500_000_000, 8);
        assert!((49.0..=51.0).contains(&pacer.estimated_fps()));
        for index in 9..=23 {
            pacer.observe_frame(500_000_000 + (index - 8) * 10_000_000, index);
        }
        assert!((49.0..=51.0).contains(&pacer.estimated_fps()));

        for index in 24..=68 {
            pacer.observe_frame(500_000_000 + (index - 8) * 10_000_000, index);
        }
        assert!((59.0..=60.0).contains(&pacer.estimated_fps()));
    }
}
