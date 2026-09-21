//! Audio device layer: find a loopback device, and play into it.
//!
//! A *loopback device* is a virtual audio device whose output side feeds its
//! own input side (BlackHole, VB-CABLE, and the devices Zoom, Teams, ToDesk
//! and others install). Cardmic plays the microphone stream into the output
//! side; every other app then sees it on the input side as an ordinary mic.
//!
//! Cardmic never changes the system's default input device. The user selects
//! the loopback device as the microphone in the app they are using. Changing
//! the default, and failing to restore it, is what made the original
//! prototype unpleasant to live with.

pub mod probe;
pub mod sink;

use cpal::traits::{DeviceTrait, HostTrait};

/// Human-readable name of a device, or an empty string if it has none.
pub fn device_name(device: &cpal::Device) -> String {
    device.description().map(|d| d.name().to_string()).unwrap_or_default()
}

/// Find an output device by exact name.
pub fn find_output(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.output_devices().ok()?.find(|d| device_name(d) == name)
}

/// Find an input device by exact name.
pub fn find_input(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.input_devices().ok()?.find(|d| device_name(d) == name)
}

/// Names of devices present on both the output and the input side.
///
/// This is only a *candidate* list. Appearing on both sides is necessary but
/// not sufficient for a loopback; use [`probe::loopback_rms`] to confirm.
pub fn loopback_candidates(host: &cpal::Host) -> Vec<String> {
    let outputs: Vec<String> = host
        .output_devices()
        .map(|it| it.map(|d| device_name(&d)).collect())
        .unwrap_or_default();
    let inputs: Vec<String> = host
        .input_devices()
        .map(|it| it.map(|d| device_name(&d)).collect())
        .unwrap_or_default();
    outputs.into_iter().filter(|n| !n.is_empty() && inputs.contains(n)).collect()
}
