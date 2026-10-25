# Tidewater: agent instructions

Rust reimplementation (an engine-neutral core in an SDL 3 shell) of the engine behind *Warcraft II: Battle.net Edition* (with *Beyond the Dark Portal*),
running on the user's own game data. This repository is written to be published: read [CLEANROOM.md](CLEANROOM.md)
before writing any code, and follow it exactly.

## Hard rules

- **This repo is the implementation side.** Never put disassembly, decompiler output, code derived from the original
  program, or any game file or extract into it, and never work from them here. Allowed sources are listed in
  CLEANROOM.md. If a task needs knowledge of the original program's behaviour, request a spec; don't go and look.
- **Nothing personal or private:** no host names, IP addresses, user names, personal paths or secrets in files or
  commit messages. The hooks (`git config core.hooksPath .githooks`) enforce this; never bypass them with
  `--no-verify`. History is never rewritten, so a leak must be stopped before the commit exists.
- **Tests use synthetic data** built by our own code. Checks against a real install go through `twtool` with a path
  given at run time, never a fixture copied into the repo.

## Layout

- `crates/formats`: readers for the installation's data (MPQ archives, PUD maps, tilesets, GRP sprites (compressed
  and uncompressed), PCX images, raw interface pictures and cursors (`raw`), fonts (`fnt`), Smacker video (`smk`), string tables, unit and
  upgrade tables, map restrictions).
- `crates/twtool`: inspection CLI (`import-check`, `disc-list`, `mpq-*`, `render-map`, `render-grp`, `render-pcx`, `smk-frame`, `smk-audio`, `grp-sheet`, `units`, `upgrades`,
  `strings`, `scripts`, `script-ops`, `fell`, `sim`, `oracle`: compare the simulation with a per-cycle trace of the original game, `snapshot`: the
  whole-map behaviour snapshot `tests/snapshot.rs` checks, `replay`: verify a `.twrep`, `desync-diff`: find the first differing cycle and field, `app-icon`: draw the app icon
  for `packaging/icon/render.sh`, which redraws every platform's icon from `packaging/icon/tidewater.svg`).
- `crates/import`: game-file import (`source`: what a path is, `validate`: key-file checks, `copy`: allowlisted copy
  and atomic swap into `<data>/game`, `manifest`); used by `tidewater --import` and `twtool import-check`;
  `docs/design/import.md`.
- `crates/replay`: the `.twrep` replay format (`format`), rebuilding a game from a header (`start`) and running or comparing replays (`run`); `docs/design/multiplayer-determinism.md`.
- `crates/sim`: the deterministic game simulation (no engine types; placeholders until specs land, see `Rules`).
- `crates/client`: the game client: a library (`lib.rs`, no engine types, no `unsafe`) holding the core (`core.rs`:
  `Core::advance` runs a frame; `Core::suspend`/`resume` are the phone's background and foreground) and `host.rs` (start-up from the command line, the headless scripted loop, helpers
  for the shell), and the `tidewater` binary, the SDL 3 shell (`shell/app.rs`: window, input events, mouse lock,
  sound device, `shell/main.rs` its `main`; `shell/sdl.rs` is our thin safe wrapper over `sdl3-sys` and the only `unsafe` code). SDL is built
  from source and linked in, so every build needs CMake and a C compiler, not an SDL install.
  `game.rs` runs the game screen, `args.rs` holds the command line. `scene.rs` builds the map and unit images, `hud.rs`
  lays out and draws the interface (frame, minimap, resources, selection, command buttons) into one picture, `panel.rs` holds the command panel's button sets and rules, `info.rs` the unit information panel's rules.
  `campaign.rs` holds the campaign flow's rules, `video.rs` the Smacker playback helpers and `display.rs` the window
  scale, black border, map zoom and mouse-lock rules;
  `frontend.rs` runs the screens around a game (videos, title, menu, act screens, briefings, the outcome dialogs,
  the score screen, endings, credits) as the flow's states (`states.rs`), and the game screen's systems run only in `AppState::Game`.
  `customsetup.rs` is the Custom Scenario screen (options, the Select scenario dialog, the map list from the `Maps` folder, the `Setup::Custom` it starts).
  `menuscreen.rs` holds the menu screens' rules (the shared menu picture, which switches fade, button and text colours).
  `--goto SCREEN` opens a flow screen directly for development.
  `audio.rs` decodes WAV sounds and mixes them at 22,050 Hz stereo; the shell streams the mixer to the device.
  `firstrun.rs` is the first-run import screen shown when there are no game files (`AppState::FirstRun`; its own font
  in `firstrun/font.rs`, the automatic search in `firstrun/search.rs`; `docs/design/import.md`).
  Touch (`docs/design/touch.md`): `gesture.rs` is the state machine turning fingers into mouse frames, pan and zoom,
  `chrome.rs` the on-screen buttons in the pillarbox (menu, pause, Shift/Ctrl/Alt latches, plain click, keyboard, control groups),
  `touchemu.rs` the mouse as fingers (`--touch-emulation`), `touchhelp.rs` the Touch Controls help page and `touch.rs` the core's glue (`Core::touch`).
  `session.rs` is the `Session` trait between the game screen and the sim (`LocalSession`, `ReplaySession`); `net/` is network play (`proto`: messages and identity, `tcp`, `lockstep`: `NetSession`, `lobby`: the command-line host and join, `room`: the menu lobby's host and guest ends with map transfer, `beacon`: the LAN game-list UDP packet; `docs/design/network.md`).
  `mpmenu.rs` is the multiplayer menus (Multi Player Game to the game list and Direct IP), `mpwidgets.rs` their edit boxes and pop-ups, `lobbyscreen.rs` the game setup lobby; `chat.rs` is the in-game chat line, sent as `Msg::Chat` outside the turns.
  `extras.rs` is the optional improvements' registry, settings and flags (`docs/design/extras.md`); `settings.rs` is the Settings
  screen (SDL-free: its rules and drawing, generated from the registry; opened by F2 or the link on the main menu, `AppState::Settings`, `--goto settings`; in a game by Ctrl+G or the Options dialog's link, as the game menu's `Screen::Settings`); `update.rs` is the desktop self-update
  (the release check, "Update now"; `docs/release.md`).
  The game screen's other parts: `input.rs` (a frame's keys, mouse and touches), `clock.rs` (real and game time, the fixed step), `layers.rs` (the screen's layers as pictures), `mapview.rs` (the map view's geometry and zoom), `view.rs` (drawing the map view), `fog.rs`, `minimap.rs`, `screen.rs` (resource bar, scrolling, pointers), `mouse.rs` (selecting and commanding, the key table), `messages.rs` (message list, notices, status bar), `carry.rs` (workers and tankers carrying goods), `palette.rs` (colour cycling and fades), `screenshot.rs` (Print Screen), `cheats.rs` and `diplomacy.rs` (the chat-line cheat codes and `/diplomacy`).
  Sound and music: `sound.rs` (which file plays for each cue) and `music.rs` (which track plays when), with `jukebox.rs` playing it in the mixer.
  The game menu: `gamemenu.rs` draws it and the dialogs under it, `menu.rs` holds the rules, `prefs.rs` the stored settings behind its dialogs; saved games are `savegame.rs` (collect a game and start from one), `savefile.rs` (the file layout), `saves.rs` (folder and dialog rules) and `savedlg.rs` (the dialogs).
  Optional improvements' rules sit in their own modules: `overlay.rs` (health bars, clock), `idle.rs`, `groupbar.rs`, `attackmove.rs`, `ping.rs`; `keymap.rs` holds the rebindable keys.
  `storage.rs` is the `Storage` trait for everything read or written outside the core (installation, saves,
  settings, screenshots, wall clock) and the desktop's folders.
  The window shows one picture composed on the CPU, 640 × 480 or wider in a game with the `widescreen` option (`compose.rs`, `widescreen.rs`; `core.rs` places the layers, the shell
  shows it). `script.rs` runs scripted scenes (`--script NAME --script-out DIR [--headless]`) for the frame
  snapshot `tests/frames.rs`; the design of the core and its shell is `docs/design/client-core.md`.
- `crates/ios`: the iOS app's static library (`tidewater-ios`): empty except on `target_os = "ios"`, where it builds
  the shell files `crates/client/src/shell/app.rs` and `sdl.rs` by `#[path]`, so desktop and phone run one code path.
  The iOS entry (`tidewater_ios_main`), the document-picker call and the lifecycle event watch are in `sdl.rs`.
- `platform/ios`: the Xcode project around it: `project.yml` (xcodegen), `Sources/main.c` (hands over to `SDL_RunApp`),
  `Sources/picker.m` (UIDocumentPickerViewController), `Config/*.xcconfig` (signing is the user's own), `build.sh`, `package-ipa.sh` (the unsigned `.ipa`);
  `docs/design/ios.md`.
- `crates/android`: the Android app's native library `libmain.so` (`tidewater-android`, a `cdylib`): empty except on
  `target_os = "android"`, where it builds the same two shell files by `#[path]`; `build.rs` keeps SDL's JNI entry points
  exported. The entry (`SDL_main`), the picker callback and the environment set-up are in `sdl.rs`.
- `platform/android`: the Gradle project around it (SDL's `android-project` layout): `TidewaterActivity.java` (arguments
  and the Storage Access Framework picker), the manifest, the Gradle wrapper, `build.sh` (cargo-ndk, copies SDL's Java
  sources from the `sdl3-src` crate at build time, runs Gradle); `docs/design/android.md`.
- `docs/specs/`: reviewed behaviour specifications, the only source for engine behaviour.

## Workflow

- The toolchain is pinned in `rust-toolchain.toml`; dependencies track their newest releases.
- Building needs CMake and a C compiler besides Rust (SDL 3 is built from source). GitHub Actions
  (`.github/workflows/ci.yml`) runs fmt, clippy and the tests without game data, plus release builds on Linux, macOS
  and Windows, the iOS Simulator and Android libraries and an unsigned iOS `.ipa`; the full check with game data and oracle traces is still
  run locally. `.github/workflows/release.yml` builds the downloads (`docs/release.md`).
- The multiplayer end-to-end test (`crates/client/tests/mp_e2e.rs`) needs `TIDEWATER_INSTALL` and port 6112; see `docs/design/network.md`, "Running the end-to-end check".
- Before every commit run `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
- Conventional commits (`feat:`, `fix:`, `docs:`, ...): they decide the release version. Work lands on `develop`; merging
  it into `main` cuts a release (`docs/release.md`).
- Cite the public source for every format detail in the module doc of the code that uses it.
- Rules modes: Enhanced is the default and the target; Classic is opt-in and multiplayer only. Say so in any doc that mentions rules.
