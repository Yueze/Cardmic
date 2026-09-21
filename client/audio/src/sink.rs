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

/// Audio buffered before playback starts, so a little network jitter does not
/// cause an immediate underrun.
const PREBUFFER: Duration = Duration::from_millis(60);

/// Upper bound on buffered audio. The device and host clocks drift, and
/// without a cap latency creeps upward for as long as the session runs.
const MAX_LATENCY: Duration = Duration::from_millis(200);

fn samples_for(d: Duration, rate: u32) -> usize {
    (d.as_secs_f64() * rate as f64) as usize
}

struct QueueState {
    buf: VecDeque<f32>,
    /// Playback has buffered enough to start. Cleared on underrun, so the
    /// queue rebuilds its cushion instead of stuttering sample by sample.
    playing: bool,
    prebuffer: usize,
    max: usize,
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
                buf: VecDeque::with_capacity(samples_for(MAX_LATENCY, rate) * 2),
                playing: false,
                prebuffer: samples_for(PREBUFFER, rate),
                max: samples_for(MAX_LATENCY, rate),
            }),
            underruns: AtomicU64::new(0),
            trimmed: AtomicU64::new(0),
        }
    }

    /// Append audio. If the queue would exceed the latency cap, the oldest
    /// audio is dropped back down to the prebuffer level.
    pub fn push(&self, samples: &[f32]) {
        let mut s = self.state.lock().unwrap();
        s.buf.extend(samples.iter().copied());
        if s.buf.len() > s.max {
            let excess = s.buf.len() - s.prebuffer;
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

    /// Fill `out` (interleaved, `channels` wide) from the queue. Called from
    /// the audio callback; never blocks.
    fn fill(&self, out: &mut [f32], channels: usize) {
        let Ok(mut s) = self.state.try_lock() else {
            out.fill(0.0);
            return;
        };
        if !s.playing && s.buf.len() >= s.prebuffer {
            s.playing = true;
        }
        if !s.playing {
            out.fill(0.0);
            return;
        }
        for frame in out.chunks_mut(channels) {
            match s.buf.pop_front() {
                Some(v) => frame.fill(v),
                None => {
                    frame.fill(0.0);
                    if s.playing {
                        s.playing = false;
                        self.underruns.fetch_add(1, Ordering::Relaxed);
                    }
                }
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

    #[test]
    fn stays_silent_until_prebuffered() {
        let q = SampleQueue::new(RATE);
        q.push(&vec![0.5; samples_for(PREBUFFER, RATE) - 1]);
        let mut out = vec![9.0; 8];
        q.fill(&mut out, 2);
        assert!(out.iter().all(|&s| s == 0.0), "must not play before the prebuffer fills");
    }

    #[test]
    fn plays_once_prebuffered_and_replicates_channels() {
        let q = SampleQueue::new(RATE);
        q.push(&vec![0.25; samples_for(PREBUFFER, RATE)]);
        let mut out = vec![0.0; 6];
        q.fill(&mut out, 3);
        assert_eq!(out, vec![0.25; 6]);
    }

    #[test]
    fn underrun_rebuffers_instead_of_stuttering() {
        let q = SampleQueue::new(RATE);
        let pre = samples_for(PREBUFFER, RATE);
        q.push(&vec![0.5; pre]);
        let mut out = vec![0.0; (pre + 10) * 2];
        q.fill(&mut out, 2);
        assert_eq!(q.underruns(), 1);
        // A trickle below the prebuffer level must not restart playback.
        q.push(&[0.5; 4]);
        let mut again = vec![9.0; 4];
        q.fill(&mut again, 2);
        assert!(again.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn latency_is_capped() {
        let q = SampleQueue::new(RATE);
        q.push(&vec![0.1; samples_for(MAX_LATENCY, RATE) + 5_000]);
        assert!(q.buffered() <= samples_for(MAX_LATENCY, RATE));
        assert_eq!(q.trims(), 1);
    }

    #[test]
    fn clear_drops_audio_and_requires_rebuffering() {
        let q = SampleQueue::new(RATE);
        q.push(&vec![0.5; samples_for(PREBUFFER, RATE)]);
        q.clear();
        assert_eq!(q.buffered(), 0);
        let mut out = vec![9.0; 4];
        q.fill(&mut out, 2);
        assert!(out.iter().all(|&s| s == 0.0));
    }
}
