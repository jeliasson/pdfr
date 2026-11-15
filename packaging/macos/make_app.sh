#!/bin/bash
# Builds pdfr.app (Apple silicon and Intel in one), a .zip and a .dmg.
# Run by the release workflow on macOS (or on a Mac to try it):
#   packaging/macos/make_app.sh <version> <out-dir>
# Signs and notarizes when APPLE_CERTIFICATE (base64 .p12),
# APPLE_CERTIFICATE_PASSWORD, APPLE_SIGNING_IDENTITY, APPLE_ID, APPLE_TEAM_ID
# and APPLE_APP_PASSWORD are set; otherwise the app is signed ad hoc (users
# then right-click → Open the first time, or install with Homebrew).
set -euo pipefail
v=${1:?version}; out=${2:?out dir}
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
mkdir -p "$out"

for t in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --locked --release -p pdfr --target "$t"
done

app="$out/pdfr.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/pdfr" \
  target/aarch64-apple-darwin/release/pdfr target/x86_64-apple-darwin/release/pdfr
sed "s/@VERSION@/$v/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp assets/icon/pdfr.icns "$app/Contents/Resources/pdfr.icns"
cp LICENSE.md assets/LICENSE-Inter.txt "$app/Contents/Resources/"

if [ -n "${APPLE_CERTIFICATE:-}" ]; then
  kc="$RUNNER_TEMP/pdfr.keychain-db"
  security create-keychain -p pdfr "$kc"
  security set-keychain-settings -lut 21600 "$kc"
  security unlock-keychain -p pdfr "$kc"
  echo "$APPLE_CERTIFICATE" | base64 --decode > "$RUNNER_TEMP/cert.p12"
  security import "$RUNNER_TEMP/cert.p12" -k "$kc" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple: -k pdfr "$kc"
  security list-keychains -d user -s "$kc"
  codesign --force --options runtime --timestamp -s "$APPLE_SIGNING_IDENTITY" "$app"
else
  codesign --force -s - "$app"
fi

zip="$out/pdfr-v$v-macos.zip"
ditto -c -k --keepParent "$app" "$zip"
if [ -n "${APPLE_ID:-}" ]; then
  xcrun notarytool submit "$zip" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_PASSWORD" --wait
  xcrun stapler staple "$app"
  rm "$zip"; ditto -c -k --keepParent "$app" "$zip"
fi

stage=$(mktemp -d)
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname pdfr -srcfolder "$stage" -ov -format UDZO "$out/pdfr-v$v-macos.dmg"
rm -rf "$stage" "$app"
ls -la "$out"
