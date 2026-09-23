//! The device's spectrogram, computed on the computer.
//!
//! A port of `spectrogram_column()` in the firmware (app_cardmic.cpp), so the
//! Cardmic window draws the same picture the Cardputer does: a 256-point FFT
//! every 10 ms of 16 kHz audio, 62 log-spaced bands from 100 Hz to 8 kHz, each
//! shown relative to its own noise floor, so steady background is black and
//! speech lights up.

use std::collections::VecDeque;
use std::f32::consts::PI;

/// Bands per column, bottom (100 Hz) first. Same as the device's `SPEC_H`.
pub const BANDS: usize = 62;
const FFT_N: usize = 256; // 16 ms at 16 kHz; 62.5 Hz per bin
const HOP: usize = 160; // one column per 10 ms
const SAMPLE_RATE: f32 = 16_000.0;
/// Columns kept for a reader that has not caught up (4 s).
const MAX_QUEUED: usize = 400;

/// One column (10 ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    /// Per band, 0 = at or below its noise floor, 255 = 45 dB above it.
    pub bands: [u8; BANDS],
    /// Peak level of these 10 ms: 0 = -60 dBFS or less, 255 = full scale.
    /// Drives the level meter in step with the picture.
    pub level: u8,
}

pub struct Spectrum {
    ring: VecDeque<f32>,
    since_column: usize,
    peak: f32,
    hann: [f32; FFT_N],
    lo: [usize; BANDS],
    hi: [usize; BANDS],
    floor: [f32; BANDS],
    floor_ready: bool,
    columns: VecDeque<Column>,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::new()
    }
}

impl Spectrum {
    pub fn new() -> Spectrum {
        let mut hann = [0.0; FFT_N];
        for (i, h) in hann.iter_mut().enumerate() {
            *h = 0.5 - 0.5 * (2.0 * PI * i as f32 / (FFT_N - 1) as f32).cos();
        }
        let bin_hz = SAMPLE_RATE / FFT_N as f32;
        let mut lo = [0; BANDS];
        let mut hi = [0; BANDS];
        for r in 0..BANDS {
            let f0 = 100.0 * 80f32.powf(r as f32 / BANDS as f32);
            let f1 = 100.0 * 80f32.powf((r + 1) as f32 / BANDS as f32);
            lo[r] = ((f0 / bin_hz) as usize).clamp(1, FFT_N / 2 - 1);
            hi[r] = ((f1 / bin_hz) as usize).clamp(lo[r], FFT_N / 2 - 1);
        }
        Spectrum {
            ring: VecDeque::with_capacity(FFT_N + 1),
            since_column: 0,
            peak: 0.0,
            hann,
            lo,
            hi,
            floor: [0.0; BANDS],
            floor_ready: false,
            columns: VecDeque::new(),
        }
    }

    /// Feed 16 kHz mono samples in -1..1.
    pub fn push(&mut self, samples: &[f32]) {
        for &s in samples {
            if self.ring.len() == FFT_N {
                self.ring.pop_front();
            }
            self.ring.push_back(s);
            self.peak = self.peak.max(s.abs());
            self.since_column += 1;
            if self.since_column >= HOP && self.ring.len() == FFT_N {
                self.since_column -= HOP;
                let mut col = self.column();
                let db = 20.0 * self.peak.max(1e-6).log10();
                col.level = (((db + 60.0) / 60.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                self.peak = 0.0;
                if self.columns.len() == MAX_QUEUED {
                    self.columns.pop_front();
                }
                self.columns.push_back(col);
            }
        }
    }

    /// Columns computed since the last call, oldest first.
    pub fn take(&mut self) -> Vec<Column> {
        self.columns.drain(..).collect()
    }

    fn column(&mut self) -> Column {
        // Digital silence (a lost packet filled in, or a muted device) says
        // nothing about the room: keep it black and leave the noise floor
        // alone, or everything after it looks far too loud until the floor
        // has crept back up. The device never sees this: its mic always hisses.
        if self.ring.iter().all(|s| s.abs() < 1e-5) {
            return Column { bands: [0u8; BANDS], level: 0 };
        }
        let mut re = [0.0f32; FFT_N];
        let mut im = [0.0f32; FFT_N];
        for (i, (r, s)) in re.iter_mut().zip(self.ring.iter()).enumerate() {
            *r = s * self.hann[i];
        }
        fft(&mut re, &mut im);

        // Full-scale sine through a Hann window; samples here are -1..1.
        let reference = FFT_N as f32 * 0.5 * 0.5;
        let mut out = Column { bands: [0u8; BANDS], level: 0 };
        for r in 0..BANDS {
            let mut m = 0.0f32;
            for k in self.lo[r]..=self.hi[r] {
                m = m.max(re[k] * re[k] + im[k] * im[k]);
            }
            let db = 10.0 * (m / (reference * reference) + 1e-12).log10();
            // The floor follows a quiet band down quickly and creeps up slowly
            // (1.5 dB/s), so sustained speech is not mistaken for noise.
            if !self.floor_ready {
                self.floor[r] = db;
            } else if db < self.floor[r] {
                self.floor[r] += (db - self.floor[r]) * 0.25;
            } else {
                self.floor[r] += 0.015;
            }
            let above = db - self.floor[r] - 7.0; // gate: noise flickers a few dB above its floor
            out.bands[r] = ((above / 38.0).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        self.floor_ready = true;
        out
    }
}

/// In-place iterative radix-2 FFT, as on the device.
fn fft(re: &mut [f32; FFT_N], im: &mut [f32; FFT_N]) {
    let mut j = 0;
    for i in 1..FFT_N {
        let mut bit = FFT_N >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= FFT_N {
        let ang = -2.0 * PI / len as f32;
        for i in (0..FFT_N).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f32).sin_cos();
                let (a, b) = (i + k, i + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, amp: f32, n: usize, start: usize) -> Vec<f32> {
        (start..start + n).map(|i| amp * (2.0 * PI * hz * i as f32 / SAMPLE_RATE).sin()).collect()
    }

    #[test]
    fn one_column_per_ten_milliseconds() {
        let mut s = Spectrum::new();
        s.push(&vec![0.0; 16_000]);
        let cols = s.take();
        // 100 per second, give or take the first window filling.
        assert!((cols.len() as i64 - 100).abs() <= 1, "{}", cols.len());
        assert!(s.take().is_empty());
    }

    #[test]
    fn steady_noise_is_black_and_a_new_tone_lights_its_band() {
        let mut s = Spectrum::new();
        // Quiet white noise first: it flickers faintly above its floor, as on
        // the device, but stays dark on average.
        let mut x: u32 = 12345;
        let noise: Vec<f32> = (0..32_000)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((x >> 8) as f32 / 16_777_216.0 - 0.5) * 0.002
            })
            .collect();
        s.push(&noise);
        let settled = s.take();
        let recent = &settled[settled.len() - 50..];
        let mean = recent.iter().flat_map(|c| c.bands.iter()).map(|&v| v as f32).sum::<f32>() / (50 * BANDS) as f32;
        assert!(mean < 30.0, "background mean {mean}");

        // Then a 1 kHz tone.
        s.push(&tone(1000.0, 0.3, 1600, 0));
        let lit = s.take().last().unwrap().bands;
        let band = (BANDS as f32 * (1000f32 / 100.0).ln() / 80f32.ln()) as usize;
        assert!(lit[band] > 200, "1 kHz band = {}", lit[band]);
        assert!(lit[2] < 60, "100 Hz band should stay dark: {}", lit[2]);
    }

    #[test]
    fn level_follows_the_peak() {
        let mut s = Spectrum::new();
        s.push(&tone(1000.0, 0.5, 3200, 0)); // -6 dBFS
        let level = s.take().last().unwrap().level as f32 / 255.0 * 60.0 - 60.0;
        assert!((level + 6.0).abs() < 1.0, "{level} dB");
        s.push(&vec![0.0; 1600]);
        assert_eq!(s.take().last().unwrap().level, 0);
    }

    #[test]
    fn digital_silence_does_not_drag_the_floor_down() {
        let mut s = Spectrum::new();
        let mut x: u32 = 1;
        let mut noise = |n: usize| -> Vec<f32> {
            (0..n)
                .map(|_| {
                    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    ((x >> 8) as f32 / 16_777_216.0 - 0.5) * 0.002
                })
                .collect()
        };
        s.push(&noise(16_000));
        s.push(&vec![0.0; 8_000]); // half a second of lost packets
        s.push(&noise(3_200));
        let after = s.take();
        let last = after.last().unwrap();
        let mean = last.bands.iter().map(|&v| v as f32).sum::<f32>() / BANDS as f32;
        assert!(mean < 40.0, "room noise after a gap should stay dark, mean {mean}");
    }

    #[test]
    fn a_reader_that_never_reads_does_not_grow_memory() {
        let mut s = Spectrum::new();
        s.push(&vec![0.0; 16_000 * 10]);
        assert_eq!(s.take().len(), MAX_QUEUED);
    }
}
