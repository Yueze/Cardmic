#!/bin/bash
# Build Cardmic.app and Cardmic.dmg.
#
#   tray/macos/bundle.sh [--universal]
#
# Run from client/. Without --universal it builds for this Mac only. Set
# CODESIGN_IDENTITY to a "Developer ID Application" identity to sign for
# distribution; otherwise the app is signed ad hoc (runs here, and on other
# Macs after "Open Anyway" in System Settings > Privacy & Security).
# With a Developer ID, also set NOTARY_KEY (path to an App Store Connect API
# key, .p8), NOTARY_KEY_ID and NOTARY_ISSUER to notarize the app and the disk
# image, so they open without any warning.
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

# A Developer ID signature carries a secure timestamp (notarization needs it);
# an ad-hoc one cannot.
if [ -n "${CODESIGN_IDENTITY:-}" ]; then TIMESTAMP=--timestamp; else TIMESTAMP=--timestamp=none; fi
NOTARIZE=
if [ -n "${CODESIGN_IDENTITY:-}" ] && [ -n "${NOTARY_KEY:-}" ]; then NOTARIZE=1; fi
notarize() {
  xcrun notarytool submit "$1" --key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER" --wait
}

codesign --force --options runtime $TIMESTAMP \
  --sign "${CODESIGN_IDENTITY:--}" "$APP/Contents/Resources/cardmic"
# CARDMIC_LOCAL_DEV=1: an ad-hoc signature that names only the bundle ID as
# its identity, so macOS keeps the microphone permission from one local build
# to the next instead of asking again after every rebuild. Not for builds
# that leave this machine: there, the default (the build's own hash) is safer.
REQ=()
if [ -n "${CARDMIC_LOCAL_DEV:-}" ] && [ -z "${CODESIGN_IDENTITY:-}" ]; then
  REQ=(-r='designated => identifier "io.github.yueze.cardmic"')
fi
codesign --force --options runtime $TIMESTAMP \
  --entitlements tray/macos/Cardmic.entitlements ${REQ[@]+"${REQ[@]}"} \
  --sign "${CODESIGN_IDENTITY:--}" "$APP"

# Notarize the app and staple the ticket to it, so it opens without a network
# check wherever it is copied, including when it updates itself.
if [ -n "$NOTARIZE" ]; then
  ditto -c -k --keepParent "$APP" "$OUT/notarize.zip"
  notarize "$OUT/notarize.zip"
  rm "$OUT/notarize.zip"
  xcrun stapler staple "$APP"
fi

# Disk image: the app next to an Applications shortcut.
STAGE=$OUT/dmg
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "Cardmic $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$OUT/Cardmic.dmg"
rm -rf "$STAGE"
if [ -n "${CODESIGN_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$OUT/Cardmic.dmg"
  if [ -n "$NOTARIZE" ]; then
    notarize "$OUT/Cardmic.dmg"
    xcrun stapler staple "$OUT/Cardmic.dmg"
  fi
fi
echo "$APP"
echo "$OUT/Cardmic.dmg"
