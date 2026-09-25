//! Following the Cardputer's clock.
//!
//! The Cardputer's microphone and this computer's audio output each run on
//! their own clock. When they differ, say the microphone is 0.6 % slow and
//! delivers 15 904 samples a second, a queue played at exactly 16 000 runs dry
//! over and over, and every underrun is a gap. So playback speed follows the
//! queue instead: a little slower while it holds less than its target, a
//! little faster while it holds more. The integral term learns the steady
//! difference between the clocks, so the queue settles at its target.
//!
//! Following the device's clock also puts its pitch right: a microphone that
//! samples 1 % slow, played at 16 000, sounds 1 % high. Up to 3 % is followed;
//! a clock off by more than that still underruns, but far less often.
//!
//! The gains give a damped loop (damping about 0.9, settling in well under a
//! minute) that learns a 0.6 % difference exactly, and moves the speed by
//! only a few hundredths of a percent under 50 ms of network jitter.

/// Largest speed change, either way.
pub const MAX_TRIM: f64 = 0.03;
/// Speed change per second of error (buffered below or above the target).
const KP: f64 = 0.3;
/// Speed change per second of error, per second it lasts.
const KI: f64 = 0.03;
/// Time constant of the averaged queue level, in seconds: packets arrive in
/// 20 ms steps and the output takes them in callbacks, so the raw level
/// jitters by tens of ms.
const SMOOTHING: f64 = 1.0;

#[derive(Debug, Clone, Default)]
pub struct Drift {
    level: Option<f64>,
    integral: f64,
}

impl Drift {
    pub fn new() -> Drift {
        Drift::default()
    }

    /// Start averaging the level afresh (after a resync the queue is empty),
    /// but keep what was learned about the clocks: it is the same device.
    pub fn restart(&mut self) {
        self.level = None;
    }

    /// Forget the device's clock too: another Cardputer.
    pub fn reset(&mut self) {
        *self = Drift::default();
    }

    /// The speed to play the next `dt` seconds of input at, given what the
    /// queue holds and aims for (seconds) and whether it is playing yet.
    pub fn speed(&mut self, buffered: f64, target: f64, dt: f64, playing: bool) -> f64 {
        let a = (dt / SMOOTHING).min(1.0);
        let level = self.level.map_or(buffered, |l| l + (buffered - l) * a);
        self.level = Some(level);
        if !playing {
            // Filling up before playback: no error to act on yet.
            return 1.0 - (KI * self.integral).clamp(-MAX_TRIM, MAX_TRIM);
        }
        let error = target - level; // above zero: too little buffered, play slower
        self.integral = (self.integral + error * dt).clamp(-MAX_TRIM / KI, MAX_TRIM / KI);
        1.0 - (KP * error + KI * self.integral).clamp(-MAX_TRIM, MAX_TRIM)
    }

    /// How far the device's clock seems to be from this computer's, as a
    /// fraction: negative when the device is slow.
    pub fn estimate(&self) -> f64 {
        -(KI * self.integral).clamp(-MAX_TRIM, MAX_TRIM)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device sending `rate` samples a second (nominally 16 000) into a
    /// queue played at 16 000, `seconds` long, with packets arriving up to
    /// `jitter` seconds late. Returns (underruns, levels over the last minute).
    fn run(rate: f64, seconds: f64, jitter: f64) -> (u32, Vec<f64>) {
        let (target, packet) = (0.060, 320.0);
        let interval = packet / rate; // seconds between packets
        let mut drift = Drift::new();
        let mut level = 0.0f64; // seconds of audio queued
        let mut playing = false;
        let (mut underruns, mut levels) = (0u32, Vec::new());
        let mut seed = 0x2545_F491u32;
        let mut prev_late = 0.0;
        let mut t = 0.0;
        while t < seconds {
            // Deterministic jitter: xorshift, 0..jitter seconds late.
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let late = jitter * (seed % 1000) as f64 / 1000.0;
            let speed = drift.speed(level, target, packet / 16_000.0, playing);
            level += packet / 16_000.0 / speed; // output made from this packet
            if !playing && level >= target {
                playing = true;
            }
            // A packet that is late shortens the wait for the next one: arrival
            // times wobble around the schedule, they do not drift from it.
            let late = if t > 1.0 { late } else { 0.0 };
            let elapsed = interval + late - prev_late;
            prev_late = late;
            if playing {
                level -= elapsed; // the output plays in real time
                if level < 0.0 {
                    level = 0.0;
                    playing = false;
                    underruns += 1;
                }
            }
            t += interval;
            if t > seconds - 60.0 {
                levels.push(level);
            }
        }
        (underruns, levels)
    }

    #[test]
    fn a_slow_clock_is_followed_without_gaps() {
        // 0.6 % slow, as the original Cardputer's report suggested.
        let (underruns, levels) = run(15_904.0, 180.0, 0.0);
        assert_eq!(underruns, 0);
        let mean = levels.iter().sum::<f64>() / levels.len() as f64;
        assert!((mean - 0.060).abs() < 0.010, "settles near the target: {mean}");
    }

    #[test]
    fn a_fast_clock_is_followed_too() {
        let (underruns, levels) = run(16_096.0, 180.0, 0.0);
        assert_eq!(underruns, 0);
        let mean = levels.iter().sum::<f64>() / levels.len() as f64;
        assert!((mean - 0.060).abs() < 0.010, "settles near the target: {mean}");
    }

    #[test]
    fn network_jitter_does_not_swing_the_speed() {
        // Exact clock, packets up to 30 ms late: the speed stays within a
        // fraction of the limit, and the learned clock difference stays small.
        let mut drift = Drift::new();
        let (_, levels) = run(16_000.0, 120.0, 0.030);
        assert!(levels.iter().all(|&l| l > 0.0));
        for i in 0..6000 {
            let wobble = if i % 2 == 0 { 0.015 } else { -0.015 };
            drift.speed(0.060 + wobble, 0.060, 0.02, true);
        }
        assert!(drift.estimate().abs() < MAX_TRIM / 4.0, "{}", drift.estimate());
    }

    #[test]
    fn it_learns_the_clock_difference() {
        let mut drift = Drift::new();
        let mut level = 0.060f64;
        for _ in 0..(180.0 / 0.020) as usize {
            let speed = drift.speed(level, 0.060, 0.020, true);
            level += 0.020 / speed - 0.020 * 16_000.0 / 15_904.0;
        }
        assert!((drift.estimate() + 0.006).abs() < 0.001, "about -0.6 %: {} (level {level})", drift.estimate());
    }
}
