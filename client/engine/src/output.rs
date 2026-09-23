//! Choosing the loopback device Cardmic plays into.

use cardmic_audio::probe;
use cardmic_audio::Candidate;

/// Pick a device among those that verifiably loop back. Purpose-built
/// loopback drivers are preferred over ones bundled with conferencing apps,
/// which can disappear when that app is uninstalled or updated.
pub fn preferred(working: &[Candidate]) -> Option<&Candidate> {
    working
        .iter()
        .find(|c| c.output.contains("BlackHole"))
        .or_else(|| working.iter().find(|c| c.output.starts_with("CABLE Input")))
        .or_else(|| working.iter().find(|c| c.output.contains("CABLE")))
        .or_else(|| working.first())
}

/// Candidates that pass the loopback probe (a short tone played into the
/// output side must come back on the input side).
pub fn working(host: &cpal::Host) -> Vec<Candidate> {
    cardmic_audio::loopback_candidates(host)
        .into_iter()
        .filter(|c| probe::loopback_rms(host, c).is_loopback())
        .collect()
}

/// Names of drivers made to be loopbacks. These are used without the probe.
const PURPOSE_BUILT: &[&str] = &["BlackHole", "CABLE Input", "Loopback Audio", "Soundflower"];

/// A device that can be chosen without the probe: the one a previous run
/// settled on, if it still exists, or a purpose-built loopback driver.
///
/// The probe records from the loopback's input side, which macOS counts as
/// microphone access; this path needs no permission and plays no test tone.
pub fn known(host: &cpal::Host, remembered: Option<&str>) -> Option<Candidate> {
    let candidates = cardmic_audio::loopback_candidates(host);
    if let Some(name) = remembered {
        if let Some(c) = candidates.iter().find(|c| c.output == name) {
            return Some(c.clone());
        }
    }
    let built: Vec<Candidate> =
        candidates.into_iter().filter(|c| PURPOSE_BUILT.iter().any(|p| c.output.contains(p))).collect();
    preferred(&built).cloned()
}

/// The device to play into: [`known`] if possible, else the best candidate
/// that passes the probe.
pub fn choose(host: &cpal::Host, remembered: Option<&str>) -> Option<Candidate> {
    known(host, remembered).or_else(|| preferred(&working(host)).cloned())
}

/// The candidate whose output side is `name`, or a same-named pair if the
/// device is not a recognised loopback (the user knows best).
pub fn named(host: &cpal::Host, name: &str) -> Candidate {
    cardmic_audio::loopback_candidates(host)
        .into_iter()
        .find(|c| c.output == name)
        .unwrap_or_else(|| Candidate { output: name.to_string(), input: name.to_string() })
}

/// What to install when this machine has no loopback device.
pub fn install_hint() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("VB-CABLE", "https://vb-audio.com/Cable/")
    } else {
        ("BlackHole 2ch", "https://existential.audio/blackhole/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(o: &str, i: &str) -> Candidate {
        Candidate { output: o.into(), input: i.into() }
    }

    #[test]
    fn purpose_built_loopbacks_win() {
        let list = [c("WeMeet Audio Device", "WeMeet Audio Device"), c("BlackHole 2ch", "BlackHole 2ch")];
        assert_eq!(preferred(&list).unwrap().output, "BlackHole 2ch");
        let win = [
            c("扬声器 (ToDesk Virtual Audio)", "麦克风 (ToDesk Virtual Audio)"),
            c("CABLE In 16ch (VB-Audio Virtual Cable)", "CABLE Output (VB-Audio Virtual Cable)"),
            c("CABLE Input (VB-Audio Virtual Cable)", "CABLE Output (VB-Audio Virtual Cable)"),
        ];
        assert_eq!(preferred(&win).unwrap().output, "CABLE Input (VB-Audio Virtual Cable)");
    }

    #[test]
    fn anything_that_works_beats_nothing() {
        let list = [c("BYOM-Audio", "BYOM-Audio")];
        assert_eq!(preferred(&list).unwrap().output, "BYOM-Audio");
        assert!(preferred(&[]).is_none());
    }
}
