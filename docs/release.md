# Releasing

**In short:** work lands on `develop`. To release, merge `develop` into `main`. The push to `main` works out the version
from the commit messages, builds the Windows, macOS, Linux, iOS, Android and PS Vita downloads, and publishes a GitHub release. Nothing else to do.

## Branches

| Branch | Job |
|---|---|
| `develop` | The default branch. All work lands here (pull requests target it). CI runs on every push. |
| `main` | The release branch. It only moves by merging `develop` in; each push to it can cut a release. |

Use [conventional commits](https://www.conventionalcommits.org) on `develop`: they decide the version.

## How a release is cut

1. Make sure `develop` is green in CI.
2. Merge `develop` into `main` and push `main`.
3. `.github/workflows/release.yml` runs. Check the release page when it finishes.

**The version** comes from the commits since the last `v*` tag (`.github/scripts/release.py`, with tests beside it):

| Commit since the last tag | Bump |
|---|---|
| `type!:` or a `BREAKING CHANGE:` footer | major (minor while the version is below 1.0) |
| `feat:` | minor |
| `fix:` or `perf:` | patch |
| anything else (`docs:`, `chore:`, `ci:`, `test:`, `refactor:`) | no release on its own |

The biggest bump wins. If nothing releasable is there, the workflow ends after its first job and publishes nothing. With no
tag yet, the first release is `v0.1.0`.

**The version lives only in the tag.** No commit bumps `Cargo.toml`, so `main` never differs from `develop`. The workflow
passes the number to the build as `TIDEWATER_VERSION` (and `TIDEWATER_REPO`, from `github.repository`); the program reads it
with `option_env!`. The main menu's version line and `tidewater --version` show it, or `dev` for a build without it. The
version in `Cargo.toml` is not used.

**What the workflow does, in order:**

1. `plan`: runs the script's tests, computes the version and the notes.
2. `linux`, `macos`, `windows`, `ios`, `android`, `vita`: build the downloads in parallel.
3. `windows-smoke`: installs the setup program silently on a clean Windows runner, runs `tidewater --version`, uninstalls.
4. `publish` (only when a release is due, never on a dry run): writes `SHA256SUMS`, then `gh release create` makes the tag
   `vX.Y.Z` on the built commit and the release, with every file and notes grouped by type (breaking changes, features,
   fixes, performance). The tag is made here because a tag made with the workflow's token would not start another workflow.

## Dry run

**Actions, Release, Run workflow, `dry_run` (on by default)** builds and smoke-tests everything and publishes nothing: no
tag, no release. Run it from `develop` or `main`. If nothing is releasable it builds `0.0.0-dryrun`, a version that never
offers updates. The run's summary shows the version and notes a real release would get. A run with `dry_run` off only
publishes from `main`.

## What gets published

| File | Made by |
|---|---|
| `tidewater-X.Y.Z-windows-x64-setup.exe` | Inno Setup, `packaging/windows/tidewater.iss`. Per-user install (no administrator), Start menu entry, optional desktop shortcut, uninstaller. The uninstaller removes only the program; imported game files, saves and settings live in the user's data folders and stay. |
| `tidewater-X.Y.Z-macos.dmg` | `packaging/macos/build-dmg.sh`: `Tidewater.app` (ad-hoc signed), an Applications link and a "Read me first" note. The binary joins the arm64 and x86_64 builds with `lipo`. |
| `tidewater-X.Y.Z-linux-x64.tar.gz` | `packaging/linux/build-tarball.sh`: the program, `tidewater.desktop` and its icon `tidewater.png`, `README.txt`, the licence files. No AppImage yet. |
| `tidewater-X.Y.Z-android.apk` | `platform/android/build.sh --release`: one APK with arm64-v8a and x86_64 libraries, signed with the release keystore (see [Android APK](#android-apk)). Left out, with a note in the run summary, while the signing secrets are not set. |
| `tidewater-X.Y.Z-ios.ipa` | `platform/ios/package-ipa.sh --version X.Y.Z` after `platform/ios/build.sh device`: the Release device build (arm64), **unsigned**, with `Payload/Tidewater.app` at the root. The version goes into `CFBundleShortVersionString` and `CFBundleVersion`. The bundle id stays `dev.tidewater.client`; whoever sideloads it re-signs with their own Apple ID ([docs/design/ios.md](design/ios.md#install-the-unsigned-ipa)). CI builds the same file on every run as the `tidewater-ios-unsigned` artifact. |
| `tidewater-X.Y.Z-vita.vpk` | `platform/vita/build.sh --version X.Y.Z`: the PS Vita app with a pinned nightly and the Vita SDK ([docs/design/vita.md](design/vita.md)). The version shows as `XX.YY` (major and minor). CI builds the same file on every run as the `tidewater-vita` artifact. |
| `SHA256SUMS` | `sha256sum` of every file above (`publish` sums `tidewater-*`, which covers the APK and the IPA). |

The asset names are what the update check looks for (`update.rs`), so keep them. The check picks the Windows and macOS files by their endings (`-windows-x64-setup.exe`, `-macos.dmg`); the phones only open the release page, so the `-ios.ipa` name is never matched. Only the program is packaged, never game
data.

To try the packaging on your own machine: `packaging/macos/build-dmg.sh 0.0.0-test target/release/tidewater out.dmg`, or
`packaging/linux/build-tarball.sh 0.0.0-test target/release/tidewater out.tar.gz`.

### The app icon

One drawing, `packaging/icon/tidewater.svg` (our own artwork, MPL-2.0: a square-sailed ship riding the waves at dusk),
is the icon everywhere. `packaging/icon/render.sh` redraws every platform's file from it with `twtool app-icon`; the
results are committed, so the release builds don't draw anything. To change the icon, edit the SVG, run the script and
commit what it writes.

| Platform | File | Used by |
|---|---|---|
| iOS | `platform/ios/Sources/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png` | `project.yml` (`ASSETCATALOG_COMPILER_APPICON_NAME`). |
| Android | `platform/android/app/src/main/res/mipmap-*/` | The manifest's `android:icon`. |
| macOS | `packaging/macos/tidewater.icns` | `build-dmg.sh` puts it in `Tidewater.app/Contents/Resources`; `Info.plist` names it. |
| Vita | `platform/vita/sce_sys/icon0.png` | 8-bit indexed (`--indexed`), as the Vita's installer requires; `build.sh` packs it into the VPK. |
| Windows | `packaging/windows/tidewater.ico` | `tidewater.iss`: the installer's own icon, the shortcuts and the Apps list. `tidewater.exe` has no icon resource yet, so Explorer shows a plain program icon for the file itself. |
| Linux | `packaging/linux/tidewater.png` | The tarball, for `tidewater.desktop`'s `Icon=` line. |
| The window | `crates/client/assets/icon/window.png` | The desktop shell sets it with `SDL_SetWindowIcon` (title bar, taskbar, Dock). |

## Unsigned: what players see

Nothing is signed with a purchased certificate, so each system warns once. The README's Install section tells players
what to do.

| System | What happens | Way through |
|---|---|---|
| **macOS** | Gatekeeper refuses the first launch. The ad-hoc signature only lets Apple-silicon Macs run the code. | Right-click, Open, Open. On newer macOS: System Settings, Privacy & Security, Open Anyway. Or `xattr -dr com.apple.quarantine Tidewater.app`. |
| **Windows** | SmartScreen says "Windows protected your PC" for the setup program. | More info, Run anyway. |
| **Linux** | None. | `chmod +x` if the file manager dropped the executable bit. |
| **iOS** | The file is not signed, so iOS refuses to install it as it is. | Sideload with AltStore, SideStore or Sideloadly, which sign it with your own Apple ID. |
| **Android** | The system asks to allow installs from the app that opened the APK (the browser or file manager). | Allow it for that app, then install. Not on Google Play. |
| **PS Vita** | Stock firmware refuses an app Sony did not sign. | Install the VPK with VitaShell on a Vita with custom firmware ([vita.md](design/vita.md#install)). |

## Android APK

The `android` job builds the APK with the pinned NDK (`platform/android/ndk-version`, installed with `sdkmanager`), `cargo-ndk` and JDK 21, then checks the signature with `apksigner`
and prints the certificate's SHA-256 in the run summary (it must be the same on every release).

**versionName and versionCode.** `versionName` is the release version. `versionCode` is `major * 10000 + minor * 100 +
patch` (1.2.3 gives 10203), so it rises with every release; Android refuses to install a lower one over a higher one. Keep
minor and patch under 100 (`build.sh` refuses a version that breaks that). A `0.0.0-dryrun` build gets 1.

**Signing.** A sideloaded app updates in place only if every release has the same signing key, so the key is one keystore
kept in repository secrets, never in the repository. `app/build.gradle` reads it from environment variables
(`TIDEWATER_KEYSTORE_FILE`, `TIDEWATER_KEYSTORE_PASSWORD`, `TIDEWATER_KEY_ALIAS`, `TIDEWATER_KEY_PASSWORD`) that the job
fills from the secrets.

| Run | Secrets set | Secrets missing |
|---|---|---|
| Release (push to `main`) | Signed APK is published. | The APK is skipped and the run summary says so. The release still goes out. |
| Dry run | Signed APK as a workflow artifact. | Debug-signed APK as a workflow artifact, to look at. |

A dry run never publishes anything.

### One-time setup

1. **Make the keystore** (once, on your own machine; the validity is about 27 years):

   ```sh
   keytool -genkeypair -v -keystore tidewater-release.keystore -alias tidewater \
     -keyalg RSA -keysize 4096 -validity 10000
   ```

2. **Back it up now,** with its passwords, somewhere other than your laptop (a password manager's file storage). **If the keystore is lost, nobody can update in place:** every player must uninstall the app (losing its imported game files and settings) and install the new one. Do not commit it.
3. **Set the four secrets** (the key password is the keystore password if `keytool` was not given a separate one):

   ```sh
   base64 < tidewater-release.keystore | tr -d '\n' | gh secret set ANDROID_KEYSTORE_BASE64
   gh secret set ANDROID_KEYSTORE_PASSWORD --body '<keystore password>'
   gh secret set ANDROID_KEY_ALIAS --body 'tidewater'
   gh secret set ANDROID_KEY_PASSWORD --body '<key password>'
   ```

4. Run the Release workflow as a dry run and read the run summary: it should say "release keystore" and show the certificate fingerprint. Note the fingerprint; later releases must show the same one.

To build a signed APK locally, export the four `TIDEWATER_*` variables and run `platform/android/build.sh --release
--version X.Y.Z --out tidewater.apk`. Without `TIDEWATER_KEYSTORE_FILE` it signs with Gradle's debug key and says so.

## Self-update

At start-up the program asks `https://api.github.com/repos/<TIDEWATER_REPO>/releases/latest` (no sign-in, one request). A
newer plain version (not a pre-release or draft) puts **Update now** and **Later** on the main menu. The download is the
asset's `browser_download_url`; GitHub answers with a redirect to its file storage, which the HTTP client follows. The
size must match the release's listed size; there is no other signature check.

| System | Update now |
|---|---|
| Windows | Downloads the setup program and runs it silently (`/SILENT`); it starts the game again. |
| macOS | Downloads the disk image, copies `Tidewater.app` beside the running one, swaps them, relaunches. Files fetched this way carry no quarantine mark, so the new copy opens without the Gatekeeper prompt. |
| Linux | Opens the release page. |
| Android | Opens the release page (the shell's `SDL_OpenURL`); players download the newer APK and install it over the old one, which keeps their data because the signature matches. The check runs as on the desktop. |
| iOS | Opens the release page; players sideload the newer IPA over the old one ([ios.md](design/ios.md#install-the-unsigned-ipa)). |
| PS Vita | No check (`update.rs` skips it); players install the newer VPK by hand. |

**macOS: App Translocation.** An app run from the disk image, or from Downloads while still quarantined, runs from a
read-only or randomised path (`/Volumes/...`, `.../AppTranslocation/...`) and can't replace itself. The main menu then says
"Move Tidewater to Applications to enable updates." once (remembered as `move_notice_seen` in the settings file), and
Update now opens the release page instead.

### Testing updates without a release

`TIDEWATER_UPDATE_FEED=<url or file>` replaces the GitHub answer with a file of the same shape (`tag_name`, `html_url`,
`assets[].name/size/browser_download_url`). With it set, the address checks on downloads are off, and the program says so on
stderr. It works in every build, but only a build that has a version does anything, so build with both variables set:

```sh
TIDEWATER_VERSION=0.1.0 TIDEWATER_REPO=test/tidewater cargo build --release -p tidewater-client   # the "old" app
TIDEWATER_VERSION=0.2.0 TIDEWATER_REPO=test/tidewater cargo build --release -p tidewater-client   # the "new" one
packaging/macos/build-dmg.sh 0.2.0 <new binary> feed/tidewater-0.2.0-macos.dmg
```

Put a `feed.json` in `feed/` with the new DMG's real byte size and `http://127.0.0.1:8765/...` download addresses, serve it
with `python3 -m http.server 8765 --directory feed`, and install the old app somewhere harmless (`~/Applications/TidewaterTest/`,
never `/Applications`). Run it with `TIDEWATER_UPDATE_FEED=http://127.0.0.1:8765/feed.json`, `--install <game folder>
--goto menu`, and a temporary `--save-dir` and `--settings`; then press **U** (Update now) on the main menu.

The relaunched copy starts with no arguments, so it uses the real per-user data folder: close it at once, and never test
against a folder you care about. `TIDEWATER_DEBUG=1` shows the check's steps.

## Where players start

The update check and the release downloads both need the repository to be public, because they call GitHub without
signing in. A private repository answers 404: no update is offered (silently; `TIDEWATER_DEBUG=1` shows it) and downloads
need sign-in. There is no landing page: the README and the GitHub releases page are where players start.
