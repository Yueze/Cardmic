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

/// A possible loopback: audio played into `output` may come back on `input`.
///
/// Usually both sides share a name (BlackHole, Loopback). VB-CABLE does not:
/// you play into "CABLE Input" and apps record from "CABLE Output".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Device Cardmic plays into.
    pub output: String,
    /// Device the user picks as the microphone in their app.
    pub input: String,
}

impl Candidate {
    /// One name if both sides share it, else "output -> input".
    pub fn label(&self) -> String {
        if self.output == self.input {
            self.output.clone()
        } else {
            format!("{} -> {}", self.output, self.input)
        }
    }
}

/// Trailing "(...)" of a device name, e.g. the driver in
/// "CABLE Input (VB-Audio Virtual Cable)".
fn driver_suffix(name: &str) -> Option<&str> {
    let name = name.trim_end();
    if !name.ends_with(')') {
        return None;
    }
    name.rfind('(').map(|i| &name[i..])
}

/// Pair output and input device names that could form a loopback.
///
/// Only a candidate list: [`probe::loopback_rms`] confirms. The probe plays a
/// tone into the output side, so real hardware (speakers) must never be
/// proposed here -- pairing by driver name is limited to virtual devices.
pub fn pair_candidates(outputs: &[String], inputs: &[String]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut push = |o: &str, i: &str| {
        let c = Candidate { output: o.to_string(), input: i.to_string() };
        if !out.contains(&c) {
            out.push(c);
        }
    };
    for o in outputs.iter().filter(|o| !o.is_empty()) {
        // 1. Same name on both sides.
        if inputs.contains(o) {
            push(o, o);
            continue;
        }
        // 2. VB-Audio naming: play into "X Input", record from "X Output".
        if o.contains("Input") {
            let swapped = o.replacen("Input", "Output", 1);
            if inputs.contains(&swapped) {
                push(o, &swapped);
                continue;
            }
        }
        // 3. Same virtual driver, differently named endpoints (e.g. ToDesk's
        //    speaker and microphone). Never for physical hardware.
        if let Some(suffix) = driver_suffix(o) {
            let lower = suffix.to_lowercase();
            if ["virtual", "cable", "loopback"].iter().any(|w| lower.contains(w)) {
                for i in inputs.iter().filter(|i| driver_suffix(i) == Some(suffix)) {
                    push(o, i);
                }
            }
        }
    }
    out
}

/// Loopback candidates among this machine's audio devices.
pub fn loopback_candidates(host: &cpal::Host) -> Vec<Candidate> {
    let outputs: Vec<String> = host
        .output_devices()
        .map(|it| it.map(|d| device_name(&d)).collect())
        .unwrap_or_default();
    let inputs: Vec<String> = host
        .input_devices()
        .map(|it| it.map(|d| device_name(&d)).collect())
        .unwrap_or_default();
    pair_candidates(&outputs, &inputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn pairs(outputs: &[&str], inputs: &[&str]) -> Vec<(String, String)> {
        pair_candidates(&names(outputs), &names(inputs)).into_iter().map(|c| (c.output, c.input)).collect()
    }

    #[test]
    fn same_name_devices_pair_with_themselves() {
        assert_eq!(
            pairs(&["BlackHole 2ch", "MacBook Pro Speakers"], &["BlackHole 2ch", "MacBook Pro Microphone"]),
            vec![("BlackHole 2ch".into(), "BlackHole 2ch".into())]
        );
    }

    #[test]
    fn vb_cable_pairs_input_with_output() {
        // As Windows names them: you play into CABLE Input, record from CABLE Output.
        assert_eq!(
            pairs(
                &["CABLE Input (VB-Audio Virtual Cable)", "Realtek Digital Output (Realtek High Definition Audio)"],
                &["CABLE Output (VB-Audio Virtual Cable)"]
            ),
            vec![("CABLE Input (VB-Audio Virtual Cable)".into(), "CABLE Output (VB-Audio Virtual Cable)".into())]
        );
        assert_eq!(
            pairs(&["VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)"], &["VoiceMeeter Output (VB-Audio VoiceMeeter VAIO)"]).len(),
            1
        );
    }

    #[test]
    fn virtual_drivers_pair_by_driver_name() {
        assert_eq!(
            pairs(&["扬声器 (ToDesk Virtual Audio)"], &["麦克风 (ToDesk Virtual Audio)"]),
            vec![("扬声器 (ToDesk Virtual Audio)".into(), "麦克风 (ToDesk Virtual Audio)".into())]
        );
    }

    #[test]
    fn physical_hardware_is_never_proposed() {
        // The probe would play a tone out of these speakers.
        assert!(pairs(
            &["Speakers (Realtek High Definition Audio)"],
            &["Microphone (Realtek High Definition Audio)"]
        )
        .is_empty());
    }
}
