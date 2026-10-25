#!/bin/sh
# Builds the unsigned device app and packs it as an .ipa (docs/design/ios.md, "Install the unsigned IPA").
#
#   platform/ios/package-ipa.sh [--version X.Y.Z] [--out FILE]
#
# Run `platform/ios/build.sh device` first (the device library and the Xcode project). Nothing is signed: whoever
# installs the file signs it with their own Apple ID (AltStore, SideStore, Sideloadly, or re-signing by hand).
# --version: sets CFBundleShortVersionString and CFBundleVersion in the built app (a "-suffix" such as -dryrun is cut,
#            because iOS wants digits and dots); without it project.yml's values stay.
# --out:     the file to write (default platform/ios/Tidewater.ipa).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
version=
out="$here/Tidewater.ipa"
while [ $# -gt 0 ]; do
    case "$1" in
        --version) version=${2:?--version needs a value}; shift 2 ;;
        --out) out=${2:?--out needs a value}; shift 2 ;;
        *)
            echo "usage: $0 [--version X.Y.Z] [--out FILE]" >&2
            exit 2
            ;;
    esac
done
case "$out" in /*) ;; *) out="$PWD/$out" ;; esac

echo "==> Tidewater.app for a device, unsigned"
(cd "$here" && xcodebuild -project Tidewater.xcodeproj -scheme Tidewater -configuration Release \
    -sdk iphoneos -destination 'generic/platform=iOS' -derivedDataPath DerivedData \
    CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY="" build -quiet)
app="$here/DerivedData/Build/Products/Release-iphoneos/Tidewater.app"
[ -d "$app" ] || { echo "no app at $app" >&2; exit 1; }

if [ -n "$version" ]; then
    plain=${version%%-*}
    echo "==> version $plain"
    /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $plain" -c "Set :CFBundleVersion $plain" "$app/Info.plist"
fi

echo "==> $out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/Payload"
cp -R "$app" "$work/Payload/Tidewater.app"
mkdir -p "$(dirname "$out")"
rm -f "$out"
(cd "$work" && zip -qry "$out" Payload)
echo "built: $out"
