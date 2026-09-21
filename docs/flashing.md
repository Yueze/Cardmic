# Installing the firmware

Cardmic ships as M5Stack's factory firmware with the Cardmic app added. You
keep the stock boot screen, launcher and every stock app. You flash over USB
**once**; after that, updates install over Wi-Fi from the device itself
(Cardmic > Settings > About > Check update).

> Flashing replaces whatever firmware is on the device and resets the stock
> settings (saved Wi-Fi, etc.).

## What you need

- An M5Stack **Cardputer ADV**
- A USB-C cable **that carries data**. Many cables only charge. If your
  computer never sees the device, try another cable first.
- Python 3 with esptool: `pip install esptool`
- `cardmic-<version>-full.bin` from the
  [latest release](https://github.com/Yueze/cardmic/releases/latest)

## 1. Put the Cardputer in download mode

This is M5Stack's documented procedure:

1. Slide the power switch to **OFF**.
2. Press and hold the **G0** button on the top edge.
3. While holding G0, plug the USB cable into the computer.
4. Release G0. The screen stays dark; that is expected.

## 2. Find the serial port

| OS | Port looks like | How to list |
|----|-----------------|-------------|
| macOS | `/dev/cu.usbmodem101` | `ls /dev/cu.usbmodem*` |
| Windows | `COM5` | Device Manager > Ports (COM & LPT) |
| Linux | `/dev/ttyACM0` | `ls /dev/ttyACM*` |

## 3. Flash

```bash
python -m esptool --chip esp32s3 -p PORT write_flash 0x0 cardmic-0.5.0-full.bin
```

When it prints `Hash of data verified`, unplug the cable, slide the power
switch to ON, and pick **Cardmic** in the launcher.

## Updating later

- **Over Wi-Fi (normal):** Cardmic > Settings (`S`) > About > `Enter`. The device
  checks GitHub for a newer release and installs it. If a new version fails to
  boot, the device rolls back to the previous one automatically.
- **Over USB:** repeat the steps above with the newer `-full.bin`.

## Troubleshooting

- **No serial port appears.** Use a data cable, and a port directly on the
  computer rather than a hub. On macOS, `ioreg -p IOUSB` should list an
  "USB JTAG/serial debug unit" while the device is in download mode.
- **`Failed to connect to ESP32-S3`.** Redo step 1: power switch OFF first,
  then hold G0 while plugging in.
- **Back to the stock firmware.** Flash M5Stack's factory image with
  [M5Burner](https://docs.m5stack.com/en/download), or build `firmware/` from
  the upstream repository.
