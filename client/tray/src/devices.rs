//! Watches the machine's audio devices from a background thread: whether a
//! Cardputer is plugged in over USB (and what it says about itself), and
//! which loopback devices exist.

use cardmic_audio::Candidate;
use cpal::traits::HostTrait;
use hidapi::HidApi;
use std::time::Duration;

/// What a plugged-in Cardputer reports over USB (its identity HID report).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UsbIdentity {
    /// The pairing code, present while pairing is on.
    pub code: Option<String>,
    pub name: String,
    pub firmware: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Devices {
    /// Name of the Cardputer's USB microphone, if one is plugged in.
    pub usb_mic: Option<String>,
    pub usb_identity: Option<UsbIdentity>,
    /// Loopback candidates, in the order the system lists them.
    pub loopbacks: Vec<Candidate>,
}

/// USB vendor IDs a Cardputer running Cardmic can have: TinyUSB's placeholder,
/// which the firmware uses today, and Espressif's, for product IDs allocated
/// from its list. The identity report itself ("CM1;...") tells a Cardmic from
/// any other Espressif device.
const CARDMIC_VIDS: [u16; 2] = [0xCAFE, 0x303A];
const ID_USAGE_PAGE: u16 = 0xFF00;
const ID_REPORT: u8 = 3;

pub fn scan(hid: Option<&mut HidApi>) -> Devices {
    let host = cpal::default_host();
    let usb_identity = hid.and_then(read_identity);
    let usb_mic = host.input_devices().ok().and_then(|it| {
        it.map(|d| cardmic_audio::device_name(&d))
            .find(|n| is_cardmic_mic(n, usb_identity.as_ref().map(|id| id.name.as_str())))
    });
    Devices { usb_mic, usb_identity, loopbacks: cardmic_audio::loopback_candidates(&host) }
}

/// Whether an input device is the Cardputer's microphone. It is listed as
/// "Cardmic Microphone" unless the Cardputer was given a name (Settings >
/// Name), which it then also goes by over USB: as is on macOS, as
/// "Microphone (name)" on Windows.
fn is_cardmic_mic(device: &str, name: Option<&str>) -> bool {
    device.contains("Cardmic")
        || name.is_some_and(|name| {
            !name.is_empty() && (device == name || device.ends_with(&format!(" {name})")) || device.ends_with(&format!("({name})")))
        })
}

fn read_identity(api: &mut HidApi) -> Option<UsbIdentity> {
    api.refresh_devices().ok()?;
    let candidates: Vec<_> = api
        .device_list()
        .filter(|d| CARDMIC_VIDS.contains(&d.vendor_id()) && d.usage_page() == ID_USAGE_PAGE)
        .cloned()
        .collect();
    candidates.iter().find_map(|info| {
        let device = info.open_device(api).ok()?;
        let mut buf = [0u8; 64];
        buf[0] = ID_REPORT;
        let n = device.get_feature_report(&mut buf).ok()?;
        parse_identity(&String::from_utf8_lossy(&buf[1..n.max(1)]))
    })
}

/// `CM1;pair=1;code=7K2M9QXB4TPA;name=Cardmic-05AC;fw=0.6.0`
pub fn parse_identity(text: &str) -> Option<UsbIdentity> {
    let text = text.trim_end_matches('\0');
    let mut parts = text.split(';');
    if parts.next()? != "CM1" {
        return None;
    }
    let mut id = UsbIdentity::default();
    let mut paired = false;
    for kv in parts {
        match kv.split_once('=') {
            Some(("pair", v)) => paired = v == "1",
            Some(("code", v)) => id.code = Some(v.to_string()),
            Some(("name", v)) => id.name = v.to_string(),
            Some(("fw", v)) => id.firmware = v.to_string(),
            _ => {}
        }
    }
    if !paired {
        id.code = None;
    }
    Some(id)
}

/// Calls `changed` with the current devices at once, then whenever they change.
pub fn watch(mut changed: impl FnMut(Devices) + Send + 'static) {
    std::thread::Builder::new()
        .name("cardmic-devices".into())
        .spawn(move || {
            let mut hid = HidApi::new().ok();
            let mut last: Option<Devices> = None;
            loop {
                let now = scan(hid.as_mut());
                if last.as_ref() != Some(&now) {
                    changed(now.clone());
                    last = Some(now);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("spawn device watcher");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_with_pairing_on() {
        let id = parse_identity("CM1;pair=1;code=7K2M9QXB4TPA;name=Cardmic-05AC;fw=0.6.0\0\0").unwrap();
        assert_eq!(id.code.as_deref(), Some("7K2M9QXB4TPA"));
        assert_eq!(id.name, "Cardmic-05AC");
        assert_eq!(id.firmware, "0.6.0");
    }

    #[test]
    fn a_renamed_cardputer_is_still_found() {
        assert!(is_cardmic_mic("Cardmic Microphone", None));
        assert!(is_cardmic_mic("Microphone (Cardmic Microphone)", None));
        assert!(is_cardmic_mic("Kellan's mic", Some("Kellan's mic")), "macOS lists it by its name");
        assert!(is_cardmic_mic("Microphone (Kellan's mic)", Some("Kellan's mic")), "Windows");
        assert!(is_cardmic_mic("Microphone (2- Kellan's mic)", Some("Kellan's mic")), "Windows, a second one");
        assert!(!is_cardmic_mic("MacBook Pro Microphone", Some("Mic")), "a short name is not a substring match");
        assert!(!is_cardmic_mic("MacBook Pro Microphone", None));
        assert!(!is_cardmic_mic("Studio Display Microphone", Some("")));
    }

    #[test]
    fn identity_with_pairing_off_has_no_code() {
        let id = parse_identity("CM1;pair=0;name=Cardmic-05AC;fw=0.6.0").unwrap();
        assert_eq!(id.code, None);
        assert!(parse_identity("something else").is_none());
    }
}
