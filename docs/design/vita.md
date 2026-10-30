# Tidewater on the PS Vita

**Bottom line.** Tidewater builds as a Vita app (`.vpk`) from the same shell code as the other platforms, and CI builds
it on every run. In the Vita3K emulator it plays: the videos, menus and a custom game work with the Vita's buttons
and sticks, and every frame-test scene gives the same frames and sound as on the desktop. One player has installed and
played it on a real Vita (GitHub issue #3); the cursor and the scrolling move in
steps there, which looks like a low frame rate, and a test build is measuring it.

The import is in [import.md](import.md), the touch gestures in [touch.md](touch.md) and the Android version in
[android.md](android.md).

## What you get

| Area | State |
|---|---|
| Build | Works on Linux and macOS (`platform/vita/build.sh`). CI builds the VPK on every run. |
| Vita3K | Plays. All 33 game scenes of the frame test match the desktop snapshots, frames and sound (`firstrun` differs on purpose, see [Test builds](#test-builds)). Played by hand with the pad: videos, menus, a custom game. |
| Real Vita | Installs (v0.9.1) and plays, by one player so far. The pointer and the scrolling move in steps; the frame rate is being measured (GitHub issue #3). |
| Game files | Copied to `ux0:data/Tidewater`; the first-run screen scans that folder. |
| Saves and settings | `ux0:data/Tidewater`. A first run starts with Widescreen at `auto` (the Vita's 16:9 screen); Settings, General changes it. |
| Front touch screen | Read as touch, like a phone. Not tried on hardware. |
| Buttons and sticks | Read as a gamepad (SDL's built-in "PSVita Controller" mapping). Tried in Vita3K: click, right click, panel mode, the minimap jump, the game menu, pause and scrolling work. |
| Speed | Unmeasured on a Vita. Vita3K on a fast Mac holds 60 fps with 10 to 12 ms of work a frame in a game, which says little about the hardware. Expect 30 fps at best until the compositor gets cheaper. |
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

Plan for about 500 MB of free space for the import, and the same again while your copy is still there.

## How it is put together

The same shell and core as every other platform (see [client-core.md](client-core.md)); only the build and a few
`target_os = "vita"` paths differ.

- **Toolchain.** The Vita target has no prebuilt standard library, so `platform/vita/build.sh` builds with a pinned
  nightly (`platform/vita/nightly-version`) and `-Z build-std`. The rest of the repository stays on the stable
  toolchain in `rust-toolchain.toml`. SDL 3 is built from source with the Vita SDK's CMake toolchain, as for the other
  targets.
- **Packaging.** The script turns the ELF into a `.velf`, then `eboot.bin`, writes `param.sfo` and packs the VPK with
  the icon (`platform/vita/sce_sys/icon0.png`, drawn by `packaging/icon/render.sh` as an 8-bit indexed PNG: a real Vita
  refuses any other LiveArea image at install with error 0x8010113D, which Vita3K does not check; a `twtool` test guards it). The title id is `TDWT00001`.
  `ATTRIBUTE2=12` asks for extended memory, which the 88 to 155 MB in-game footprint needs.
- **C stub** (`platform/vita/stub.c`). It answers `getpwuid_r` with "no such user", which Rust's standard library
  asks for and the Vita's C library lacks. It also sets `sceUserMainThreadStackSize` to 8 MB: the default main stack
  overflowed when the core was built. `build.sh` keeps that symbol with `-Wl,-u`. And it sets
  `_newlib_heap_size_user` to 256 MiB: newlib's default heap is 128 MiB whatever the memory budget, and the start-up
  videos with the music behind them went past it.
- **Linker headroom.** `build.sh` defines `__sce_headroom` (16 KiB), which the SDK's linker script leaves between the
  code and the data. `vita-elf-create` puts the module's own data there; without it, packaging failed whenever the
  code happened to end just below a 64 KiB boundary.
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

### Test builds

A Vita app gets no command line (Vita3K's `--app-args` does not reach it either), so a test build bakes one in:

```sh
# One scene, fixed at build time:
platform/vita/build.sh --args "--script firstrun --script-out ux0:data/TWtest/firstrun --headless"
# Any command line, read from a file when the app starts, so one build runs every scene:
platform/vita/build.sh --args "@ux0:data/TWtest/args.txt" --env "TIDEWATER_NO_BEACON=1"
```

`--env "NAME=value ..."` sets environment variables first, such as `TIDEWATER_SCRIPT_PICTURES=0` (save each shot as
a PNG) or `TIDEWATER_FRAME_STATS=1` (the frame times every 10 seconds; a path such as
`ux0:data/Tidewater/frame-stats.txt` in place of `1` also adds them to that file, since a real Vita has no
console). With `@FILE`, a missing file means no arguments: the normal program. Release
builds carry neither option. A test build runs the same on a real Vita: copy the files there with VitaShell and read
the results back from `ux0:data/TWtest`.

To compare with the desktop, copy the imported game folder to `ux0:data/Tidewater/game`, run a scene with
`--install ux0:data/Tidewater/game --script NAME --script-out ux0:data/TWtest/NAME --headless`, and diff the
`NAME.txt` and `NAME-audio.txt` it writes against `crates/client/tests/snapshots/`. The `firstrun` scene differs on
purpose: the Vita's first-run screen has its own text and only **Scan again**.

Scenes run only headless here. With a window, the scripted run waits for a frame shown at one window pixel per screen
pixel, and the Vita's window is always the whole 960 by 544 screen, so it never ends.

## Speed

Measured on the real Vita with `TIDEWATER_FRAME_STATS` on a 2-player Gold Rush custom game at the 864-pixel
`widescreen` width: the game holds 60 fps (frame interval p99 16.8 ms over 600 frames, the frame's own work 11.3 ms on
average and 15.8 ms at p99). What gets it there, all of it on every platform unless it says otherwise:

- **Clocks.** The shell raises the CPU to 444 MHz, and the GPU to 222 MHz, the most an app may ask for
  (`raise_vita_clocks` in `shell/sdl.rs`; the system's defaults for an app are 333 and 166).
- **The GPU draws the layers.** On the Vita the shell draws the core's draw list with the renderer's textures
  (`shell/gpu.rs`) instead of composing the frame on the CPU (`compose/cpu.rs`, still the default elsewhere and in
  every scripted run). `TIDEWATER_COMPOSITOR=gpu` or `cpu` chooses on any platform; the window comparison in
  `tests/frames.rs` checks the GPU's picture against the CPU's.
- **Only what changed.** The map view, the interface and the terrain record the parts of their pictures that changed
  (`layers::Pictures::get_mut_parts`); the view and the interface colour only those, and the GPU uploads only those,
  since writing texture memory is slow on the Vita (about 180 MB/s). A palette step redraws only the parts that use
  a changed colour.
- **Pausing.** The pause's greyscale changes every colour. The terrain recolours only the tiles in view and greys the
  rest as they scroll in (`game::freshen_terrain`); the interface keeps its picture as drawn without the greyscale and
  puts it over that (`game::Hud::plain`, `palette::apply_from`) rather than drawing every part again; the map view
  colours itself whole. Pausing or resuming takes one frame of about 90 or 65 ms (178 ms before).
- **The touch pulse is drawn by the GPU too.** The long-press pulse is the draw list's last item
  (`compose::Source::Mark`) rather than painted into a composed frame, so a long press keeps 60 fps (it took frames of
  85 ms before).
- **No division in hot loops.** The Vita's CPU has no divide instruction, so the sound's resampling and the music's
  decoding step instead of dividing.
- **Music off the frame.** A track is read and decoded on a thread of its own (`jukebox.rs`), so a game no longer waits
  some 4 seconds for its music before it starts.
- **Faster decoders.** The archive's Huffman and PKWARE decoders read a code through a lookup table rather than a bit
  at a time (`mpq_codec/`); reading a battle track takes half the time and the unit sprites a third.

What is left at a game's start is mostly reading files: the Vita's memory card gave about 4 MB/s, and the unit sprites
are 2.4 MB.

## Limits

- **One real Vita so far** (an LCD model). The first hardware install, of v0.9.0, failed with error
  0x8010113D (the icon was a 32-bit PNG; fixed in v0.9.1). Memory is not measured on hardware yet.
- **Controls tuned only in Vita3K.** SDL's Vita backend presents the buttons and sticks as a gamepad, which the
  gamepad support reads like any other; which buttons the Vita lacks is in [gamepad.md](gamepad.md).
- **Starting a game takes about 3 seconds** on the Vita (see "Speed" below), most of it reading the unit sprites from
  the memory card.
- **No GOG installer.** `innoextract` is a separate program and the Vita cannot start one. Unpack the installer on a
  computer first.
- **No system keyboard**, so no typing in chat or save names yet.
- **Network play** is untried (the Vita's `std::net` is partial).
- **No self-update.** Install a newer VPK over the old one; your game files and saves stay.
