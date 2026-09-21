#!/usr/bin/env bash
# Package a built firmware into release assets, in dist/:
#   cardmic-ota.bin               app image; the device's OTA updater fetches
#                                 exactly this name from the latest release
#   cardmic-<version>-full.bin    bootloader + partition table + app, flash at 0x0
# Run from firmware/ after `idf.py build`, with ESP-IDF exported.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^set(PROJECT_VER "\(.*\)")$/\1/p' CMakeLists.txt)
[ -n "$version" ] || { echo "PROJECT_VER not found in CMakeLists.txt" >&2; exit 1; }

rm -rf dist && mkdir dist
cp build/cardputer-adv.bin dist/cardmic-ota.bin
python -m esptool --chip esp32s3 merge_bin -o "dist/cardmic-${version}-full.bin" \
    --flash_mode dio --flash_freq 80m --flash_size 8MB \
    0x0 build/bootloader/bootloader.bin \
    0x8000 build/partition_table/partition-table.bin \
    0xd000 build/ota_data_initial.bin \
    0x10000 build/cardputer-adv.bin
(cd dist && sha256sum * > SHA256SUMS 2>/dev/null || shasum -a 256 * > SHA256SUMS)
ls -l dist
