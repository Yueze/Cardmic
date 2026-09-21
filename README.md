<p align="center">
  <img src="docs/images/icon.png" width="112" alt="Cardmic icon">
</p>

<h1 align="center">Cardmic</h1>

<p align="center">
  <b>Turn an M5Stack Cardputer ADV into a microphone.</b><br>
  Plug it in over USB and it just works. Unplug it and keep talking over Wi-Fi.
</p>

<p align="center">
  <a href="https://github.com/Yueze/cardmic/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/Yueze/cardmic?color=99ff00&labelColor=111"></a>
  <a href="https://github.com/Yueze/cardmic/actions/workflows/firmware.yml"><img alt="Firmware" src="https://img.shields.io/github/actions/workflow/status/Yueze/cardmic/firmware.yml?label=firmware&labelColor=111"></a>
  <a href="https://github.com/Yueze/cardmic/actions/workflows/client.yml"><img alt="Client" src="https://img.shields.io/github/actions/workflow/status/Yueze/cardmic/client.yml?label=client&labelColor=111"></a>
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-2f6bff?labelColor=111"></a>
</p>

<p align="center">
  <img src="docs/images/screen-main.png" width="720" alt="Cardmic main screen: live spectrogram and level meter">
</p>

---

Cardmic is an app for the Cardputer ADV's factory firmware. It keeps the stock
boot screen, launcher and apps, and adds a microphone that your computer can
use two ways:

- **USB.** A standard USB Audio Class microphone. No driver, no client, no
  setup. It shows up as **Cardmic Microphone**.
- **Wi-Fi.** A 16 kHz stream to a small open-source desktop client, which plays
  it into a virtual audio device. Every app on the computer can then pick it
  as a microphone.

## Features

|  |  |
|---|---|
| **Driverless USB mic** | USB Audio Class 2.0, 16 kHz mono. Tested with macOS; Windows 10+ and Linux ship the same class driver. |
| **Wireless mode** | UDP streaming on the local network, 20 ms packets, loss concealment, automatic discovery. |
| **Lives in the stock firmware** | Installed as one more app in M5Stack's launcher. Nothing else on the device changes. |
| **On-device setup** | Scan and join Wi-Fi with the built-in keyboard. No phone app, no config files. |
| **Live spectrogram** | Scrolling 100 Hz – 8 kHz heat map with a segmented level meter and peak hold. |
| **Clean signal path** | ES8311 codec at +30 dB PGA, DC removal, 100 Hz high-pass (4th order), three gain steps. |
| **Updates over Wi-Fi** | Checks GitHub Releases from the device and installs new versions, with automatic rollback. |
| **Open protocol** | A few pages of [spec](protocol/PROTOCOL.md) and reference code on both ends. |

## Screens

<table>
  <tr>
    <td><img src="docs/images/screen-settings.png" width="360" alt="Settings"></td>
    <td><img src="docs/images/screen-info.png" width="360" alt="Network info"></td>
    <td><img src="docs/images/screen-about.png" width="360" alt="About and updates"></td>
  </tr>
  <tr>
    <td align="center"><sub>Settings: Wi-Fi, info, mute, gain, about</sub></td>
    <td align="center"><sub>Info: signal, addresses, receiver</sub></td>
    <td align="center"><sub>About: version and over-the-air update</sub></td>
  </tr>
</table>

<sub>Pixel-exact captures from the device (`cardmic screenshot`), shown at 3x.
Network details in the captures are placeholders.</sub>

## Quick start

**1. Install the firmware (once).** Download `cardmic-0.5.0-full.bin` from the
[latest release](https://github.com/Yueze/cardmic/releases/latest) and flash it
over USB. Step-by-step instructions: [docs/flashing.md](docs/flashing.md).

**2. Use it over USB.** Open **Cardmic** in the launcher, plug the Cardputer
into your computer, and choose **Cardmic Microphone** as the input.

**3. Use it over Wi-Fi.** In Cardmic, press `S` > **Wi-Fi** and join your
network. On the computer, install a loopback audio device and run the client:

```bash
cardmic doctor   # finds a working loopback device
cardmic run      # receives the stream and plays it into that device
```

Then choose the loopback device (for example **BlackHole 2ch**) as the
microphone in your app. Details: [macOS](docs/macos.md) · [Windows](docs/windows.md).

## What your computer needs

| | macOS | Windows |
|---|---|---|
| **USB mode** | Nothing | Nothing |
| **Wi-Fi mode** | `cardmic` client + [BlackHole 2ch](https://existential.audio/blackhole/) | `cardmic` client + [VB-CABLE](https://vb-audio.com/Cable/) |

A virtual device you already have (from Zoom, Teams, Loopback and similar) may
work instead of BlackHole or VB-CABLE; `cardmic doctor` tests each one.

## Controls

| Key | Where | Action |
|---|---|---|
| `S` or `Enter` | Main | Open settings |
| `M` | Main | Mute / unmute (the stream keeps running, silent) |
| `;` `.` | Lists | Move up / down |
| `Enter` | Lists | Select |
| `Tab` | Password | Show / hide |
| `Esc` | Settings pages | Back |
| `G0` | Anywhere | Back; on the main screen, exit to the launcher |

## How it works

```
 Cardputer ADV (ESP32-S3)                              Computer
 ┌──────────────────────────────────┐
 │ ES8311 codec ─ I2S 16 kHz        │    USB Audio Class
 │   └─ DC block ─ 100 Hz HPF ─ gain├───────────────────────► any app
 │        │                         │
 │        └─ UDP packets (20 ms) ───┼───── Wi-Fi ─────► cardmic run
 │                                  │                     │ reorder, conceal,
 │ Stock launcher + apps (M5Stack)  │                     │ resample
 └──────────────────────────────────┘                     ▼
                                               loopback device ─► any app
```

The firmware is M5Stack's factory firmware
([M5Cardputer-UserDemo](https://github.com/m5stack/M5Cardputer-UserDemo), ESP-IDF
5.4) with Cardmic added as an app in
[`firmware/main/apps/app_cardmic`](firmware/main/apps/app_cardmic). The desktop
client is a single Rust binary built on [cpal](https://github.com/RustAudio/cpal),
in [`client/`](client).

## Build from source

**Firmware** (ESP-IDF v5.4.2):

```bash
cd firmware
python3 fetch_repos.py        # stock dependencies: M5GFX, M5Unified, mooncake...
idf.py build
./tools/package.sh            # dist/cardmic-ota.bin and dist/cardmic-<ver>-full.bin
```

**Client** (Rust 1.85+):

```bash
cd client
cargo test --workspace
cargo build --release         # target/release/cardmic
```

No hardware? `cardmic-synth` stands in for a device and streams a test tone,
so the client can be developed end to end:

```bash
cargo run -p cardmic-synth &                         # listens on UDP 41235
cargo run -p cardmic -- run --device 127.0.0.1:41235
```

## Status and roadmap

Cardmic is young. What works today, and what is next:

- [x] USB microphone, verified on macOS
- [x] Wireless streaming with the desktop client
- [x] On-device Wi-Fi setup, settings, over-the-air updates
- [ ] Pairing and encryption for wireless mode. Until then the stream is
      unencrypted: use it on networks you trust.
- [ ] Voice keyboard mode: hold a key to talk to your dictation app (USB HID +
      audio)
- [ ] A registered USB product ID (the firmware uses a development ID for now)
- [ ] Browser-based flashing and an M5Burner listing
- [ ] Signed and notarized desktop builds

Issues and pull requests are welcome.

## License

[MIT](LICENSE). The firmware is a fork of M5Stack's MIT-licensed factory
firmware; see [NOTICE.md](NOTICE.md) for third-party credits. Cardmic is an
independent project, not affiliated with M5Stack.
