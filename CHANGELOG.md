# Changelog

## Unreleased

- **Pairing is on out of the box.** A new Cardputer sends audio only to
  computers that know its code, encrypted: plug it in once and the app takes
  the code, or type it. A Cardputer that ran Cardmic 0.6 with pairing never
  turned on keeps it off, so an update does not cut off the computers it
  streams to.
- **Settings > Name.** Give the Cardputer a name (up to 16 characters). The
  Cardmic app shows it over Wi-Fi at once; over USB, computers list the
  microphone under it, and the app shows it, from the next time Cardmic
  opens. Two Cardputers are easy to tell apart. Unnamed, it stays Cardmic-XXXX in the app and
  Cardmic Microphone over USB.
- **App: a renamed Cardputer is still found** over USB, by the name it
  reports, on macOS and Windows.
- **Hold to talk guide:** a new talk key applies when Cardmic opens again,
  as the device says; the guide said at once.
- **Protocol:** the device tells its receiver its name, `CARDMIC_NAME <name>`
  (see PROTOCOL.md). Older apps ignore it.
- **A recorded discovery can no longer hold the stream.** With pairing on,
  anyone on the network could record a paired computer's discovery and send
  it again while that computer was away: the audio stayed encrypted, but the
  replay kept the paired computer from connecting. Now the device challenges
  each sender, and a paired app answers, bound to the device's address. A
  sender that has not answered (a recording, or an app from before 0.7.0)
  gets the stream only while no answering computer wants it, and is never
  told the device's name. Older apps and older firmware keep working.
- **Windows: a blocked microphone says so.** With a microphone privacy switch
  off (for the device, the user, or desktop apps), the app shows
  "Microphone access is off" and opens that Settings page, instead of
  reporting that no virtual device works.
- **App:** downloads of versions already installed are cleared at start;
  `Cardmic --diagnose` prints in the Windows console it was started from; the
  installer names its publisher The Cardmic Authors.
- **App: ready for a registered USB ID.** It also finds a Cardputer's identity
  under Espressif's vendor ID, so a firmware with a registered product ID
  will be recognised without an app update.
- **Builds:** macOS releases sign with a Developer ID and notarize once the
  signing secrets are set in CI; until then they are signed ad hoc, as before.

## 0.6.2 — 2026-09-24

App fixes; the firmware is unchanged apart from its version.

- **App: the version lives in the footer only**, always shown there, with
  where updates stand; the title bar is just the name.
- **App: the lock follows the link.** Turning pairing on while a computer is
  receiving encrypts the stream a moment later; the app now shows the lock
  then, instead of keeping the first, still-plain packet's "unencrypted"
  until it reconnects.

## 0.6.1 — 2026-09-23

- **Pairing page, reworded.** It names both ways to pair as the app does:
  plug in once, or type the code in Cardmic > Pairing > Enter code.
- **No clipped text on the device.** Every line of small text fits the
  screen; the pairing status and the update hint used to lose their last
  letters.
- **App: the Wi-Fi light is grey while it searches**, also when USB is what
  the window shows (it was amber, which reads as trouble).
- **App: the microphone prompt says what Cardmic listens to**, including the
  Cardputer over USB for the live view. The About box and Windows file
  details no longer show a license or a link.

## 0.6.0 — 2026-09-23

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
- **Original Cardputer (experimental).** The firmware now detects the model at
  boot. On the original Cardputer it scans the GPIO key matrix instead of the
  ADV's TCA8418 keyboard chip, and Cardmic records from the PDM microphone
  instead of the ES8311 codec; the IMU, LoRa and GPS apps are hidden. Before,
  the launcher came up but ignored every key, which looked like a freeze (#1).
  Confirmed on a Cardputer v1.1 by the reporter: keyboard and Cardmic work.
  The PDM microphone is clocked at 2.048 MHz, as in M5Unified, and
  Settings > Info shows the measured capture rate ("MIC").
- **"Keyboard not found" screen** for any other device, instead of a
  launcher that silently ignores keys.
- About shows the model (Cardputer ADV or Cardputer).
- **USB comes back after a restart.** Restarting while Cardmic is open (for
  example right after an over-the-air update) used to leave the Cardputer
  with no USB device until it was unplugged; the USB port is now handed back
  to the stock serial port on every restart.
- **Cardmic app for macOS and Windows.** A Dock/menu bar app (tray on
  Windows) with a small window in the device's style: its spectrogram and
  meter, live readouts, and Mac controls. `Cardmic-macOS.dmg`,
  `Cardmic-Windows-Setup.exe`. The `cardmic` command stays, built on the
  same engine.
- **The app updates itself.** It checks GitHub Releases like the device
  does, downloads a new version in the background (checksum, and code
  signature on macOS), and installs and restarts when nothing is in use, or
  when you click Restart to update.
- **Pair by plugging in.** Connect the Cardputer over USB once with pairing
  on, and the app takes the code itself; Wi-Fi then works, encrypted. A
  Cardputer with pairing on also tells a computer that lacks the code, so
  the app asks for it instead of searching forever.
- Client: the jitter buffer starts at 60 ms, grows after dropouts (up to
  400 ms on very busy networks) and comes back down after 15 calm seconds.
  `cardmic --version`.
- Update check: a pre-release (e.g. 0.6.0-beta.1) now ranks below the
  release of the same number, so beta testers still get the final version.

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
