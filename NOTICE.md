Cardmic
Copyright 2026 The Cardmic Authors

Licensed under the Apache License, Version 2.0 (see LICENSE). Releases up to
0.6.0-beta.2 were published under the MIT License.

# Third-party notices

## M5Cardputer-UserDemo

`firmware/` is a fork of M5Stack's factory firmware for the Cardputer ADV,
[m5stack/M5Cardputer-UserDemo](https://github.com/m5stack/M5Cardputer-UserDemo)
(branch `CardputerADV`), Copyright (c) 2025 M5Stack Technology CO LTD,
released under the MIT License as stated in its source headers
(`SPDX-License-Identifier: MIT`).

Cardmic's own code lives in `firmware/main/apps/app_cardmic/` plus small,
marked changes to `firmware/CMakeLists.txt`, `firmware/partitions.csv`,
`firmware/sdkconfig.defaults`, `firmware/main/main.cpp`,
`firmware/main/apps/apps.h`, `firmware/main/hal/hal.{h,cpp}` and
`firmware/main/hal/keyboard/keyboard.{h,cpp}` (model detection and the
original Cardputer's GPIO-matrix keyboard, ported from M5Stack's firmware for
that board, [M5Cardputer-UserDemo](https://github.com/m5stack/M5Cardputer-UserDemo)
branch `main`, MIT). Everything else is upstream code, kept as is.

The upstream firmware in turn builds on these projects (see their licenses):

- [M5GFX](https://github.com/m5stack/M5GFX) and [M5Unified](https://github.com/m5stack/M5Unified), M5Stack
- [mooncake](https://github.com/Forairaaaaa/mooncake), [mooncake_log](https://github.com/Forairaaaaa/mooncake_log), [smooth_ui_toolkit](https://github.com/Forairaaaaa/smooth_ui_toolkit), Forairaaaaa
- [PikaPython](https://github.com/pikasTech/PikaPython), [RadioLib](https://github.com/jgromes/RadioLib), [TinyGPSPlus](https://github.com/mikalhart/TinyGPSPlus), [Adafruit_TCA8418](https://github.com/adafruit/Adafruit_TCA8418), [raylib](https://github.com/raysan5/raylib)
- [TinyUSB](https://github.com/hathach/tinyusb) and [ESP-IDF](https://github.com/espressif/esp-idf), via the Espressif component registry

M5Stack, Cardputer and related marks belong to their owners. Cardmic is an
independent project and is not affiliated with or endorsed by M5Stack.
