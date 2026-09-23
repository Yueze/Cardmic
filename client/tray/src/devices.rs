//! Watches the machine's audio devices from a background thread: whether a
//! Cardputer is plugged in over USB, and which loopback devices exist.

use cardmic_audio::Candidate;
use cpal::traits::HostTrait;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Devices {
    /// Name of the Cardputer's USB microphone, if one is plugged in.
    pub usb_mic: Option<String>,
    /// Loopback candidates, in the order the system lists them.
    pub loopbacks: Vec<Candidate>,
}

pub fn scan() -> Devices {
    let host = cpal::default_host();
    let usb_mic = host
        .input_devices()
        .ok()
        .and_then(|it| it.map(|d| cardmic_audio::device_name(&d)).find(|n| n.contains("Cardmic")));
    Devices { usb_mic, loopbacks: cardmic_audio::loopback_candidates(&host) }
}

/// Calls `changed` with the current devices at once, then whenever they change.
pub fn watch(mut changed: impl FnMut(Devices) + Send + 'static) {
    std::thread::Builder::new()
        .name("cardmic-devices".into())
        .spawn(move || {
            let mut last: Option<Devices> = None;
            loop {
                let now = scan();
                if last.as_ref() != Some(&now) {
                    changed(now.clone());
                    last = Some(now);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("spawn device watcher");
}
