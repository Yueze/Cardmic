//! Listening to the Cardputer over USB, for the window.
//!
//! Plugged in, the Cardputer is an ordinary USB microphone: apps use it
//! directly and Cardmic has nothing to receive. The window still shows it
//! live by recording from it and running the same analysis as for Wi-Fi.
//! Only while the window is open, so the system's microphone indicator is
//! not on the rest of the time.

use cardmic_audio::find_input;
use cardmic_core::dsp::Resampler;
use cardmic_engine::spectrum::{Column, Spectrum};
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub struct UsbMonitor {
    _stream: cpal::Stream,
    spectrum: Arc<Mutex<Spectrum>>,
    pub name: String,
}

impl UsbMonitor {
    pub fn open(name: &str) -> Result<UsbMonitor, String> {
        let host = cpal::default_host();
        let device = find_input(&host, name).ok_or_else(|| format!("no input device named {name:?}"))?;
        let config = device.default_input_config().map_err(|e| format!("input config: {e}"))?;
        let rate = config.sample_rate();
        let channels = config.channels().max(1) as usize;

        let spectrum = Arc::new(Mutex::new(Spectrum::new()));
        let sink = spectrum.clone();
        let mut resampler = Resampler::new(rate, 16_000);
        let (mut mono, mut out) = (Vec::with_capacity(4096), Vec::with_capacity(4096));
        let stream = device
            .build_input_stream(
                config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // The device is mono; the system may present it as more.
                    mono.clear();
                    mono.extend(data.chunks(channels).map(|f| f[0]));
                    out.clear();
                    resampler.process(&mono, &mut out);
                    if let Ok(mut s) = sink.lock() {
                        s.push(&out);
                    }
                },
                |_err: cpal::Error| {},
                None,
            )
            .map_err(|e| format!("open {name:?}: {e}"))?;
        stream.play().map_err(|e| format!("start {name:?}: {e}"))?;
        Ok(UsbMonitor { _stream: stream, spectrum, name: name.to_string() })
    }

    pub fn take_spectrum(&self) -> Vec<Column> {
        self.spectrum.lock().map(|mut s| s.take()).unwrap_or_default()
    }
}
