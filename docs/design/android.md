# Tidewater on Android

**Bottom line.** Tidewater builds as an Android app from the same shell code as the desktop and iOS programs. It runs
in the emulator with touch, landscape only, background pause, and a first-run import both from the app's folder and
from the system file picker. It is not yet tried on a phone.

The gestures are in [touch.md](touch.md), the import in [import.md](import.md) and the iPhone version in [ios.md](ios.md).

## What you get

| Area | State |
|---|---|
| Build (arm64-v8a, x86_64), debug APK | Works. |
| Touch, chrome buttons | On from the first frame; checked with taps in the emulator. One-finger pan, hold-then-drag box, smart tap and the control-group bar ([touch.md](touch.md)) are new and not yet run in the emulator. |
| Touch Controls help | Shows by itself at the first game, then under Help and on the main menu. |
| Exit Program, Back | Exit Program is not offered (Android closes apps itself); the Back button is Esc (`SDL_HINT_ANDROID_TRAP_BACK_BUTTON`), not yet tried in the emulator. |
| Landscape only | Works (the manifest and an SDL hint). |
| Background and foreground | The game pauses, sound stops, settings are written. Checked with Home and a relaunch. |
| Import from the app's folder | Works (folder scan, game folders). CD images use the same code as iOS; not run on Android. |
| Import with the system picker | Works for a folder (checked in the emulator). The file picker (CD image) is the same path; not run. |
| GOG installer (`.exe`) | Not possible on Android (see [Limits](#limits)). |
| Release APK | Built by the release workflow (`build.sh --release`), signed with a release keystore from repository secrets; see [release.md](../release.md#android-apk). The debug APK uses Gradle's debug key. |
| Phone, Play Store | Not tried. |

## How it is put together

One loop serves every platform. The desktop `tidewater` binary, the iOS library and the Android library compile the same
two files, `crates/client/src/shell/app.rs` (the loop) and `sdl.rs` (the safe SDL wrapper, the only `unsafe` code).

| Piece | Where | Job |
|---|---|---|
| `tidewater-android` | `crates/android` | A `cdylib` named `libmain.so` that includes `app.rs` and `sdl.rs` with `#[path]`. Empty on any other target, so `cargo test --workspace` needs no NDK. |
| JNI exports | `crates/android/build.rs` | SDL is linked in statically, so this exports its `Java_org_libsdl_app_*` functions and `JNI_OnLoad`, which Rust's linker settings would hide. |
| Entry point | `SDL_main` in `sdl.rs` | SDL's Java activity loads `libmain.so`, starts a thread and calls `SDL_main` with the command line. |
| Activity | `platform/android/app/src/main/java/dev/tidewater/client/TidewaterActivity.java` | SDL's activity that loads only `libmain.so`, passes the command line and shows the picker. |
| SDL's Java sources | `platform/android/app/src/main/java/org/libsdl/app/` | Copied by `build.sh` from the `sdl3-src` crate, so Java and C always match. Not in git (SDL is zlib-licensed). |
| Project | `platform/android/` | Gradle project in the layout of SDL's `android-project`; the Gradle wrapper (Apache 2.0) comes from that template. |
| Launcher icon | `platform/android/app/src/main/res/mipmap-*` | Android 8 and later: an adaptive icon (`mipmap-anydpi-v26/ic_launcher.xml`) whose background layer is the whole picture, zoomed out so the launcher's mask keeps it; Android 7: `ic_launcher.png`. Drawn by `packaging/icon/render.sh` (see [`docs/release.md`](../release.md#the-app-icon)). |
| Script | `platform/android/build.sh` | Builds the libraries with `cargo-ndk`, copies the Java sources and runs Gradle. |

## Build

You need rustup, [cargo-ndk](https://github.com/bbqsrc/cargo-ndk), CMake, a JDK (17 to 24, for Gradle 8.14) and the
Android SDK with an NDK (28.2 was used) and `platforms;android-35`. The script finds the SDK in `ANDROID_HOME`,
`ANDROID_SDK_ROOT`, `~/Library/Android/sdk` or `~/Android/Sdk`, the NDK in `ANDROID_NDK_HOME` or the newest one in the
SDK, and the JDK in `JAVA_HOME` or Homebrew.

```sh
platform/android/build.sh                  # both ABIs and the APK
platform/android/build.sh --abi x86_64     # one ABI
platform/android/build.sh --libs-only      # stop after libmain.so
```

The APK is `platform/android/app/build/outputs/apk/debug/app-debug.apk`. The script also sets `CMAKE_TOOLCHAIN_FILE`
to the NDK's toolchain file: without it `sdl3-sys` runs CMake with a processor name CMake's Android support rejects.
The minimum Android is 7.0 (API 24, `minSdk` in `app/build.gradle` and `-P 24` in the script).

## Run in the emulator

Create a virtual device with an `arm64-v8a` image on an Apple silicon Mac (`x86_64` on an Intel machine), then:

```sh
emulator -avd NAME -no-window            # or without -no-window for a window
adb install -r platform/android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.tidewater.client/.TidewaterActivity
adb exec-out screencap -p > shot.png     # screenshots; adb shell input tap X Y to touch
```

The activity turns the intent's `args` string array into the program's command line, so `--esa args Maps/Classic/Rivers.pud`
starts a game and `--esa args --goto,menu` opens the menu. A comma separates arguments and spaces cannot be escaped
through `adb shell`, so use a path without spaces.

## Import on Android

With no game files, the first-run screen (`Intake::SharedFolder` in `firstrun.rs`, shared with iOS) does this:

1. Scans the app's **external files folder**, `/sdcard/Android/data/dev.tidewater.client/files`, its folders and its
   files, and checks each with `tidewater_import`'s classify and validate. The shell tells the screen the folder in
   `TIDEWATER_SHARED_DIR`. You can fill it with `adb push` or, on phones that allow it, a computer's file transfer. On
   Android 11 and later file-manager apps cannot open `Android/data`.
2. Lists what it found. Tapping one copies it into the app's private data folder with the usual progress bar.
3. Offers **Choose game folder...** and **Choose CD image...**, and **Scan again**. There is no Quit.

**Picker.** SDL 3's own file dialog on Android has no folder choice and answers with `content:` addresses that `std::fs`
cannot read, so the shell asks the activity instead (a message from `SDL_SendAndroidMessage`). The activity shows the
Storage Access Framework picker (`ACTION_OPEN_DOCUMENT_TREE` or `ACTION_OPEN_DOCUMENT`), copies what you chose into the
app's cache folder (`cache/picked`), writes the copy's path to `files/pick-result.txt` and calls the native
`nativePicked`, which hands the path to the first-run screen as SDL's dialogs do. The activity shows a toast while it
copies. This needs no storage permission, because the picker grants access to the chosen item only.

Copying through the picker is slow (the full game is 700 MB and took about a minute in the emulator), and it makes a
second copy until the next start of the app, which deletes `cache/picked`. A single picked file is read alone: a `.cue`
sheet picked this way cannot see its `.bin`, so put multi-file images in the external files folder.

## Lifecycle

This follows [client-core.md](client-core.md), "Lifecycle", exactly as [ios.md](ios.md) describes. SDL raises
will-enter-background and did-enter-foreground from the activity's `onPause` and `onResume`, and the shell's event watch
calls `Core::suspend` and `Core::resume`. The game was greyed and paused after Home and a relaunch.

## Window and screen

The window is full screen and the picture fits inside the safe area with the fractional fit that touch platforms use. The
manifest asks for `sensorLandscape`, but SDL's window is resizable and so would follow the sensor into portrait; the
shell sets `SDL_HINT_ORIENTATIONS` to `LandscapeLeft LandscapeRight` on phones. The system's file picker is shown in
whatever orientation it chooses, and the game returns to landscape afterwards.

## Limits

- **No GOG installer.** `innoextract` is a separate program and an Android app cannot start one.
- **Not tried on a phone.** Only the arm64 emulator ran; a real device, other screen shapes (notches), the soft keyboard
  and a controller are untried.
- **The CD-image picker is untried** on Android (the folder picker was run); the import of CD images is the pure-Rust
  reader that works elsewhere.
- **Storage.** Imported files are about 700 MB and the original stays; the picker needs the same again in cache.
- **Network play** is untried on the phone.
- **No Play Store packaging.** The release APK is for sideloading; when a newer release exists the main menu offers it, and **Update now** opens the release page to download the new APK (`update.rs`). The manifest asks only for `INTERNET`, for network games and that check.
- **Sound** is untested in the emulator (it ran with `-no-audio`).
