#!/bin/sh
# Builds the PS Vita app (docs/design/vita.md).
#
#   platform/vita/build.sh [--version X.Y.Z] [--out FILE]
#
# Builds the program for armv7-sony-vita-newlibeabihf with the pinned nightly (nightly-version) and `-Z build-std`
# (the target has no prebuilt standard library), then packs it the way the Vita expects: velf, eboot.bin, param.sfo
# and the icon in a .vpk. The default output is platform/vita/build/tidewater.vpk; --out FILE copies it elsewhere.
# --version X.Y.Z sets the version the Vita shows (APP_VER, "XX.YY": major and minor, each under 100; 00.00 without).
#
# Needs rustup, CMake, and VITASDK (the folder of the Vita SDK, https://vitasdk.org; its bin folder holds
# arm-vita-eabi-gcc and vita-pack-vpk). Install the SDK from its official release for your system; CI does.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
version=""
out=""
while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            version=$2
            shift 2
            ;;
        --out)
            out=$2
            shift 2
            ;;
        *)
            echo "usage: $0 [--version X.Y.Z] [--out FILE]" >&2
            exit 2
            ;;
    esac
done

: "${VITASDK:?set VITASDK to the Vita SDK folder}"
export PATH="$VITASDK/bin:$PATH"
for tool in arm-vita-eabi-gcc vita-elf-create vita-make-fself vita-mksfoex vita-pack-vpk; do
    command -v "$tool" > /dev/null || {
        echo "error: $tool not found in $VITASDK/bin" >&2
        exit 1
    }
done

# The repository pins its stable toolchain in rust-toolchain.toml; this lane needs a nightly, named by date.
nightly=$(cat "$here/nightly-version")
rustup toolchain install "$nightly" --profile minimal --component rust-src > /dev/null

target=armv7-sony-vita-newlibeabihf
build="$here/build"
rm -rf "$build"
mkdir -p "$build"

arm-vita-eabi-gcc -c -O2 "$here/stub.c" -o "$build/stub.o"

# The Rust target's tools: the Vita's compiler and linker, and the SDK's CMake toolchain file (sdl3-sys builds SDL from
# source). `-u` keeps the linker from dropping the stack-size symbol.
lower=$(echo "$target" | tr - _)
upper=$(echo "$lower" | tr '[:lower:]' '[:upper:]')
export "CMAKE_TOOLCHAIN_FILE_$lower=$VITASDK/share/vita.toolchain.cmake"
export "CC_$lower=arm-vita-eabi-gcc"
export "AR_$lower=arm-vita-eabi-ar"
export "CARGO_TARGET_${upper}_LINKER=arm-vita-eabi-gcc"
export "CARGO_TARGET_${upper}_RUSTFLAGS=-C link-arg=$build/stub.o -C link-arg=-Wl,-u,sceUserMainThreadStackSize"

cd "$root"
cargo "+$nightly" build --locked --release -Z build-std=std,panic_abort --target "$target" \
    -p tidewater-client --bin tidewater
elf="$root/target/$target/release/tidewater.elf"
[ -f "$elf" ] || elf="$root/target/$target/release/tidewater"

vita-elf-create "$elf" "$build/tidewater.velf" > /dev/null
vita-make-fself -c -s "$build/tidewater.velf" "$build/eboot.bin"

appver=00.00
if [ -n "$version" ]; then
    major=${version%%.*}
    rest=${version#*.}
    minor=${rest%%.*}
    appver=$(printf '%02d.%02d' "$major" "$minor")
fi
# ATTRIBUTE2=12 asks for the extended memory mode (the program needs more than the default budget).
vita-mksfoex -s TITLE_ID=TDWT00001 -s APP_VER="$appver" -d ATTRIBUTE2=12 Tidewater "$build/param.sfo"

vita-pack-vpk -s "$build/param.sfo" -b "$build/eboot.bin" -a "$here/sce_sys/icon0.png=sce_sys/icon0.png" \
    "$build/tidewater.vpk"
echo "built $build/tidewater.vpk"
if [ -n "$out" ]; then
    mkdir -p "$(dirname "$out")"
    cp "$build/tidewater.vpk" "$out"
fi
