# Windows setup

> USB mode uses the USB Audio Class driver built into Windows 10 and later.
> Wireless reception is tested on a Windows 11 PC with VB-CABLE, using the
> command-line client. The app is built and unit-tested for Windows in CI.

## USB mode

Nothing to install. Plug the Cardputer in, open Cardmic, and choose
**Cardmic Microphone** as the input in your app, or in
Settings > System > Sound > Input.

## Wireless mode

Wireless mode needs the Cardmic app, which receives the audio, and a
*loopback* audio device, which turns it into a microphone that any app can
select.

### 1. A loopback device

Install [VB-CABLE](https://vb-audio.com/Cable/) (donationware). It adds
**CABLE Input** (a speaker) and **CABLE Output** (a microphone). A virtual
device you already have may work too; the app tests them.

### 2. The app

Download `Cardmic-Windows-Setup.exe` from the
[latest release](https://github.com/Yueze/Cardmic/releases/latest) and run it.
It installs for your user only (no administrator rights) and starts Cardmic.

Windows may warn that the publisher is unknown (the installer is not signed
yet): choose **More info** > **Run anyway**.

Cardmic opens as a small window, and keeps an icon, three dots, in the
notification area next to the clock. Click the icon to show or hide the
window; right-click it for a short menu. If you cannot see it, click **^**,
and drag the icon onto the taskbar to keep it in view. Opening Cardmic again
from the Start menu brings the window back.

### 3. Use it

1. On the Cardputer, open Cardmic and connect to Wi-Fi (Settings > Wi-Fi). The
   PC must be on the same network. Allow Cardmic through Windows Defender
   Firewall on private networks when asked.
2. The window shows **LIVE**, the spectrogram moves, and **Link** reads
   **Wi-Fi**.
3. In your app, choose the microphone named under **Microphone**, usually
   **CABLE Output (VB-Audio Virtual Cable)**.

Cardmic starts with Windows; **Start with Windows** in the window turns that
off.

Cardmic keeps itself up to date: it checks for a new version now and then,
downloads it in the background and installs it when nothing is in use (or
when you click **Restart to update** at the bottom of its window).

### 4. Pair it (recommended)

1. On the Cardputer: Settings > **Pairing** > `Enter` to turn it on.
2. Plug the Cardputer into the PC once, with Cardmic open on both. The app
   reads the pairing code over USB and says **Paired over USB**. Unplug it;
   Wi-Fi now connects encrypted.

No cable at hand? Click **Enter code** in the window's Pairing row and type
the code from the Cardputer's Pairing page.

### Command line

The installer also puts the command-line client in
`%LOCALAPPDATA%\Programs\Cardmic\cli\cardmic.exe` (`run`, `pair`, `doctor`),
for scripts and troubleshooting. Quit the app first: only one of them can
receive at a time. Networks that block broadcast need the device's address:
`cardmic.exe run --device 192.168.1.42:41234`; the address is in
Cardmic > Settings > Info on the device.

## Hold to talk

Over USB the Cardputer can also hold a key for your dictation app while you
hold Space. See [talk-key.md](talk-key.md).

