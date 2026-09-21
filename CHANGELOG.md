# Changelog

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
