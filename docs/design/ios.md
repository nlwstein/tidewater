# Tidewater on iOS

**Bottom line.** Tidewater builds as an iPhone and iPad app from the same shell code as the desktop program. It runs in
the Simulator with touch, the safe area, background pause and a first-run import from the app's shared folder, and on
an iPad mini (6th generation) with real fingers. Signing is yours to add.

The gestures are in [touch.md](touch.md) and the import in [import.md](import.md); the Android version is [android.md](android.md).

## What you get

| Area | State |
|---|---|
| Build for the Simulator and a device | Works. Simulator checked; a signed Release build runs on an iPad mini (6th generation). |
| Touch, chrome buttons, safe area | On from the first frame; the picture fits inside the safe area. One-finger pan, hold-then-drag box, smart tap and the control-group bar ([touch.md](touch.md)); checked with `--touch-emulation`, the Simulator build and on the iPad. |
| Widescreen, soft keyboard | On the iPad: a wider game screen hides the side bars; the keyboard opens for chat (a tap on the status bar) and the save name, keeps the box above it, and closes with a tap elsewhere ([touch.md](touch.md), "Text boxes"). |
| Touch Controls help | Shows by itself at the first game, then under Help and on the main menu. |
| Exit Program | Not offered (greyed out on the main menu, not in End Scenario, Ctrl+X does nothing): iOS closes apps itself. |
| Background and foreground | The game pauses, sound stops, settings are written. |
| Import from the shared folder | Works (folder scan, game folders, CD images). |
| Import with the document picker | Written, **not tried**: the Simulator run could not tap. |
| GOG installer (`.exe`) | Not possible on iOS (see [Limits](#limits)). |
| Signing, TestFlight | Yours: you fill in a team ID; nothing is uploaded from here. |

## How it is put together

One loop serves both platforms. The desktop's `tidewater` binary and the iOS static library compile the same two files,
`crates/client/src/shell/app.rs` (the loop) and `sdl.rs` (the safe SDL wrapper, the only `unsafe` code).

| Piece | Where | Job |
|---|---|---|
| `tidewater-ios` | `crates/ios` | A `staticlib` that includes `app.rs` and `sdl.rs` with `#[path]`. Empty on any other target. |
| Entry point | `platform/ios/Sources/main.c`, `tidewater_ios_main` in `sdl.rs` | `main` calls `SDL_RunApp`, which starts UIKit and then calls the Rust program once the app has launched. |
| Document picker | `platform/ios/Sources/picker.m` | `UIDocumentPickerViewController` for a folder or a file, opened in place. |
| Project | `platform/ios/project.yml` | xcodegen input: frameworks, `Info.plist`, orientation, file sharing. |
| App icon | `platform/ios/Sources/Assets.xcassets` | One 1024 × 1024 `AppIcon` (no alpha) that Xcode scales for every size; drawn by `packaging/icon/render.sh` from `packaging/icon/tidewater.svg` (see [`docs/release.md`](../release.md#the-app-icon)). |
| Script | `platform/ios/build.sh` | Builds the library, then runs xcodegen (and optionally the Simulator app). |

**Why not SDL's main callbacks?** The loop already blocks inside `SDL_RunApp`'s callback, which SDL supports on iOS. A
callback split would change the desktop loop for no gain, so desktop and phone share the one loop instead.

## Build

You need Xcode, [xcodegen](https://github.com/yonaskolb/XcodeGen), CMake and rustup (the iOS targets are added by the
script).

```sh
platform/ios/build.sh sim --app    # Simulator library, Xcode project and Tidewater.app
platform/ios/build.sh device       # the device library and the project
```

The library goes to `platform/ios/build/iphonesimulator/` or `build/iphoneos/`, which the project links with
`-force_load`. The script sets `IPHONEOS_DEPLOYMENT_TARGET=15.0` so SDL's C code and Rust agree with `project.yml`. If you
change that value, clean SDL's build (`cargo clean --release --target aarch64-apple-ios-sim -p sdl3-sys`), because
Cargo does not rebuild it for a changed environment variable.

Run in the Simulator:

```sh
xcrun simctl install booted platform/ios/DerivedData/Build/Products/Debug-iphonesimulator/Tidewater.app
xcrun simctl launch booted dev.tidewater.client
```

Command-line arguments after the bundle identifier reach the program, so `... dev.tidewater.client "Maps/Cramped BNE.pud"`
starts a game and `--goto menu` opens the menu.

## Install the unsigned IPA

Every [release](../../../../releases/latest) has `tidewater-<version>-ios.ipa`, and every CI run uploads the same kind of
file as the `tidewater-ios-unsigned` artifact. It is a Release build for a device (arm64, iOS 15 or newer) with **no
signature**, so iOS will not run it until you re-sign it with your own Apple ID:

1. Download the `.ipa` (and check it against `SHA256SUMS`).
2. Install it with [AltStore](https://altstore.io), [SideStore](https://sidestore.io) or
   [Sideloadly](https://sideloadly.io). They sign it with your Apple ID and put it on the device.
3. Open Tidewater, then [get your game files onto it](#import-on-ios).

A free Apple ID's signature lasts **7 days**: AltStore and SideStore refresh it in the background while they can reach
your computer or a pairing file, and Sideloadly needs you to install again. A paid developer account lasts a year.
The public build keeps the bundle identifier `dev.tidewater.client`; the sideloading tools change it to one of yours
when they sign. Updating means installing the newer `.ipa` over the old one (game files and saves stay).

To make the same file yourself, run `platform/ios/build.sh device`, then `platform/ios/package-ipa.sh [--version X.Y.Z]
[--out FILE]`; to sign and run from Xcode instead, see Signing below.

## Signing

The project builds for the Simulator with no signing. For a device:

1. Copy `platform/ios/Config/Signing.example.xcconfig` to `Signing.xcconfig` in the same folder (git ignores it).
2. Set `DEVELOPMENT_TEAM` to your team ID, and `PRODUCT_BUNDLE_IDENTIFIER` if the default is taken.
3. Run `platform/ios/build.sh device`, open `Tidewater.xcodeproj`, choose your device and run.

Archiving and uploading to TestFlight are left to you in Xcode. `ITSAppUsesNonExemptEncryption` is already `false`.

## Import on iOS

The app's `Documents` folder is shared (`UIFileSharingEnabled` and `LSSupportsOpeningDocumentsInPlace`), so a computer's
Finder and the Files app (On My iPhone, Tidewater) can put files there. With no game files, the first-run screen
(`Intake::SharedFolder` in `firstrun.rs`) does this:

1. Scans `Documents`, its folders and its files (`search::shared_folder_candidates`), and checks each with
   `tidewater_import`'s classify and validate, so a game folder, a folder with the game folder inside it, or a CD image
   (`.iso`, `.bin`, `.cue`, `.mdf`, `.nrg`, read in pure Rust) all show up.
2. Lists what it found. Tapping one copies it into the app's data folder (`Library/Application Support/Tidewater`),
   with the usual progress bar. You can delete your copy afterwards.
3. Offers **Choose game folder...** and **Choose CD image...** (the document picker), and **Scan again** for files you
   added while the app was open. There is no Quit: an iOS app does not quit itself.

The screen's text says that a GOG installer does not work on iOS. If you pick one anyway, the message tells you to
unpack it on a computer, or to install the game and copy the folder.

**Picker.** SDL 3 has no file dialogs on iOS (its dialog code is a stub there), so `picker.m` shows a
`UIDocumentPickerViewController` instead and the shell answers the screen as it does for SDL's dialogs. It opens the
item in place and keeps the security-scoped access until the app ends, because the copy takes several seconds. A single
file is read alone: a `.cue` sheet picked this way cannot see its `.bin` next to it, so put multi-file images in the
shared folder.

## Lifecycle

This follows [client-core.md](client-core.md), "Lifecycle".

| Event (SDL) | Core does |
|---|---|
| will-enter-background, terminating | `Core::suspend`: pauses a running game as the pause key does (unless it is already paused), stops the mixer, writes the settings. |
| did-enter-foreground | `Core::resume`: restarts the mixer, lets go of held keys. The game stays paused until you resume it. |

An SDL event watch notes these as they happen, because the queue can be too late: the system may stop the program as
soon as its callback returns. The loop then runs one more frame so the pause takes effect, draws nothing, and sleeps
until the program is back. In a network game the pause goes to the other players like any other pause.

## Safe area and the window

The window is full screen (`SDL_WINDOW_FULLSCREEN`), landscape only, with an empty launch screen and a hidden status
bar. The picture and the chrome buttons fit inside `SDL_GetWindowSafeArea`, using the sharp-bilinear fit that touch
platforms always use. Touch is active from the first frame.

## Limits

- **No GOG installer.** `innoextract` is a separate program and iOS apps cannot start programs.
- **The document picker is untried.** The Simulator run had no way to tap (no UI automation tool was available).
- **Storage.** Imported files are about 500 MB; the app needs that much free storage and the original is not removed.
- **Network play** is untried on the phone.
