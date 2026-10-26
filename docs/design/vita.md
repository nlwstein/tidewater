# Tidewater on the PS Vita

**Bottom line.** Tidewater builds as a Vita app (`.vpk`) from the same shell code as the other platforms, and CI builds
it on every run. It starts in the Vita3K emulator and reaches the first-run screen. It has not been tried on a real
Vita, its buttons and sticks work through the gamepad support ([gamepad.md](gamepad.md)) but are untested on the
Vita, and we expect it to run slower than 60 fps.

The import is in [import.md](import.md), the touch gestures in [touch.md](touch.md) and the Android version in
[android.md](android.md).

## What you get

| Area | State |
|---|---|
| Build | Works on Linux and macOS (`platform/vita/build.sh`). CI builds the VPK on every run. |
| Vita3K | Starts and draws the first-run screen. Nothing past it was tried, because no game data was loaded. |
| Real Vita | Not tried. |
| Game files | Copied to `ux0:data/Tidewater`; the first-run screen scans that folder. |
| Saves and settings | `ux0:data/Tidewater`. |
| Front touch screen | Read as touch, like a phone. Not tried on hardware. |
| Buttons and sticks | Not read yet. |
| Speed | Unmeasured on a Vita. Expect 30 fps at best until the compositor gets cheaper. |
| Update check | Off. Install a newer VPK by hand. |
| GOG installer (`.exe`) | Not possible (see [Limits](#limits)). |

## Install

1. Download `tidewater-<version>-vita.vpk` from the [latest release](../../../../releases/latest). Each release also lists
   `SHA256SUMS`.
2. Copy it to the Vita (VitaShell's FTP server, USB, or a Memory Card reader), open it in **VitaShell**, and press
   Cross to install. The Vita must run custom firmware (HENkaku or Enso) that allows unsafe homebrew: the app asks for
   extended memory.
3. Copy your game files to `ux0:data/Tidewater` (see below), then start **Tidewater** from the LiveArea.

## Game files

Copy a game folder (a GOG install, or the folder holding `War2Dat.mpq`) or a CD image (`.iso`, `.bin`/`.cue`, `.mdf`,
`.nrg`) into `ux0:data/Tidewater`. Use VitaShell's FTP server or USB mode; the folder is made when the app first runs.
On the first-run screen, choose **Scan again**: it lists what it found, and choosing one copies the files once into
`ux0:data/Tidewater/game`. You can delete your copy afterwards.

Plan for about 700 MB of free space for the import, and the same again while your copy is still there.

## How it is put together

The same shell and core as every other platform (see [client-core.md](client-core.md)); only the build and a few
`target_os = "vita"` paths differ.

- **Toolchain.** The Vita target has no prebuilt standard library, so `platform/vita/build.sh` builds with a pinned
  nightly (`platform/vita/nightly-version`) and `-Z build-std`. The rest of the repository stays on the stable
  toolchain in `rust-toolchain.toml`. SDL 3 is built from source with the Vita SDK's CMake toolchain, as for the other
  targets.
- **Packaging.** The script turns the ELF into a `.velf`, then `eboot.bin`, writes `param.sfo` and packs the VPK with
  the icon (`platform/vita/sce_sys/icon0.png`, drawn by `packaging/icon/render.sh`). The title id is `TDWT00001`.
  `ATTRIBUTE2=12` asks for extended memory, which the 88 to 155 MB in-game footprint needs.
- **C stub** (`platform/vita/stub.c`). It answers `getpwuid_r` with "no such user", which Rust's standard library
  asks for and the Vita's C library lacks. It also sets `sceUserMainThreadStackSize` to 8 MB: the default main stack
  overflowed when the core was built. `build.sh` keeps that symbol with `-Wl,-u`.
- **Folders.** `storage.rs` uses `ux0:data/Tidewater` instead of the `directories` crate, because the app's own folder
  is read-only.
- **Keyboard.** The shell never opens the system keyboard at launch: on the Vita it is a modal screen that stops the
  game.
- **First run.** The screen uses the phone layout (`Intake::SharedFolder`) with only **Scan again** (and Back after a
  first import), since the Vita has no file dialogs.
- **CI.** The `vita` job in `ci.yml` installs the Vita SDK from its pinned snapshot (checked by SHA-256, cached; see
  `.github/actions/vitasdk`) and runs the script. `release.yml` does the same with `--version` and attaches the VPK
  to the release.

## Build it yourself

You need rustup, CMake and the [Vita SDK](https://vitasdk.org) (set `VITASDK` to its folder):

```sh
platform/vita/build.sh                       # platform/vita/build/tidewater.vpk
platform/vita/build.sh --version 1.2.3 --out dist/tidewater-vita.vpk
```

To try it in Vita3K: `Vita3K --frontend sdl platform/vita/build/tidewater.vpk` installs the VPK and runs it. The
emulator prints "Unhandled write protected region" lines; they are its own memory tracking and harmless.

## Limits

- **Not tried on a Vita.** Memory, speed, touch and the extended-memory request are all untested on hardware.
- **Controls untested on the Vita.** SDL's Vita backend presents the buttons and sticks as a gamepad, which the
  gamepad support ([gamepad.md](gamepad.md)) reads like any other; the build links SDL's joystick subsystem, but
  nobody has played with them on a Vita or in Vita3K yet. The Vita has no L2, R2 or stick presses, so Alt, the faster
  pointer, the system keyboard and the idle-worker button are not on its pad.
- **Speed.** The compositor draws a 640 by 480 picture on the CPU each frame. We expect about 30 fps. Ideas, cheapest
  first: run at 30 fps (the simulation's fixed step is unaffected), raise the CPU clock to 444 MHz, compose only the
  changed regions, convert an indexed frame on the GPU.
- **No GOG installer.** `innoextract` is a separate program and the Vita cannot start one. Unpack the installer on a
  computer first.
- **No system keyboard**, so no typing in chat or save names yet.
- **Network play** is untried (the Vita's `std::net` is partial).
- **No self-update.** Install a newer VPK over the old one; your game files and saves stay.
