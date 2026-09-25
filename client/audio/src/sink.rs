//! Playback into a loopback device.
//!
//! The output stream is always opened at the device's *native* rate and
//! channel count. cpal's CoreAudio backend sets the physical format on the
//! hardware, so imposing 16 kHz mono on a shared loopback device would change
//! its rate system-wide and disrupt every other app using it. The
//! caller resamples to [`OutputSink::sample_rate`] before pushing.

use crate::{device_name, find_output};
use cpal::traits::{DeviceTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Jitter buffer. Playback starts once `target` audio is queued, and the
// target adapts to the network: every underrun raises it one step, up to a
// ceiling, and a long enough stretch without one lowers it again, down to a
// floor. A quiet LAN stays at the floor. A host whose Wi-Fi pauses
// periodically settles at a size that absorbs the pauses: a Mac on Wi-Fi
// measured 100-115 ms gaps about once a second (a Windows PC on the same
// network: none over 100 ms) and settles at 160 ms within seconds.

/// Starting target, and the floor. Packets carry 20 ms each, so this is
/// three packets: one playing, one arriving, one in hand.
const START_TARGET: Duration = Duration::from_millis(60);
/// After this long without an underrun, the target comes down a notch.
const CALM: Duration = Duration::from_secs(15);
const SHRINK_STEP: Duration = Duration::from_millis(20);
/// Added to the target after each underrun.
const TARGET_STEP: Duration = Duration::from_millis(40);
/// The target never grows past this, so latency stays usable for calls. A
/// busy venue Wi-Fi measured 83-200 ms round trips and still underran at
/// 240 ms, so the ceiling leaves room for that.
const MAX_TARGET: Duration = Duration::from_millis(400);
/// Audio beyond `target + HEADROOM` is trimmed back to `target`. The device
/// and host clocks drift apart (tens of ppm), so without a cap latency would
/// creep up for as long as the session runs. Trimming to the target, not
/// below it, keeps the cushion that absorbs the next gap.
const HEADROOM: Duration = Duration::from_millis(150);

fn samples_for(d: Duration, rate: u32) -> usize {
    (d.as_secs_f64() * rate as f64) as usize
}

struct QueueState {
    buf: VecDeque<f32>,
    /// Playback has buffered enough to start. Cleared on underrun, so the
    /// queue rebuilds its cushion instead of stuttering sample by sample.
    playing: bool,
    target: usize,
    step: usize,
    min_target: usize,
    max_target: usize,
    headroom: usize,
    /// Samples played since the last underrun (or the last shrink).
    calm_played: usize,
    calm: usize,
    shrink: usize,
}

/// Mono samples at the device rate, shared between the network thread
/// (producer) and the audio callback (consumer).
///
/// The audio callback must never block, so it uses `try_lock` and outputs
/// silence if the producer happens to hold the lock. The producer holds it
/// only for short, bounded copies.
pub struct SampleQueue {
    state: Mutex<QueueState>,
    underruns: AtomicU64,
    trimmed: AtomicU64,
}

impl SampleQueue {
    fn new(rate: u32) -> Self {
        SampleQueue {
            state: Mutex::new(QueueState {
                buf: VecDeque::with_capacity(samples_for(MAX_TARGET + HEADROOM, rate) * 2),
                playing: false,
                target: samples_for(START_TARGET, rate),
                step: samples_for(TARGET_STEP, rate),
                min_target: samples_for(START_TARGET, rate),
                max_target: samples_for(MAX_TARGET, rate),
                headroom: samples_for(HEADROOM, rate),
                calm_played: 0,
                calm: samples_for(CALM, rate),
                shrink: samples_for(SHRINK_STEP, rate),
            }),
            underruns: AtomicU64::new(0),
            trimmed: AtomicU64::new(0),
        }
    }

    /// Append audio. If the queue would exceed `target + headroom`, the
    /// oldest audio is dropped back down to the target.
    pub fn push(&self, samples: &[f32]) {
        let mut s = self.state.lock().unwrap();
        s.buf.extend(samples.iter().copied());
        if s.buf.len() > s.target + s.headroom {
            let excess = s.buf.len() - s.target;
            s.buf.drain(..excess);
            self.trimmed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Append `n` samples of silence, to conceal lost packets.
    pub fn push_silence(&self, n: usize) {
        let mut s = self.state.lock().unwrap();
        s.buf.extend(std::iter::repeat_n(0.0, n));
    }

    /// Drop everything buffered, e.g. on resync or reconnect.
    pub fn clear(&self) {
        let mut s = self.state.lock().unwrap();
        s.buf.clear();
        s.playing = false;
    }

    /// Audio currently buffered.
    pub fn buffered(&self) -> usize {
        self.state.lock().unwrap().buf.len()
    }

    /// Times playback ran dry and had to rebuffer.
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Times the latency cap trimmed old audio.
    pub fn trims(&self) -> u64 {
        self.trimmed.load(Ordering::Relaxed)
    }

    /// Current jitter-buffer target, in samples.
    pub fn target(&self) -> usize {
        self.state.lock().unwrap().target
    }

    /// Buffered audio, the target (both in samples), and whether playback is
    /// running (past its pre-roll), read together.
    pub fn level(&self) -> (usize, usize, bool) {
        let s = self.state.lock().unwrap();
        (s.buf.len(), s.target, s.playing)
    }

    /// Fill `out` (interleaved, `channels` wide) from the queue. Called from
    /// the audio callback; never blocks.
    fn fill(&self, out: &mut [f32], channels: usize) {
        let Ok(mut s) = self.state.try_lock() else {
            out.fill(0.0);
            return;
        };
        if !s.playing && s.buf.len() >= s.target {
            s.playing = true;
        }
        if !s.playing {
            out.fill(0.0);
            return;
        }
        for frame in out.chunks_mut(channels) {
            match s.buf.pop_front() {
                Some(v) => {
                    frame.fill(v);
                    s.calm_played += 1;
                }
                None => {
                    frame.fill(0.0);
                    if s.playing {
                        s.playing = false;
                        s.target = (s.target + s.step).min(s.max_target);
                        s.calm_played = 0;
                        self.underruns.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        // Steady for a while: lower the target, and drop the audio above it
        // (a few ms, once) so latency actually comes down with it.
        if s.calm_played >= s.calm && s.target > s.min_target {
            s.target = s.target.saturating_sub(s.shrink).max(s.min_target);
            s.calm_played = 0;
            let keep = s.target + s.shrink;
            if s.buf.len() > keep {
                let excess = s.buf.len() - keep;
                s.buf.drain(..excess);
            }
        }
    }
}

/// A running output stream into a named device.
pub struct OutputSink {
    _stream: cpal::Stream,
    queue: Arc<SampleQueue>,
    needs_rebuild: Arc<AtomicBool>,
    rate: u32,
    channels: u16,
    name: String,
}

impl OutputSink {
    /// Open `name` at its native format and start playback.
    pub fn open(host: &cpal::Host, name: &str) -> Result<Self, String> {
        let device = find_output(host, name).ok_or_else(|| format!("no output device named {name:?}"))?;
        let config = device.default_output_config().map_err(|e| format!("output config: {e}"))?;
        let rate = config.sample_rate();
        let channels = config.channels();

        let queue = Arc::new(SampleQueue::new(rate));
        let needs_rebuild = Arc::new(AtomicBool::new(false));
        let (q, flag) = (queue.clone(), needs_rebuild.clone());
        let width = channels as usize;

        let stream = device
            .build_output_stream(
                config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| q.fill(data, width),
                move |err: cpal::Error| {
                    use cpal::ErrorKind::*;
                    match err.kind() {
                        // Another app changed the device's rate, or the device
                        // went away. The stream is dead; the owner must rebuild.
                        StreamInvalidated | DeviceChanged | DeviceNotAvailable => {
                            flag.store(true, Ordering::Relaxed);
                        }
                        Xrun => {}
                        _ => eprintln!("output stream error: {err}"),
                    }
                },
                None,
            )
            .map_err(|e| format!("build output stream on {name:?}: {e}"))?;

        // cpal 0.18: streams do not auto-start.
        stream.play().map_err(|e| format!("start output stream: {e}"))?;

        Ok(OutputSink {
            _stream: stream,
            queue,
            needs_rebuild,
            rate,
            channels,
            name: device_name(&device),
        })
    }

    /// Queue the caller pushes resampled audio into.
    pub fn queue(&self) -> Arc<SampleQueue> {
        self.queue.clone()
    }

    /// Device's native sample rate; resample to this before pushing.
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// True once the stream has been invalidated and must be reopened.
    pub fn needs_rebuild(&self) -> bool {
        self.needs_rebuild.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn start() -> usize {
        samples_for(START_TARGET, RATE)
    }

    /// Plays everything queued plus a little more, forcing one underrun.
    fn drain_past_empty(q: &SampleQueue) {
        let mut out = vec![0.0; (q.buffered() + 10) * 2];
        q.fill(&mut out, 2);
    }

    #[test]
    fn stays_silent_until_target_is_buffered() {
        let q = SampleQueue::new(RATE);
        q.push(&vec![0.5; start() - 1]);
        let mut out = vec![9.0; 8];
        q.fill(&mut out, 2);
        assert!(out.iter().all(|&s| s == 0.0), "must not play before the target fills");
    }

    #[test]
    fn plays_once_buffered_and_replicates_channels() {
        let q = SampleQueue::new(RATE);
        q.push(&vec![0.25; start()]);
        let mut out = vec![0.0; 6];
        q.fill(&mut out, 3);
        assert_eq!(out, vec![0.25; 6]);
    }

    #[test]
    fn underrun_rebuffers_instead_of_stuttering() {
        let q = SampleQueue::new(RATE);
        q.push(&vec![0.5; start()]);
        drain_past_empty(&q);
        assert_eq!(q.underruns(), 1);
        // A trickle below the target must not restart playback.
        q.push(&[0.5; 4]);
        let mut again = vec![9.0; 4];
        q.fill(&mut again, 2);
        assert!(again.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn underruns_grow_the_target_up_to_the_ceiling() {
        let q = SampleQueue::new(RATE);
        assert_eq!(q.target(), start());
        q.push(&vec![0.5; q.target()]);
        drain_past_empty(&q);
        assert_eq!(q.target(), start() + samples_for(TARGET_STEP, RATE));
        for _ in 0..20 {
            q.push(&vec![0.5; q.target()]);
            drain_past_empty(&q);
        }
        assert_eq!(q.target(), samples_for(MAX_TARGET, RATE));
    }

    #[test]
    fn a_calm_link_brings_the_target_back_down() {
        let q = SampleQueue::new(RATE);
        // One underrun raises the target...
        q.push(&vec![0.1; start()]);
        drain_past_empty(&q);
        let raised = q.target();
        assert!(raised > start());
        // ...then 15 s of steady playback lowers it a notch.
        let mut out = vec![0.0; 480];
        let mut played = 0;
        // (plus the time it takes to refill before playback resumes)
        while played < samples_for(CALM, RATE) + 2 * raised {
            q.push(&vec![0.1; 480]);
            q.fill(&mut out, 1);
            played += 480;
        }
        assert_eq!(q.target(), raised - samples_for(SHRINK_STEP, RATE));
    }

    #[test]
    fn latency_is_capped_and_trimmed_back_to_the_target() {
        let q = SampleQueue::new(RATE);
        q.push(&vec![0.1; start() + samples_for(HEADROOM, RATE) + 5_000]);
        assert_eq!(q.buffered(), start(), "trim keeps the cushion, not less");
        assert_eq!(q.trims(), 1);
    }

    #[test]
    fn clear_drops_audio_and_requires_rebuffering() {
        let q = SampleQueue::new(RATE);
        q.push(&vec![0.5; start()]);
        q.clear();
        assert_eq!(q.buffered(), 0);
        let mut out = vec![9.0; 4];
        q.fill(&mut out, 2);
        assert!(out.iter().all(|&s| s == 0.0));
    }
}
