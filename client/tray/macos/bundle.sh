#!/bin/bash
# Build Cardmic.app and Cardmic.dmg.
#
#   tray/macos/bundle.sh [--universal]
#
# Run from client/. Without --universal it builds for this Mac only. Set
# CODESIGN_IDENTITY to a "Developer ID Application" identity to sign for
# distribution; otherwise the app is signed ad hoc (runs here, and on other
# Macs after "Open Anyway" in System Settings > Privacy & Security).
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT=target/macos
APP=$OUT/Cardmic.app
rm -rf "$OUT" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

if [ "${1:-}" = "--universal" ]; then
  cargo build --release -p cardmic-tray -p cardmic --target aarch64-apple-darwin
  cargo build --release -p cardmic-tray -p cardmic --target x86_64-apple-darwin
  lipo -create -output "$APP/Contents/MacOS/Cardmic" \
    target/aarch64-apple-darwin/release/cardmic-app target/x86_64-apple-darwin/release/cardmic-app
  lipo -create -output "$APP/Contents/Resources/cardmic" \
    target/aarch64-apple-darwin/release/cardmic target/x86_64-apple-darwin/release/cardmic
else
  cargo build --release -p cardmic-tray -p cardmic
  cp target/release/cardmic-app "$APP/Contents/MacOS/Cardmic"
  cp target/release/cardmic "$APP/Contents/Resources/cardmic"
fi
cp tray/assets/Cardmic.icns "$APP/Contents/Resources/"
sed "s/@VERSION@/$VERSION/g" tray/macos/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

codesign --force --options runtime --timestamp=none \
  --sign "${CODESIGN_IDENTITY:--}" "$APP/Contents/Resources/cardmic"
# CARDMIC_LOCAL_DEV=1: an ad-hoc signature that names only the bundle ID as
# its identity, so macOS keeps the microphone permission from one local build
# to the next instead of asking again after every rebuild. Not for builds
# that leave this machine: there, the default (the build's own hash) is safer.
REQ=()
if [ -n "${CARDMIC_LOCAL_DEV:-}" ] && [ -z "${CODESIGN_IDENTITY:-}" ]; then
  REQ=(-r='designated => identifier "io.github.yueze.cardmic"')
fi
codesign --force --options runtime --timestamp=none \
  --entitlements tray/macos/Cardmic.entitlements "${REQ[@]}" \
  --sign "${CODESIGN_IDENTITY:--}" "$APP"

# Disk image: the app next to an Applications shortcut.
STAGE=$OUT/dmg
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "Cardmic $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$OUT/Cardmic.dmg"
rm -rf "$STAGE"
echo "$APP"
echo "$OUT/Cardmic.dmg"
