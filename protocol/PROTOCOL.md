# Cardmic wire protocol

Everything runs over UDP port **41234** on the local network. All integers are
little-endian unless stated otherwise. The reference implementations are
[`firmware/main/apps/app_cardmic/cardmic_net.c`](../firmware/main/apps/app_cardmic/cardmic_net.c)
(device) and [`client/core/src/protocol.rs`](../client/core/src/protocol.rs)
(desktop).

> **Pairing.** With Settings > Pairing on (the default from 0.7.0), only
> computers that know the device's pairing code get audio, and it is
> encrypted with AES-128-GCM. With it off, anyone on the same network who
> sends discovery first can receive the audio, unencrypted. See section 3.
> A Cardputer that ran Cardmic 0.6 without pairing keeps it off
> after the update until it is turned on.

## 1. Discovery and keepalive

The client sends the 21-byte ASCII string

```
CPADV_MIC_DISCOVER_V1
```

(no trailing NUL) to UDP 41234: to each interface's subnet broadcast address,
to `255.255.255.255`, and optionally to a known device address. A paired
client sends the 45-byte v2 form instead (section 3).

Two rules matter as much as the byte layout:

1. **The device streams to the discovery packet's source address _and source
   port_.** Send discovery from the same socket that receives audio.
2. **Discovery is a keepalive.** The device drops the receiver 2.5 s after the
   last discovery packet. Clients resend every 700 ms, and once the device is
   streaming to them they send it to the device's address, not a broadcast:
   Wi-Fi acknowledges and retries unicast, and holds it for a device that is
   saving power, while broadcasts are sent once, unacknowledged, and often
   arrive late or not at all. A few lost in a row used to drop the stream.

The device sends live audio: with no receiver, captured audio is dropped
at once, and when the link falls behind, its short send queue (160 ms)
drops the oldest frame rather than the newest.

The device serves one receiver at a time. While a session is live, discovery
from any other address is ignored (first-receiver lock), except that with
pairing on a computer that answers the device's challenge takes over from a
receiver that has not (section 3).

### The device's name

On accepting a receiver, and then every 2 s while the session lasts, the
device (from 0.7.0) sends that receiver the ASCII datagram

```
CARDMIC_NAME <name>
```

with the name it goes by: the one set in Settings > Name, or `Cardmic-` plus
the last two bytes of its Wi-Fi MAC address. Names are at most 16 printable
ASCII characters without `;` or `=`. The datagram goes only to the receiver,
and with pairing on only once the receiver has answered a challenge
(section 3), so a replayed discovery does not learn it. It is not encrypted,
so it can be seen on the air by someone who can read other stations' Wi-Fi
traffic. Clients that do not know it ignore it, as any datagram that is not
an audio packet.

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

With pairing on, audio is sent as `CPM2` instead (section 3).

The client orders packets by sequence number, drops duplicates and late
packets, fills a gap of up to 5 lost packets (100 ms) with silence, and
resynchronises after a longer gap.

When the device is muted it keeps streaming silence, so the session stays up.

## 3. Pairing and encryption

The device shows a 12-character code in Settings > Pairing, e.g.
`7K2M-9QXB-4TPA` (Crockford base32: no I, L, O or U; 60 bits from the
hardware RNG). The user runs `cardmic pair 7K2M-9QXB-4TPA` once. Both sides
derive:

```
k = PBKDF2-HMAC-SHA256(code without dashes, salt "cardmic/pair/v1", 20000 rounds, 32 bytes)
enc_key = k[0..16]    AES-128-GCM key for audio
mac_key = k[16..32]   HMAC-SHA256 key for discovery
```

The slow derivation and the 60-bit code mean a recording of the traffic
cannot practically be brute-forced back to the code.

**Discovery v2** (45 bytes), sent by paired clients:

```
 0..20  "CPADV_MIC_DISCOVER_V2"
21..28  nonce      8 bytes, varies per packet
29..44  tag        HMAC-SHA256(mac_key, bytes 0..28), first 16 bytes
```

With pairing on, the device ignores v1 discovery and v2 discovery with a bad
tag (and shows "UNPAIRED PC" briefly). With pairing off it accepts both
forms and streams `CPM1`.

**Challenge and response** (device 0.7.0 and later, pairing on). A v2
discovery proves knowledge of the code, but not that its sender is live:
anyone on the network can record one and send it again. So the device
answers a valid v2 discovery with a challenge, unless the sender is already
its receiver and has answered:

```
challenge (33 bytes), device -> client
 0..16  "CARDMIC_CHALLENGE"
17..32  challenge   16 random bytes

response (50 bytes), client -> device, from the socket that sent discovery
 0..17  "CPADV_MIC_RESPONSE"
18..33  challenge   as received
34..49  tag         HMAC-SHA256(mac_key, bytes 0..33 | device IPv4 | device port)[..16]
```

The device IPv4 (4 bytes) and port (2 bytes, big-endian) are the address the
challenge came from, as the client sees it. The device checks the tag with
its own address, so a client that answers a challenge someone else relayed to
it computes a tag the device does not accept. A challenge is valid for 5 s,
from the address it was sent to, and answers once; the device sends a sender
its challenge at most every 500 ms and keeps the last four. It challenges a
receiver that has answered again every 20 s, and counts it as not answered
after 30 s without an answer, so a computer that left cannot be kept
"answered" by someone repeating its discovery from its address. A client
answers at most 8 challenges a second, since anyone can send one.

The session rules that follow:

- A sender that has **answered** takes the stream from a receiver that has
  not, but not from another that has (the first-receiver lock holds between
  answered receivers).
- A sender that has **not answered**, a recording or an app from before
  0.7.0, still gets the stream when no one else has it, encrypted as always,
  so older apps keep working. It loses it as soon as an answering computer
  asks.
- The device's name (section 1) goes only to a receiver that has answered.

Clients from before 0.7.0 ignore the challenge; devices from before 0.7.0
never send one, and a 0.7.0 client works with them as before.

**CPM2** (672 bytes), encrypted audio:

```
 0.. 3  magic         "CPM2"
 4      version       2
 5      flags         0
 6.. 7  sample_count  320
 8..11  sequence      u32
12..15  session       u32, random for each new receiver session
16..31  tag           AES-GCM tag
32..    ciphertext    AES-128-GCM(enc_key) of i16[320]

nonce (12 bytes)  = session (LE) | sequence (LE) | 00 00 00 00
associated data   = bytes 0..15 (the header)
```

A fresh random session id per receiver keeps nonces unique although the
sequence restarts at 0 for each session.

Test vectors for code `7K2M9QXB4TPA` are in
[`client/core/src/pairing.rs`](../client/core/src/pairing.rs).

### Refusals

A Cardputer with pairing on answers a discovery it cannot accept (version 1,
or version 2 with a tag that does not match its current code) with the
24-byte datagram `CARDMIC_PAIRING_REQUIRED`, at most once a second. A client
then knows to ask for the code instead of searching forever. The reply tells
only that pairing is on, which the device's screen shows anyway.

## 4. Screenshots

Used to produce the pictures in the docs.

Request, from any UDP port:

```
CARDMIC_SCREENSHOT [page]
```

`page` is optional: `main`, `settings`, `info`, `pairing` or `about`. With a
page, the device renders that page just for the picture, with placeholder
network details and a sample pairing code (so no real SSID, address or code
ends up in a screenshot), then returns to where it was. The pairing page is
never captured with its real code, and screenshots are disabled entirely
while pairing is on.

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

## 5. Firmware updates

Not a UDP protocol, but part of the contract with the device. The device's
About > Check update opens

```
https://github.com/Yueze/Cardmic/releases/latest/download/cardmic-ota.bin
```

reads the version from the image header, and installs the image only if it is
newer than the running one. A release must therefore attach the app image as
exactly `cardmic-ota.bin`, built with `PROJECT_VER` set to the release version.

## 6. Pairing over USB

Plugged in, the Cardputer is one USB device with two functions: the USB
Audio Class microphone and a HID interface. The HID interface carries a
vendor-defined collection (usage page `0xFF00`, usage `0x01`) with feature
report ID 3: 63 bytes of ASCII, NUL-padded.

```
CM1;pair=1;code=7K2M9QXB4TPA;name=Cardmic-05AC;fw=0.6.0
CM1;pair=0;name=Cardmic-05AC;fw=0.6.0          (pairing off: no code)
```

`name` is the name the microphone enumerated with: the device's name as in
section 1, except that a new name reaches USB, here and as the microphone's
product name (below), only the next time Cardmic opens. So `name` always
matches the name the computer lists the microphone under. The Cardmic app reads this report when a Cardputer is plugged in and stores the
code, so the computer is paired for Wi-Fi from then on: plugging in is the
act of trust. With the talk key on, the same interface also carries the
keyboard (report 1) and mouse (report 2), so no endpoint is added.

The microphone's USB product name is `Cardmic Microphone`, or the name set
in Settings > Name (from the next time Cardmic opens), so a computer lists a
renamed Cardputer under that name.

USB IDs: vendor `0xCAFE` (a placeholder until a registered one), product
`0x4015` (microphone), `0x4016` (microphone and talk key); development builds
`0x4017` and `0x4018` add a serial console.

