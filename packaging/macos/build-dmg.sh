#!/bin/sh
# Builds an unsigned disk image holding Tidewater.app and an Applications link.
# Usage: build-dmg.sh VERSION BINARY OUT.dmg
# The binary is the only thing packaged: no game data ever goes in.
set -eu
version=$1
binary=$2
out=$3
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

app="$work/dmg/Tidewater.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/tidewater"
chmod 755 "$app/Contents/MacOS/tidewater"
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"
# The app icon (Info.plist's CFBundleIconFile), drawn by packaging/icon/render.sh.
cp "$here/tidewater.icns" "$app/Contents/Resources/tidewater.icns"
# An ad-hoc signature (no certificate): not trusted by Gatekeeper, but Apple-silicon Macs refuse to run unsigned code.
codesign --force --deep -s - "$app"
ln -s /Applications "$work/dmg/Applications"
cp "$here/Read me first.txt" "$work/dmg/"

rm -f "$out"
hdiutil create -quiet -volname "Tidewater" -srcfolder "$work/dmg" -ov -format UDZO "$out"
echo "wrote $out"
