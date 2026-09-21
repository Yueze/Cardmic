# macOS setup

## USB mode

Nothing to install. Plug the Cardputer in, open Cardmic, and choose
**Cardmic Microphone** as the input in your app, or in
System Settings > Sound > Input.

## Wireless mode

Wireless mode needs two things on the Mac: the Cardmic client, which receives
the audio, and a *loopback* audio device, which turns it into a microphone that
any app can select.

### 1. A loopback device

Install [BlackHole 2ch](https://existential.audio/blackhole/) (free, open
source):

```bash
brew install blackhole-2ch
```

Already have a virtual device from Zoom, Teams, Loopback or similar? It may
work as is. `cardmic doctor` tests every device and tells you which ones
genuinely loop audio back.

### 2. The client

Download `cardmic-macos-universal.tar.gz` from the
[latest release](https://github.com/Yueze/Cardmic/releases/latest), then:

```bash
tar xzf cardmic-macos-universal.tar.gz
xattr -d com.apple.quarantine cardmic   # the binary is not notarized yet
./cardmic doctor
```

### 3. Use it

1. On the Cardputer, open Cardmic and connect to Wi-Fi (Settings > Wi-Fi). The
   Mac must be on the same network.
2. On the Mac, run `./cardmic run`. The WIFI dot on the device turns green when
   the stream is live.
3. In your app, choose **BlackHole 2ch** (or the device `cardmic run` names)
   as the microphone.

Leave `cardmic run` open while you talk. Press Ctrl-C to stop.

## Notes

- The first time `cardmic run` starts, macOS may ask to allow incoming network
  connections. Allow it; the audio arrives over UDP.
- Networks that block broadcast (some office and hotel Wi-Fi) need the
  device's address: `./cardmic run --device 192.168.1.42:41234`. The address
  is shown in Cardmic > Settings > Info.
