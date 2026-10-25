#!/bin/sh
# Builds the Linux download: the program, a .desktop file and its icon, a README and the licences.
# Usage: build-tarball.sh VERSION BINARY OUT.tar.gz
# The binary is the only program packaged: no game data ever goes in.
set -eu
version=$1
binary=$2
out=$3
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

name="tidewater-${version}-linux-x64"
mkdir -p "$work/$name"
cp "$binary" "$work/$name/tidewater"
chmod 755 "$work/$name/tidewater"
cp "$here/tidewater.desktop" "$here/tidewater.png" "$here/README.txt" "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$work/$name/"
tar -C "$work" -czf "$out" "$name"
echo "wrote $out"
