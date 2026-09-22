# Changelog

## 0.6.0 — 2026-09-22

- **Pairing and encryption.** Settings > Pairing shows a 12-character code;
  `cardmic pair CODE` stores it on the computer. With pairing on, the device
  only streams to computers that know the code, and audio is encrypted with
  AES-128-GCM (hardware-accelerated on the ESP32-S3). Off by default, so
  existing setups keep working after the update.
- **Hold to talk.** Settings > Talk key turns USB into microphone + keyboard;
  holding Space holds Ctrl+Option, Ctrl+Win or F13 for dictation apps.
- **Vivid spectrogram.** Each frequency band is drawn relative to its own
  noise floor, so steady background noise is black and speech lights up at
  full saturation. It no longer dims while no computer is listening.
- **Clear link lights.** USB and WIFI turn lime when linked and breathe while
  audio is flowing to that computer; amber means connecting or trouble.
- **Wi-Fi recovers by itself.** A dropped network is noticed and rejoined;
  a failed join is retried every 30 s.
- **Windows: VB-CABLE is detected.** The client used to look only for devices
  with the same name on both sides, which VB-CABLE ("CABLE Input" /
  "CABLE Output") is not. It now pairs such devices and tells you which one
  to pick as the microphone.
- **Browser installer** at https://yueze.github.io/Cardmic/.
- **"Keyboard not found" screen.** On a device without the ADV's keyboard
  controller (such as the original Cardputer) the firmware used to boot to a
  launcher that ignored every key, which looked like a freeze (#1). It now
  says so, retries every second, and G0 continues anyway.

## 0.5.2 — 2026-09-22

- Client: adaptive jitter buffer. Playback starts at 80 ms of buffer and
  grows 40 ms after each underrun, up to 240 ms, so a Wi-Fi link that pauses
  periodically (common on Macs) stops stuttering after a few seconds instead
  of every second. Over-long buffers are trimmed back to the target rather
  than to near-empty. The status line shows `buffer current/target`.
- Firmware: version bump only.

## 0.5.1 — 2026-09-22

- Check update now says "Up to date" when there is nothing newer to install,
  instead of reporting an error.
- A newer version is offered with a confirmation step: `Enter` installs,
  `G0` dismisses it for later.

## 0.5.0 — 2026-09-22

First public release.

- Cardmic runs as an app inside M5Stack's factory firmware for the Cardputer
  ADV; the stock launcher and apps are kept.
- USB Audio Class 2.0 microphone, 16 kHz mono, no driver needed.
- Wi-Fi streaming to the `cardmic` desktop client (macOS, Windows).
- On-device Wi-Fi setup with the keyboard, including hidden networks.
- Settings: Wi-Fi, network info, mute, mic gain (low / mid / high), about.
- Live spectrogram (100 Hz – 8 kHz) and segmented level meter.
- Signal path: +30 dB codec PGA, DC removal, 4th-order 100 Hz high-pass.
- Over-the-air updates from GitHub Releases with automatic rollback.
- `cardmic screenshot` captures the device screen over Wi-Fi.
