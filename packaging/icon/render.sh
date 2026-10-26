#!/bin/sh
# Redraws every platform's app icon from packaging/icon/tidewater.svg (our own artwork), with `twtool app-icon`.
#
#   packaging/icon/render.sh
#
# The results are committed, so a release build never needs this; run it after changing the SVG and commit what it
# writes. "Rounded" is the macOS grid (a rounded square of 824/1024 with a clear margin), also used on Windows and
# Linux and for the window; "square" is edge to edge for the platforms that cut their own shape (iOS, Android's
# adaptive icon).
#
#   platform/ios/Sources/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png   iOS: 1024, square, no alpha
#   platform/android/app/src/main/res/mipmap-*/ic_launcher_background.png      Android 8+: adaptive icon layer,
#                                                                               108 dp, the scene zoomed out to 0.7
#   platform/android/app/src/main/res/mipmap-*/ic_launcher.png                 Android 7: 48 dp, rounded
#   packaging/macos/tidewater.icns                                               macOS: 16 to 1024, rounded
#   packaging/windows/tidewater.ico                                              Windows: 16 to 256, rounded
#   packaging/linux/tidewater.png                                                Linux: 256, rounded
#   crates/client/assets/icon/window.png                                         the desktop window: 256, rounded
#   platform/vita/sce_sys/icon0.png                                              Vita: 128, square (the Vita rounds it)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
svg="$here/tidewater.svg"

(cd "$root" && cargo build --quiet --release -p twtool)
twtool="$root/target/release/twtool"

icon() { # icon OUT [twtool options]...
    out=$1
    shift
    mkdir -p "$(dirname "$out")"
    "$twtool" app-icon "$out" --layer "$svg" "$@"
}

icon "$root/platform/ios/Sources/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png" --size 1024 --opaque

res="$root/platform/android/app/src/main/res"
for d in mdpi:1 hdpi:1.5 xhdpi:2 xxhdpi:3 xxxhdpi:4; do
    name=${d%%:*}
    k=${d#*:}
    icon "$res/mipmap-$name/ic_launcher_background.png" --size "$(echo "108 * $k / 1" | bc)" --scale 0.7
    icon "$res/mipmap-$name/ic_launcher.png" --size "$(echo "48 * $k / 1" | bc)" --shape rounded
done

icon "$root/packaging/macos/tidewater.icns" --shape rounded --size 16 --size 32 --size 64 --size 128 --size 256 \
    --size 512 --size 1024
icon "$root/packaging/windows/tidewater.ico" --shape rounded --size 16 --size 20 --size 24 --size 32 --size 40 \
    --size 48 --size 64 --size 256
icon "$root/packaging/linux/tidewater.png" --shape rounded --size 256
icon "$root/crates/client/assets/icon/window.png" --shape rounded --size 256
icon "$root/platform/vita/sce_sys/icon0.png" --size 128
