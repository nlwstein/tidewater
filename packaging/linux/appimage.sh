#!/bin/sh
# Builds the Linux AppImage: one executable file that runs on the Steam Deck and most other x86_64 Linux systems.
# Usage: appimage.sh VERSION BINARY OUT.AppImage
# Needs Linux on x86_64 (the tool and the runtime are x86_64 builds), curl, file, and sha256sum or shasum. It downloads
# appimagetool and the AppImage runtime at the pinned versions below and checks their SHA-256 before using them;
# set APPIMAGETOOL and APPIMAGE_RUNTIME to local copies to skip the download. Downloads are cached in
# $APPIMAGE_CACHE (default ~/.cache/tidewater-packaging).
#
# What goes in: the program (SDL 3 is linked into it statically), tidewater.desktop, the icon and the licences. No
# game data ever goes in. No libraries are bundled: the program's only shared libraries must be the C library family
# (glibc is never bundled: the AppImage runs on the glibc of the system it runs on, so build on the oldest system
# the AppImage should support), and the script refuses a binary that needs anything else. SDL opens the sound,
# display and gamepad libraries (ALSA, PulseAudio, X11, Wayland) at run time from the system, as on the tarball.
# The program replaces the file itself on "Update now" (crates/client/src/update.rs; $APPIMAGE is the file's path).
set -eu

TOOL_VERSION=1.9.1
TOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

[ $# -eq 3 ] || { echo "usage: $0 VERSION BINARY OUT.AppImage" >&2; exit 2; }
version=$1
binary=$2
out=$3
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

[ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] || { echo "appimage.sh needs Linux on x86_64" >&2; exit 1; }
[ -f "$binary" ] || { echo "no such program: $binary" >&2; exit 1; }

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# fetch URL SHA256 DEST: download unless DEST is already there with the right checksum.
fetch() {
  if [ -f "$3" ] && [ "$(sha256_of "$3")" = "$2" ]; then return 0; fi
  curl -fsSL --retry 3 -o "$3.part" "$1"
  got=$(sha256_of "$3.part")
  if [ "$got" != "$2" ]; then
    rm -f "$3.part"
    echo "checksum mismatch for $1: expected $2, got $got" >&2
    exit 1
  fi
  mv "$3.part" "$3"
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The program may only depend on the C library family. Anything else would have to be bundled (and its own
# dependencies with it), so stop and say so rather than ship an AppImage that fails on someone's machine.
needed=$(ldd "$binary" 2>&1 || true)
echo "$needed"
if echo "$needed" | grep -q 'not found'; then
  echo "the program has a library that is not found" >&2
  exit 1
fi
extra=$(echo "$needed" | grep -v 'statically linked' | awk '{print $1}' |
  grep -Ev '^(linux-vdso\.so|linux-gate\.so|/lib64/ld-linux-x86-64\.so|ld-linux-x86-64\.so|libc\.so|libm\.so|libdl\.so|libpthread\.so|librt\.so|libgcc_s\.so|libutil\.so|libresolv\.so)' || true)
if [ -n "$extra" ]; then
  echo "the program needs libraries the AppImage does not bundle: $extra" >&2
  exit 1
fi

cache=${APPIMAGE_CACHE:-${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}/tidewater-packaging}
mkdir -p "$cache"
tool=${APPIMAGETOOL:-}
if [ -z "$tool" ]; then
  tool=$cache/appimagetool-$TOOL_VERSION-x86_64.AppImage
  fetch "https://github.com/AppImage/appimagetool/releases/download/$TOOL_VERSION/appimagetool-x86_64.AppImage" "$TOOL_SHA256" "$tool"
fi
runtime=${APPIMAGE_RUNTIME:-}
if [ -z "$runtime" ]; then
  runtime=$cache/runtime-$RUNTIME_VERSION-x86_64
  fetch "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-x86_64" "$RUNTIME_SHA256" "$runtime"
fi
chmod +x "$tool"

# appimagetool is an AppImage itself and needs FUSE to run as one; unpacking it first works everywhere (CI runners,
# containers).
mkdir "$work/tool"
(cd "$work/tool" && "$tool" --appimage-extract >/dev/null)

appdir=$work/Tidewater.AppDir
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/icons/hicolor/256x256/apps" "$appdir/usr/share/doc/tidewater"
cp "$binary" "$appdir/usr/bin/tidewater"
chmod 755 "$appdir/usr/bin/tidewater"
# The desktop file without the tarball's comments about editing it by hand.
grep -v '^#' "$here/tidewater.desktop" > "$appdir/tidewater.desktop"
cp "$here/tidewater.png" "$appdir/tidewater.png"
cp "$here/tidewater.png" "$appdir/usr/share/icons/hicolor/256x256/apps/tidewater.png"
ln -s tidewater.png "$appdir/.DirIcon"
cp "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$appdir/usr/share/doc/tidewater/"
# The program is the entry point: no wrapper is needed with nothing bundled.
ln -s usr/bin/tidewater "$appdir/AppRun"

mkdir -p "$(dirname "$out")"
ARCH=x86_64 VERSION=$version "$work/tool/squashfs-root/AppRun" --runtime-file "$runtime" "$appdir" "$out"
chmod 755 "$out"
echo "wrote $out"
