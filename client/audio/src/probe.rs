//! Loopback verification: the `cardmic doctor` primitive.
//!
//! Plays a tone into a device's output side and measures RMS on the same
//! device's input side. A device that merely appears on both sides is not
//! necessarily a loopback — on the author's Mac, `BYOM-Microphone` is present
//! on both sides and silent, and the prototype hit the same false positive
//! with an Oculus device. Only a measurement settles it.

use crate::{find_input, find_output, Candidate};
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Tone used for the probe. 997 Hz is prime-ish against common sample rates,
/// so it does not land on an exact bin and hide a broken path.
const TONE_HZ: f32 = 997.0;
const TONE_AMPLITUDE: f32 = 0.5;
const PROBE_DURATION: Duration = Duration::from_millis(700);

/// RMS above which a device is considered a working loopback. A 0.5-amplitude
/// sine has an RMS of 0.354; real loopbacks measure ~0.34. Silence measures 0.
pub const LOOPBACK_THRESHOLD: f32 = 0.01;

/// Result of probing one device.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Tone came back. Carries the measured RMS.
    Loops(f32),
    /// Device is present on both sides but the tone did not come back.
    Silent(f32),
    /// The probe could not run.
    Error(String),
}

impl Verdict {
    pub fn is_loopback(&self) -> bool {
        matches!(self, Verdict::Loops(_))
    }
}

/// Play a tone into the candidate's output side and measure RMS on its input side.
pub fn loopback_rms(host: &cpal::Host, candidate: &Candidate) -> Verdict {
    match measure(host, &candidate.output, &candidate.input) {
        Ok(rms) if rms > LOOPBACK_THRESHOLD => Verdict::Loops(rms),
        Ok(rms) => Verdict::Silent(rms),
        Err(e) => Verdict::Error(e),
    }
}

fn measure(host: &cpal::Host, output: &str, input: &str) -> Result<f32, String> {
    let out = find_output(host, output).ok_or("no output side")?;
    let inp = find_input(host, input).ok_or("no input side")?;

    let out_cfg = out.default_output_config().map_err(|e| format!("output config: {e}"))?;
    let in_cfg = inp.default_input_config().map_err(|e| format!("input config: {e}"))?;
    let out_rate = out_cfg.sample_rate() as f32;
    let out_channels = out_cfg.channels() as usize;

    // Sum of squares, stored as f64 bits so it can live in an atomic and the
    // input callback never takes a lock.
    let sum_sq = Arc::new(AtomicU64::new(0f64.to_bits()));
    let count = Arc::new(AtomicU64::new(0));
    let (sum_cb, count_cb) = (sum_sq.clone(), count.clone());

    let input = inp
        .build_input_stream(
            in_cfg.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let chunk: f64 = data.iter().map(|&v| (v as f64) * (v as f64)).sum();
                let mut cur = sum_cb.load(Ordering::Relaxed);
                loop {
                    let next = (f64::from_bits(cur) + chunk).to_bits();
                    match sum_cb.compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed) {
                        Ok(_) => break,
                        Err(actual) => cur = actual,
                    }
                }
                count_cb.fetch_add(data.len() as u64, Ordering::Relaxed);
            },
            |e| eprintln!("probe input stream error: {e}"),
            None,
        )
        .map_err(|e| format!("build input stream: {e}"))?;

    let mut phase = 0.0f32;
    let output = out
        .build_output_stream(
            out_cfg.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(out_channels) {
                    let v = (phase * std::f32::consts::TAU).sin() * TONE_AMPLITUDE;
                    phase = (phase + TONE_HZ / out_rate).fract();
                    frame.fill(v);
                }
            },
            |e| eprintln!("probe output stream error: {e}"),
            None,
        )
        .map_err(|e| format!("build output stream: {e}"))?;

    // cpal 0.18: streams do not auto-start.
    input.play().map_err(|e| format!("start input: {e}"))?;
    output.play().map_err(|e| format!("start output: {e}"))?;
    std::thread::sleep(PROBE_DURATION);
    drop(output);
    drop(input);

    let n = count.load(Ordering::Relaxed);
    if n == 0 {
        return Err("captured no samples".into());
    }
    Ok((f64::from_bits(sum_sq.load(Ordering::Relaxed)) / n as f64).sqrt() as f32)
}
