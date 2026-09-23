# macOS setup

## USB mode

Nothing to install. Plug the Cardputer in, open Cardmic, and choose
**Cardmic Microphone** as the input in your app, or in
System Settings > Sound > Input.

## Wireless mode

Wireless mode needs the Cardmic app, which receives the audio, and a
*loopback* audio device, which turns it into a microphone that any app can
select.

### 1. A loopback device

Install [BlackHole 2ch](https://existential.audio/blackhole/) (free, open
source), or with Homebrew: `brew install blackhole-2ch`.

Already have a virtual device from Zoom, Teams, Loopback or similar? It may
work as is: the app tests your virtual devices and uses one that loops sound
back.

### 2. The app

Download `Cardmic-macOS.dmg` from the
[latest release](https://github.com/Yueze/Cardmic/releases/latest), open it and
drag **Cardmic** to **Applications**. Then open Cardmic from Applications.

The app is not notarized by Apple yet, so the first time macOS says it cannot
check it. Open **System Settings > Privacy & Security**, scroll down and click
**Open Anyway** next to Cardmic. After that it opens normally.

Cardmic has no window. Its icon, three dots pointing right, sits in the Dock
and in the menu bar: filled while audio is flowing, rings while it looks for
the Cardputer. Click either one for its menu. A full menu bar or the camera
notch can hide the menu bar icon; the Dock icon always works.

The first time, macOS asks to let Cardmic use the microphone. Allow it:
Cardmic records from your virtual audio devices for a moment, once, to find
one that works. It never records your real microphones.

### 3. Use it

1. On the Cardputer, open Cardmic and connect to Wi-Fi (Settings > Wi-Fi). The
   Mac must be on the same network. If macOS asks to let Cardmic find devices
   on your local network, allow it.
2. The icon fills in and the menu says **Connected over Wi-Fi**.
3. In your app, choose the microphone the menu names, for example
   **BlackHole 2ch**.

Cardmic opens at login (System Settings > General > Login Items); the menu
has a switch to turn that off.

### 4. Pair it (recommended)

Out of the box, anyone on the same Wi-Fi who runs Cardmic first could receive
the audio. Pairing fixes that:

1. On the Cardputer: Settings > **Pairing** > `Enter` to turn it on. Note the
   code, e.g. `7K2M-9QXB-4TPA`.
2. In the Cardmic menu: **Pair with Cardputer…**, and type the code.

The menu then says **Connected over Wi-Fi · encrypted**. Only computers that
know the code receive audio. `N` on the Pairing page makes a new code, which
unpairs every computer.

### Command line

The app bundle also contains the command-line client:
`/Applications/Cardmic.app/Contents/Resources/cardmic` (`run`, `pair`,
`doctor`, `screenshot`). Quit the app first: only one of them can receive at
a time.

## Hold to talk

Over USB the Cardputer can also hold a key for your dictation app while you
hold Space. See [talk-key.md](talk-key.md).

## Notes

- **Brief stutter in the first seconds.** Wi-Fi on a Mac can pause for about
  100 ms roughly once a second, often because of AWDL, the link behind
  AirDrop and Continuity. Cardmic notices and grows its buffer; after that,
  audio is continuous.
  For the lowest latency you can turn AWDL off until the next restart with
  `sudo ifconfig awdl0 down` (AirDrop stops working until it is back on), or
  put the Mac on Ethernet.

- macOS may ask to allow incoming network connections. Allow it; the audio
  arrives over UDP.
- Networks that block broadcast (some office and hotel Wi-Fi) need the
  device's address, which only the command line takes for now:
  `cardmic run --device 192.168.1.42:41234`. The address is shown in
  Cardmic > Settings > Info.
- Something not working? The app keeps a short log in
  `~/Library/Logs/Cardmic.log`; attach it to an issue.
