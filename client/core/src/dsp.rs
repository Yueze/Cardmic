//! Sample-rate conversion and channel mapping.
//!
//! The client must open its output stream at the *device's* native rate and
//! channel count, never impose 16 kHz mono. cpal's CoreAudio backend sets the
//! physical format on the hardware device, so opening a 16 kHz stream on a
//! shared loopback device changes its rate system-wide and disrupts every
//! other app capturing from it.
//!
//! So the device's 16 kHz mono stream is converted here, in pure code that
//! can be tested without any audio hardware.

/// Convert signed 16-bit PCM to `f32` in `[-1.0, 1.0)`.
pub fn i16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| s as f32 / 32768.0).collect()
}

/// Streaming linear-interpolation resampler for a mono signal.
///
/// Linear interpolation is adequate for voice upsampling (16 kHz to 44.1 or
/// 48 kHz): upsampling does not alias, and the imaging it leaves above 8 kHz
/// sits outside the voice band a dictation app cares about. It is dependency
/// free and trivially correct, which matters more for v1 than the last few dB.
///
/// State is carried across calls. Feeding one 20 ms packet at a time produces
/// exactly the same output as feeding the whole stream at once, with no
/// discontinuity at packet boundaries — the failure mode of the naive
/// per-chunk approach is an audible click every 20 ms.
#[derive(Debug, Clone)]
pub struct Resampler {
    /// Input samples consumed per output sample (`from_rate / to_rate`).
    step: f64,
    /// Position of the next output sample, measured in input samples from
    /// `prev`. Always in `[0, 1)` between calls.
    phase: f64,
    /// Last input sample of the previous call, the left anchor for
    /// interpolating across the boundary.
    prev: f32,
    primed: bool,
}

impl Resampler {
    /// # Panics
    /// If either rate is zero.
    pub fn new(from_rate: u32, to_rate: u32) -> Self {
        assert!(from_rate > 0 && to_rate > 0, "sample rates must be non-zero");
        Resampler { step: from_rate as f64 / to_rate as f64, phase: 0.0, prev: 0.0, primed: false }
    }

    /// Resample a chunk, appending the output to `out`.
    ///
    /// The most recent input sample is held back as the left anchor for the
    /// next call, because interpolating up to it needs the sample after it.
    /// So a single call emits output up to, but not including, its last input
    /// sample; that sample is emitted on the next call. This is what makes
    /// chunked and one-shot processing produce identical output.
    pub fn process(&mut self, mut input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if !self.primed {
            // Consume the first sample as the anchor. It must not also be
            // treated as the next input, or it is emitted twice and the whole
            // stream lags by one sample.
            self.prev = input[0];
            self.primed = true;
            input = &input[1..];
            if input.is_empty() {
                return;
            }
        }

        // Treat `prev` as virtual index 0 and input[k] as virtual index k+1.
        let len = input.len() as f64;
        let at = |i: usize| if i == 0 { self.prev } else { input[i - 1] };

        while self.phase < len {
            let left = self.phase.floor() as usize;
            let frac = (self.phase - left as f64) as f32;
            let a = at(left);
            let b = at(left + 1);
            out.push(a + (b - a) * frac);
            self.phase += self.step;
        }

        self.phase -= len;
        self.prev = *input.last().unwrap();
    }

    /// Forget stream history, e.g. after a resync or reconnect.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.prev = 0.0;
        self.primed = false;
    }
}

/// Replicate a mono signal across `channels` interleaved channels.
///
/// Loopback devices come in many widths — BlackHole ships as 2, 16, 64, 128
/// and 256 channel devices, and VB-CABLE has a 16 channel variant. The
/// prototype hard-failed on anything but 1 or 2; this handles any N.
///
/// # Panics
/// If `channels` is zero.
pub fn mono_to_interleaved(mono: &[f32], channels: usize, out: &mut Vec<f32>) {
    assert!(channels > 0, "channel count must be non-zero");
    out.reserve(mono.len() * channels);
    for &s in mono {
        out.extend(std::iter::repeat_n(s, channels));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * hz * std::f32::consts::TAU / rate as f32).sin()).collect()
    }

    #[test]
    fn i16_conversion_spans_the_range() {
        assert_eq!(i16_to_f32(&[0, i16::MIN]), vec![0.0, -1.0]);
        assert!(i16_to_f32(&[i16::MAX])[0] < 1.0);
    }

    #[test]
    fn output_length_tracks_the_rate_ratio() {
        // One input sample is held back as the anchor for the next call, so a
        // single call over N inputs spans N-1 input intervals.
        for to in [44_100u32, 48_000] {
            let n = 16_000usize; // one second
            let mut r = Resampler::new(16_000, to);
            let mut out = Vec::new();
            r.process(&vec![0.0; n], &mut out);
            let expected = ((n - 1) as f64 * to as f64 / 16_000.0).ceil() as i64;
            let diff = (out.len() as i64 - expected).abs();
            assert!(diff <= 1, "16k->{to}: got {}, expected {expected}", out.len());
        }
    }

    #[test]
    fn chunked_output_equals_one_shot_output() {
        // The property that rules out a click every 20 ms.
        let input = sine(16_000, 440.0, 320 * 25);

        let mut whole = Vec::new();
        Resampler::new(16_000, 48_000).process(&input, &mut whole);

        let mut chunked = Vec::new();
        let mut r = Resampler::new(16_000, 48_000);
        for packet in input.chunks(320) {
            r.process(packet, &mut chunked);
        }

        assert_eq!(chunked.len(), whole.len());
        let worst = whole.iter().zip(&chunked).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-5, "chunking changed the signal by {worst}");
    }

    #[test]
    fn chunk_boundaries_are_continuous_at_awkward_ratios() {
        // 44.1 kHz is not an integer multiple of 16 kHz, so the phase carried
        // across boundaries is fractional. Check no sample jumps at a seam.
        let input = sine(16_000, 300.0, 320 * 40);
        let mut out = Vec::new();
        let mut r = Resampler::new(16_000, 44_100);
        for packet in input.chunks(320) {
            r.process(packet, &mut out);
        }
        // A 300 Hz sine at 44.1 kHz moves at most ~0.043 per sample.
        let max_step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(max_step < 0.05, "discontinuity of {max_step} between samples");
    }

    #[test]
    fn preserves_amplitude() {
        let input = sine(16_000, 440.0, 16_000);
        let mut out = Vec::new();
        Resampler::new(16_000, 48_000).process(&input, &mut out);
        let peak = out.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!((peak - 1.0).abs() < 0.02, "peak drifted to {peak}");
    }

    #[test]
    fn identity_rate_passes_through_without_lag() {
        // At 1:1 the output must equal the input sample-for-sample, less the
        // one sample held back for the next call. Guards against the priming
        // bug where the first sample was emitted twice and the stream lagged.
        let input = sine(16_000, 440.0, 640);
        let mut out = Vec::new();
        Resampler::new(16_000, 16_000).process(&input, &mut out);
        assert_eq!(out.len(), input.len() - 1, "exactly one sample is held back");
        let worst = input.iter().zip(&out).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-6, "output lags or duplicates the input (worst {worst})");
    }

    #[test]
    fn held_back_sample_is_emitted_on_the_next_call() {
        let mut r = Resampler::new(16_000, 16_000);
        let mut out = Vec::new();
        r.process(&[0.1, 0.2, 0.3], &mut out);
        assert_eq!(out, vec![0.1, 0.2]);
        r.process(&[0.4], &mut out);
        assert_eq!(out, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn replicates_mono_across_any_channel_count() {
        for channels in [1usize, 2, 16, 64] {
            let mut out = Vec::new();
            mono_to_interleaved(&[0.25, -0.5], channels, &mut out);
            assert_eq!(out.len(), 2 * channels);
            assert!(out[..channels].iter().all(|&s| s == 0.25));
            assert!(out[channels..].iter().all(|&s| s == -0.5));
        }
    }
}
