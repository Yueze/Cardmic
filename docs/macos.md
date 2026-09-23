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

Cardmic opens as a small window, and keeps an icon, three dots pointing
right, in the Dock and in the menu bar. Click the Dock icon to bring the
window back; click the menu bar icon for a short menu. A full menu bar or the
camera notch can hide the menu bar icon; the Dock icon always works.

The first time, macOS asks to let Cardmic use the microphone. Allow it:
Cardmic listens to your virtual audio devices for a moment to find one that
works, and to the Cardputer over USB while its window is open, to show it
live. It never records your other microphones.

### 3. Use it

1. On the Cardputer, open Cardmic and connect to Wi-Fi (Settings > Wi-Fi). The
   Mac must be on the same network. If macOS asks to let Cardmic find devices
   on your local network, allow it.
2. The window shows **LIVE**, the spectrogram moves, and **Link** reads
   **Wi-Fi**.
3. In your app, choose the microphone named under **Microphone**, for example
   **BlackHole 2ch**. **Copy** puts the name on the clipboard.

Cardmic opens at login; **Open at login** in the window turns that off.

Cardmic keeps itself up to date: it checks for a new version now and then,
downloads it in the background and installs it when nothing is in use (or
when you click **Restart to update** at the bottom of its window).

### 4. Pair it (recommended)

Out of the box, anyone on the same Wi-Fi who runs Cardmic first could receive
the audio. Pairing fixes that:

1. On the Cardputer: Settings > **Pairing** > `Enter` to turn it on.
2. Plug the Cardputer into the Mac once, with Cardmic open on both. The app
   reads the pairing code over USB and says **Paired over USB**. Unplug it;
   Wi-Fi now connects encrypted.

No cable at hand? Click **Enter code** in the window's Pairing row and type
the code from the Cardputer's Pairing page, e.g. `7K2M-9QXB-4TPA`.

Once paired, **Link** shows a lock, and only computers that know the code
receive audio. `N` on the Pairing page makes a new code, which unpairs every
computer; the next time, the window asks for it.

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
