# Windows setup

> Windows support is new. USB mode uses the USB Audio Class driver built into
> Windows 10 and later. The wireless client builds and passes its tests on
> Windows in CI but has had little real-world use yet. Reports are welcome.

## USB mode

Nothing to install. Plug the Cardputer in, open Cardmic, and choose
**Cardmic Microphone** as the input in your app, or in
Settings > System > Sound > Input.

## Wireless mode

Wireless mode needs the Cardmic client, which receives the audio, and a
*loopback* audio device, which turns it into a microphone that any app can
select.

### 1. A loopback device

Install [VB-CABLE](https://vb-audio.com/Cable/) (donationware). It adds
**CABLE Input** (a speaker) and **CABLE Output** (a microphone).

### 2. The client

Download `cardmic-windows-x64.zip` from the
[latest release](https://github.com/Yueze/Cardmic/releases/latest), unzip it,
and in a terminal in that folder run:

```powershell
.\cardmic.exe doctor
```

It tests your audio devices and names the loopback it will use.

### 3. Use it

1. On the Cardputer, open Cardmic and connect to Wi-Fi (Settings > Wi-Fi). The
   PC must be on the same network.
2. Run `.\cardmic.exe run`. Allow it through Windows Defender Firewall on
   private networks when asked. The WIFI dot on the device turns green when
   the stream is live.
3. In your app, choose **CABLE Output (VB-Audio Virtual Cable)** as the
   microphone.

Leave `cardmic run` open while you talk. Press Ctrl-C to stop.

Networks that block broadcast need the device's address:
`.\cardmic.exe run --device 192.168.1.42:41234`. The address is shown in
Cardmic > Settings > Info.
