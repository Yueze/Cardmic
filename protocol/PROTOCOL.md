# Cardmic wire protocol

Everything runs over UDP port **41234** on the local network. All integers are
little-endian unless stated otherwise. The reference implementations are
[`firmware/main/apps/app_cardmic/cardmic_net.c`](../firmware/main/apps/app_cardmic/cardmic_net.c)
(device) and [`client/core/src/protocol.rs`](../client/core/src/protocol.rs)
(desktop).

> **No encryption yet.** Anyone on the same network who sends discovery first
> can receive the audio. Use wireless mode on networks you trust. Pairing is
> on the roadmap; the `CPM2` framing below reserves room for it.

## 1. Discovery and keepalive

The client sends the 21-byte ASCII string

```
CPADV_MIC_DISCOVER_V1
```

(no trailing NUL) to UDP 41234: to each interface's subnet broadcast address,
to `255.255.255.255`, and optionally to a known device address.

Two rules matter as much as the byte layout:

1. **The device streams to the discovery packet's source address _and source
   port_.** Send discovery from the same socket that receives audio.
2. **Discovery is a keepalive.** The device drops the receiver 2.5 s after the
   last discovery packet. Clients resend every 700 ms.

The device serves one receiver at a time. While a session is live, discovery
from any other address is ignored (first-receiver lock).

## 2. Audio packets

16 kHz, mono, signed 16-bit PCM, 20 ms (320 samples) per packet.

```
CPM1 (656 bytes)
 0.. 3  magic         "CPM1"
 4      version       1
 5      flags         0
 6.. 7  sample_count  320
 8..11  sequence      u32, starts at 0 for each new receiver
12..15  sample_rate   u32, 16000
16..    samples       i16[320]
```

`CPM2` (672 bytes) is reserved for pairing. It keeps the same 16-byte header
with magic `"CPM2"` and version 2, then a 16-byte authentication tag, then the
samples. The client already parses it; the firmware does not send it yet.

The client orders packets by sequence number, drops duplicates and late
packets, fills a gap of up to 5 lost packets (100 ms) with silence, and
resynchronises after a longer gap.

When the device is muted it keeps streaming silence, so the session stays up.

## 3. Screenshots

Used to produce the pictures in the docs.

Request, from any UDP port:

```
CARDMIC_SCREENSHOT [page]
```

`page` is optional: `main`, `settings`, `info` or `about`. With a page, the
device renders that page just for the picture, with placeholder network
details (so no real SSID or address ends up in a screenshot), then returns to
where it was.

The device answers from an ephemeral port to the requester with one packet
per two rows of the full 240x135 screen:

```
 0.. 3  magic     "CMSS"
 4.. 5  width     240
 6.. 7  height    135
 8.. 9  y         first row in this packet
10..11  rows      rows in this packet (1 or 2)
12..15  reserved  0
16..    pixels    rows * width RGB565 values, big-endian
```

`cardmic screenshot DEVICE_IP [--page NAME]` implements the client side.

## 4. Firmware updates

Not a UDP protocol, but part of the contract with the device. The device's
About > Check update opens

```
https://github.com/Yueze/Cardmic/releases/latest/download/cardmic-ota.bin
```

reads the version from the image header, and installs the image only if it is
newer than the running one. A release must therefore attach the app image as
exactly `cardmic-ota.bin`, built with `PROJECT_VER` set to the release version.
