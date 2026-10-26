# Tidewater

**Tidewater is a from-scratch Rust reimplementation of the *Warcraft II: Battle.net Edition* engine, including *Beyond the Dark Portal*, that runs on your own copy of the game.** You can play both campaigns in single player, start a custom game on any map, and play early LAN multiplayer. Tidewater contains no game data: it reads the files of your own copy.

**Status: early but playable.** Menus, briefings, score screens, endings, credits, saved games, sound, music and video all work. Enhanced rules (the original's default) are what Tidewater plays and tests against; Classic rules are an opt-in for multiplayer only. See [Features today](#features-today) and the [roadmap](#roadmap).

## What you need

- **Your own copy of the game.** Tidewater needs *Warcraft II: Battle.net Edition* (for example the GOG release, or the Battle.net Edition CD). No game data is included. The 1995 DOS edition is not supported.
- **A desktop computer.** macOS, Windows and Linux builds are made and checked in CI. Day-to-day play testing has been on macOS. iOS, Android and PS Vita apps exist as early builds (see [iPhone and iPad](#iphone-and-ipad-early), [Android](#android-early) and [PS Vita](#ps-vita-early)).
- **A download, or a build toolchain.** Prebuilt downloads are on the [releases page](../../releases/latest); otherwise [build from source](#build-from-source).

## Install

Download the file for your system from the [latest release](../../releases/latest). None of them contains game data. Each release also lists `SHA256SUMS` to check the downloads against.

| System | File | How |
|---|---|---|
| **Windows** (x64) | `tidewater-<version>-windows-x64-setup.exe` | Run it. It installs for your user only, so it needs no administrator, and adds a Start menu entry (and, if you tick it, a desktop shortcut) and an uninstaller. Uninstalling removes the program only: your imported game files and your saved games and settings (see [Where your files go](#import-your-game-files)) stay, so a reinstall picks up where you left off; delete those folders by hand if you want them gone. Windows may say "Windows protected your PC" because the installer is not signed: choose **More info**, then **Run anyway**. |
| **macOS** (Apple silicon and Intel) | `tidewater-<version>-macos.dmg` | Open it and **drag Tidewater to Applications**, then eject the disk image. The app is not signed, so the first time **right-click it, choose Open and confirm**. If macOS offers no Open button, open System Settings, Privacy & Security and click **Open Anyway**. After that it opens normally. |
| **Linux** (x64) | `tidewater-<version>-linux-x64.tar.gz` | Unpack it and run `tidewater`. The archive has a `tidewater.desktop` menu entry to adapt and a `README.txt`. |
| **Android** (7.0 or newer, early) | `tidewater-<version>-android.apk` | Download it on the device and open it; Android asks to allow installs from that browser or file manager, so allow it for that app. It is not on Google Play and does not update itself: to update, download the newer APK and install it over the old one (your game files and settings stay). Then [get your game files onto it](#android-early). A release may lack the APK until its signing key is set up. |
| **iPhone and iPad** (iOS 15 or newer, early) | `tidewater-<version>-ios.ipa` | Not signed and not on the App Store: sideload it with your own Apple ID using AltStore, SideStore or Sideloadly (a free Apple ID needs a re-sign every 7 days). Steps: [Install the unsigned IPA](docs/design/ios.md#install-the-unsigned-ipa). |
| **PS Vita** (custom firmware, early) | `tidewater-<version>-vita.vpk` | Install it with VitaShell, then copy your game files to `ux0:data/Tidewater`. Not tried on a real Vita; its buttons and sticks use the controller support, untested there. Steps: [PS Vita](#ps-vita-early). |

## Import your game files

The first time you start Tidewater with no game files, it opens an import screen. It lists the installs it found in the usual places (a GOG folder, `/Applications`, Wine and CrossOver prefixes). Pick one, choose a game folder with the buttons, or drop a folder on the window. Tidewater checks the files, copies the archives (`War2Dat.mpq`, `Install.mpq`, `War2Patch.mpq`) and the `Maps` folder into its own per-user data directory once, shows what it imported and goes on into the game. If something is wrong, the screen says what and what to do next. Esc quits.

**What you can import.** Choose any of these as `<path-to-game>`:

| Source | Notes |
|---|---|
| **A game folder** | A GOG folder, a Wine prefix, a `.app` bundle or a mounted CD all work. Tidewater looks a few levels down. |
| **The GOG installer** | `setup_warcraft_ii_*.exe`, with its `.bin` file in the same folder. It works directly, without installing the game: choose the `.exe` (or the folder holding it). It needs the free [`innoextract`](https://constexpr.org/innoextract/) program (`brew install innoextract` on macOS, your package manager on Linux, a download on Windows); Tidewater tells you if it can't find it. |
| **A CD image** | `.iso`, BIN/CUE (`.bin` with its `.cue`, choose either), `.img`, `.mdf` or `.nrg`, read directly with no mounting. This includes the Battle.net Edition CD, whose game is inside its `INSTALL.EXE`. A disc image gives the game as first sold, without the later patches. If a disc only holds some other installer, Tidewater says so: install the game from the disc and choose the installed folder. `twtool disc-list <image>` shows what is on an image. |
| **Archives** | A folder of `.mpq` files, or one `.mpq` file. |

**Where your files go.** Tidewater keeps everything in a per-user data directory and never writes into your game installation.

| System | Data directory |
|---|---|
| macOS | `~/Library/Application Support/Tidewater` |
| Windows | Game files: `%LOCALAPPDATA%\Tidewater\data`. Saved games and settings: `%APPDATA%\Tidewater\data` (it roams with your account; the game files are too big to). |
| Linux | `~/.local/share/tidewater` (or under `$XDG_DATA_HOME`) |

Imported files go in `game`, saved games in `Save`, settings in `settings.txt`. `--data-dir <dir>` moves all of it.

**Import again.** Run `tidewater --reimport` to open the import screen even though files are already there, for example to use another copy of the game. A second import replaces the first, and Esc leaves it as it was. Removing the data directory (both of them on Windows) removes everything Tidewater stored.

**Without a window.** `tidewater --import <path-to-game>` imports from the command line, prints a summary and exits. `twtool import-check <path-to-game>` runs the same checks and copies nothing. How the import works: [docs/design/import.md](docs/design/import.md).

## Play

Start Tidewater from the Start menu, Applications or `tidewater` on the command line. Once the files are imported it plays the intro videos and opens the title and main menu. Add a map to start a game on it at once: `tidewater <map.pud>`.

## Update

Tidewater looks for a newer release in the background when it starts (one request to GitHub, nothing sent about you). If there is one, the main menu shows **Update now** and **Later**.

| System | Update now |
|---|---|
| **Windows** | Downloads the installer, runs it silently and restarts the game. |
| **macOS** | Replaces the app and restarts it. This needs the app in a folder you can write to, such as Applications: run from the disk image or straight from Downloads, the main menu says "Move Tidewater to Applications to enable updates." once, and Update now opens the release page. |
| **Linux** | Opens the release page; download the new archive and unpack it over the old one. |
| **Android, iOS** | Opens the release page; download the new APK or IPA and install it over the old one (your game files and saves stay). |
| **PS Vita** | No check; install the newer VPK over the old one (your game files and saves stay). |

Downloads are checked against the size the release lists; they are not signed. A failed check is ignored without a message, and development builds never check. Turn the check off with `update_check=0` in `settings.txt` or `--no-update-check` for one run. The main menu's version line and `tidewater --version` show the release you have. Branches, versioning and cutting a release: [docs/release.md](docs/release.md).

## Build from source

1. **Install the build tools.**
   - The Rust toolchain pinned in [`rust-toolchain.toml`](rust-toolchain.toml) (`rustup` picks it up by itself).
   - CMake and a C compiler. SDL 3 is built from source and linked into the program, so you do not install SDL itself.
   - macOS: `xcode-select --install` and `brew install cmake`.
   - Debian or Ubuntu (the packages CI installs): `sudo apt-get install build-essential cmake pkg-config libx11-dev libxext-dev libxrandr-dev libxcursor-dev libxi-dev libxfixes-dev libxss-dev libxtst-dev libxkbcommon-dev libwayland-dev wayland-protocols libdecor-0-dev libasound2-dev libpulse-dev libudev-dev libdbus-1-dev libegl1-mesa-dev libgl1-mesa-dev`
   - Windows: Visual Studio's C++ build tools and CMake.
2. **Run it.** The first run opens the import screen (see [Import your game files](#import-your-game-files)):

   ```sh
   cargo run --release -p tidewater-client
   ```

   Pass options after `--`, for example `cargo run --release -p tidewater-client -- --import <path-to-game>`.

**Developer override.** `--install <path-to-game>` (or the `TIDEWATER_INSTALL` environment variable) reads the files in place and skips the import.

## Features today

| Area | What works |
|---|---|
| **Campaigns** | All 52 campaign missions: *Tides of Darkness* and *Beyond the Dark Portal*, orc and human. Briefings, act screens, victory and defeat dialogs, score screens, endings and credits. |
| **Game screen** | Map view with fog of war, minimap, resource bar, selection and unit information panel, command buttons and hotkeys, building placement, messages and the status bar. |
| **Game rules** | Movement and pathfinding, combat, spells, harvesting, construction, training and research, repair, transports, walls, rescue, victory and defeat, and computer players that build, train, attack and cast. |
| **Custom games** | The Custom Scenario screen (see below), or any `.pud` map from the command line, with the original custom-game choices through `--setup` (race, opponents, resources, terrain, starting units, fog). |
| **Settings** | Tidewater's own Settings screen (F2 on the main menu, Ctrl+G in a game): the optional improvements, the window, the mouse lock, the intro video, the update check and the game files. See [Settings](#settings). |
| **Menus** | Main menu (Single Player Game, Multi Player Game, Replay Introduction, Show Credits, Exit Program), single-player menu (New Campaign, Load Game, Custom Scenario), new-campaign choice, and the in-game menu (F10) with save, load, options, help, sound, speed and preferences dialogs, objectives, restart and surrender. |
| **Saved games** | Save and load from the in-game menu and the victory dialog, into the `Save` folder of the data directory. |
| **Sound, music, video** | Unit and event sounds, the music jukebox, the intro and story videos (Smacker), and palette effects such as colour cycling and fades. |
| **Multiplayer** | Early LAN and Direct IP games for two to eight players, with observers (see [Multiplayer](#multiplayer-early)). |
| **Cheats** | The original cheat codes, typed into the chat line. |
| **Display** | A 640 x 480 picture drawn in software, with a black border. By default it is shown at the largest whole-number scale that fits, or scaled to fill the screen with crisp pixels when a whole number would leave much of it empty (Settings, Scaling). `--ui-scale N` picks the window's size. |

### Playing a custom game

Choose **Single Player Game** on the main menu, then **Custom Scenario** (or start with `--goto custom`), to open the Custom Scenario screen. It offers the original's
choices: Your Race, Opponents, Resources, Map Tileset and Units, each a drop-down that you open with the pointer or step
with Tab, Enter and the arrow keys. **Select Scenario** (E) opens a dialog that lists the maps and narrows them by type,
number of players and map size; **Start Game** (S) plays the map you picked with Enhanced rules, and **Cancel Game** (C or
Esc) returns to the main menu. The single-player menu also holds New Campaign and Load Game. The list holds every `.pud` in the `Maps` folder of `<data>/game`, subfolders included, so
copy your own maps there. When the game ends, the score screen leads back to the main menu.

### Controls

| Input | Action |
|---|---|
| **Left click or drag** | Select. Shift adds or removes; Ctrl or a double-click takes every unit of that type on screen. |
| **Right click** | The right-click command (move, attack, harvest). With `--second-mouse-style` it centres the view instead. |
| **Command buttons** | Click one or press its hotkey. A command that needs a target takes the next left click on the map or minimap; a right click cancels. |
| **Control groups** | Ctrl+digit sets a group; the digit recalls it (twice quickly centres the view). |
| **Scrolling** | Screen edge, arrow keys, Home, End, Page Up, Page Down, keypad. Left click or drag on the minimap moves the view. |
| **Placing a building** | While you choose a site, the selected workers stand still; they move again when you place the building, or when you leave placement (right click, Escape, another command). |
| **Speed and pause** | `+` and `-` change the game speed; Ctrl+P pauses. |
| **Menu** | F10 or Alt+M opens the game menu; Escape closes it. Tab moves between controls in dialogs and Enter presses a button. |

The pointer is locked to the window during a game, as in full screen, so pushing it against an edge scrolls the map. The game menus free it, and so does holding Cmd (the Windows key elsewhere) or switching to another app. The Settings screen's **Mouse lock** row turns this off for good; `--no-mouse-lock` turns it off for one run. The full key list is in the specs under [`docs/specs/ui`](docs/specs/ui/).

### Touch controls

Touchscreens are our own addition (the original has no touch): fingers become the clicks and keys the game already
knows, so the rules don't change. Touch is on from the start on iOS and Android, and on a desktop when you touch a
touchscreen or use `--touch-emulation`.

| Do this | To |
|---|---|
| Drag a finger on the map | Scroll the map (a flick glides on) |
| Hold a finger still a moment (a ring shows), then drag | Draw a selection box |
| Tap a unit | Select it (a near miss still counts) |
| Tap the ground, an enemy or a resource, with your units selected | Order them there, as a right click does |
| Tap twice | Select all of that type on screen |
| Hold a finger still until a pulse shows, then lift | Right click (nothing happens while the finger stays down, so you can still drag a box) |
| Tap or drag the minimap; hold it | Move the view; order the selected units there |
| Place a building or choose a target | Drag the outline (or tap where it should go), then tap it to confirm, as the status bar says; a finger at the map's edge scrolls |
| Two fingers: drag; pinch | Scroll the map; zoom (on phones; a setting on tablets) |
| Three fingers: tap | Open the game menu |

**The side bars.** The left bar holds, from the top: Menu (three lines), Pause (two bars; a play triangle while
paused), the Shift (⇧), Ctrl (⌃) and Alt (⌥) latches (held for your next click), a plain-click latch (a pointer
with a click mark: the next tap selects, never orders) and the keyboard. The right bar holds the control groups,
numbered 1 to 9: a tap recalls a group, two taps centre on it, and holding one makes the selection that group. With
the `idle-worker` option on, a pickaxe button selects the next idle worker. A latch waiting for its click is outlined
in yellow; a button that is on is filled yellow.

**Pick units one by one:** double-tap Shift to lock it (a lock shows in its corner), then tap units to add or remove
them; tap Shift again to let go. Ctrl and Alt lock the same way.

**Settings and help.** The Settings screen's Touch page changes how a drag, a tap, two fingers, the pinch and the
holds work, the button size and which bar is which. The Touch Controls help sums all this up on two pages: the
gestures, then each button as the bars draw it (Next, Back and OK turn the pages). A phone or tablet shows it at your
first game, and it is under Help in the game menu and the Touch controls link on the main menu. On a phone or tablet
there is no Exit Program (the system closes apps), and Android's Back button is Esc.

**Try it on a desktop** with `--touch-emulation`: the mouse is one finger, and holding Alt as you press adds a second
finger (Alt+Shift: beside the first, Alt+Ctrl: a third). How it works, and the measurements it uses, is in
[`docs/design/touch.md`](docs/design/touch.md).

### Controllers

A gamepad works on its own or together with the mouse and a touchscreen (the original has no controller support;
like touch, the pad only presses the buttons and keys a player could, so the rules don't change). Plug one in or pair
it any time; SDL knows the common pads (Xbox, PlayStation, Switch, the Steam Deck's own controls, the PS Vita's).

**Default mapping.** The Xbox labels are shown; on a PlayStation pad A, B, X, Y are Cross, Circle, Square,
Triangle and LB/RB/LT/RT are L1/R1/L2/R2. On a Switch pad the face buttons go by place, so B (bottom) clicks and A
(right) right-clicks unless you turn on **Swap confirm and back**.

| Xbox | PlayStation | In a game | In the menus |
|---|---|---|---|
| Left stick | Left stick | Move the pointer (small push: slow, full push: fast) | The same |
| A | Cross | Left click; hold and move to drag a selection box | Left click |
| B | Circle | Right click (move, attack, harvest) | Right click |
| X | Square | Command buttons: the d-pad steps between them, A presses, B goes back | - |
| Y | Triangle | The pointer to the minimap, and back | - |
| LB | L1 | Shift, held (add to or remove from the selection) | The same |
| RB | R1 | Ctrl, held (all of that type on screen) | The same |
| LT | L2 | The pointer twice as fast, held | The same |
| RT | R2 | Alt, held | The same |
| D-pad | D-pad | Control groups 1 to 4 (up, right, down, left): recall, twice to centre; RB held: set; RT held: centre and recall | Arrow keys |
| Right stick | Right stick | Scroll the map | Scroll lists |
| Menu (☰) | Options | Game menu (F10); Esc while a menu is open | Enter |
| View (⧉) | Create | Pause and resume | Esc |
| Left stick press | L3 | The system keyboard, for chat and save names | The same |
| Right stick press | R3 | Next idle worker (F6, with the `idle-worker` option) | - |

The control groups are the Enhanced rules' (the default; Classic is opt-in and multiplayer only). The Vita has no
L2, R2 or stick presses. The full table, by pad maker, is in [`docs/design/gamepad.md`](docs/design/gamepad.md).

Whichever you used last has the pointer: the stick moves on from where the mouse or a finger left it, and moving the
mouse takes it back. While a pad is connected the touch side bars hide. The Settings screen's Controller page (shown
once a pad is connected) sets the pointer speed, acceleration, the dead zone, swapping South and East for Nintendo's
layout, and the side bars. Details: [`docs/design/gamepad.md`](docs/design/gamepad.md).

### iPhone and iPad (early)

Tidewater builds as an iOS app that runs in the Simulator (a real device needs your own signing; not tried on a device yet). It is the same program: touch is on from the start, the picture fits inside the safe area, and the game pauses when you leave the app. Details, signing and limits: [`docs/design/ios.md`](docs/design/ios.md).

**Build it.** You need Xcode, [xcodegen](https://github.com/yonaskolb/XcodeGen) (`brew install xcodegen`), CMake and rustup:

```sh
platform/ios/build.sh sim --app     # library, Xcode project and Tidewater.app for the Simulator
xcrun simctl install booted platform/ios/DerivedData/Build/Products/Debug-iphonesimulator/Tidewater.app
xcrun simctl launch booted dev.tidewater.client
```

To install a release on a device without Xcode, use the unsigned IPA ([steps](docs/design/ios.md#install-the-unsigned-ipa)). To build for a device yourself, run `platform/ios/build.sh device`, copy `platform/ios/Config/Signing.example.xcconfig` to `Signing.xcconfig`, fill in your team ID and open `platform/ios/Tidewater.xcodeproj`.

**Get the game files onto it.** Copy a game folder (a GOG install, or the folder holding `War2Dat.mpq`) or a CD image (`.iso`, `.bin`/`.cue`, `.mdf`, `.nrg`) into Tidewater's folder: in Finder (device, Files tab), in the Files app (On My iPhone, Tidewater), or for the Simulator into the app's `Documents` folder. Open the app and the first-run screen lists what it finds; tap one, or tap Scan again after copying. "Choose game folder..." opens the system picker instead. **A GOG installer (`.exe`) does not work on iOS**, because unpacking it needs `innoextract`, which cannot run there: unpack it on a computer, or install the game and copy the folder.

### Android (early)

Tidewater builds as an Android app that runs in the emulator (it is not yet tried on a phone). It is the same program as the desktop and iOS builds: touch is on from the start, the screen stays in landscape, and the game pauses when you leave the app. Details and limits: [`docs/design/android.md`](docs/design/android.md).

**Build it.** You need rustup, [cargo-ndk](https://github.com/bbqsrc/cargo-ndk) (`cargo install cargo-ndk`), CMake, a JDK (17 to 24) and the Android SDK with an NDK and `platforms;android-35`:

```sh
platform/android/build.sh          # libmain.so for arm64-v8a and x86_64, then a debug APK
adb install -r platform/android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.tidewater.client/.TidewaterActivity
```

`--abi arm64-v8a` builds one ABI; `--libs-only` stops before Gradle. This debug APK is signed with Gradle's debug key. The releases' APK is signed with a release key once the maintainer has set it up (see [docs/release.md](docs/release.md#android-apk)); `platform/android/build.sh --release --version X.Y.Z` builds that kind.

**Get the game files onto it.** Either copy a game folder (a GOG install, or the folder holding `War2Dat.mpq`) or a CD image (`.iso`, `.bin`/`.cue`, `.mdf`, `.nrg`) into the app's folder with a computer, for example `adb push Warcraft2 /sdcard/Android/data/dev.tidewater.client/files/` (open the app once first so the folder exists), then tap Scan again on the first-run screen; or tap "Choose game folder..." or "Choose CD image..." to pick from the phone's storage, which Tidewater copies into its cache first. **A GOG installer (`.exe`) does not work on Android**, because unpacking it needs `innoextract`: unpack it on a computer, or install the game and copy the folder.

### PS Vita (early)

Tidewater builds as a PS Vita app that starts in the Vita3K emulator (it is not yet tried on a Vita, and the buttons and sticks are not read yet). The build uses a pinned nightly Rust toolchain and the [Vita SDK](https://vitasdk.org). Details and limits: [`docs/design/vita.md`](docs/design/vita.md).

```sh
export VITASDK=/path/to/vitasdk
platform/vita/build.sh             # platform/vita/build/tidewater.vpk
```

**Install it** by opening the VPK in VitaShell. **Get the game files onto it:** copy a game folder or a CD image into `ux0:data/Tidewater` (VitaShell's FTP or USB), then choose Scan again on the first-run screen.

### How faithful it is

Tidewater follows the behaviour of the original game, not only its looks. The **Enhanced** rules are the default and the target: single player always uses them. The **Classic** rules are an opt-in for multiplayer only (a choice in the multiplayer menus, or `classic` in `--setup`); Tidewater has them only where the specs describe them.

Fidelity is measured, not assumed. The simulation is deterministic and integer-only. `twtool oracle` compares it cycle by cycle with recorded traces of the original game and reports the first record that differs. Every campaign mission (human, orc and both Beyond the Dark Portal campaigns) and six skirmish maps, played by the computer players with the human side idle, run identically to the original for 60000 cycles or until the game ends, about 2.4 million cycles in all, Enhanced rules. Every campaign mission also matches when a scripted person plays the human side, and when the game is saved and loaded twice through its own dialogs. Hundreds of shorter traces cover spells, naval and transport scenarios and specific rules. The Classic rules are checked on a few skirmish maps only, and two-person network play is not measured yet. The traces are not in this repository, so this comparison runs on the maintainers' machines, not in CI.

Some screens are our own layout, because the original's dialog definition files are not specified yet: the menus, dialogs, briefing and score screen look different from the original.

## Settings

**Open Settings from the main menu: press F2, or click or tap the "Settings (F2)" link at the bottom right.** Choose a
value in each drop-down, then **OK** to apply and save, **Cancel** (or Esc) to throw the changes away, or **Defaults**
to put every row on every page back to its default (Cancel still discards that). Changes are written to `settings.txt`
at once, so a phone app that the system closes keeps them. The screen works with the mouse, the keyboard (Tab and Shift+Tab or Up and
Down move, Enter opens a drop-down or presses a button, Left and Right change a value) and touch. It is our own addition: the
original's main-menu buttons are unchanged.

| Page | What it holds | Takes effect |
|---|---|---|
| **General** | Window or full screen (desktop), Scaling, Widescreen, Mouse lock (desktop), Intro video, Check for updates (desktop release builds), the imported game files with **Import game files again...**, and **Open settings folder** (desktop). | Window, Scaling and Mouse lock at once; Widescreen from the next game; Intro video and Check for updates the next time Tidewater starts |
| **Interface** | Health bars, idle worker button, game clock, control group bar, smart attack-move, minimap ping, record replays, magnifying zoom. | At once or from the next game |
| **Sound** | Unit voices, sound effects and music, 0 to 100 percent in steps of 10. | At once |
| **Gameplay** | Rally points, production queue, selection limit, extra game speeds, shift-queued orders. These change the game rules: they apply to single-player games you start, and in a multiplayer game the host chooses them in the lobby. | From the next game |
| **Keys** | Every key you can rebind, in a scrolling list, with **Reset keys**. Desktop only (a phone has no keyboard to rebind). | At once |

**During a game, press Ctrl+G** (F2 is the first remembered view there) **or tap "Settings (Ctrl+G)" in the Options dialog**
(F10, Options). It opens over the game, which stops while it is open, as for the game menu; in a network game it keeps
running, so nobody waits for you. Everything works as on the main menu, and your volumes, health bars, clock and the
like apply at once on OK. The **Gameplay** page is read-only there, because those rules were set when the game started.
**Import game files again...** is hidden. OK, Cancel and Esc return to the game.

**Rebinding a key:** open the **Keys** page, choose an action (click it, or move with the arrow keys and press Enter),
then press the new key (Esc gives up). Your own keys show in gold, the original's in grey; Backspace puts a row's
original key back, **Reset keys** every key. A key that another action has is not bound, and the description says which
action has it: give that action another key first (two actions swap in two steps). A game key needs Ctrl or Alt with a
letter, a command button a bare letter or digit; a letter, a digit, F1 to F12, minus and equal can be used, and F10,
Esc, Enter, Space and the arrow keys cannot. **OK** writes the keys into the `[keys]` section of `settings.txt` and the
game uses them at once; Cancel discards them. Ctrl+G always opens the Settings screen in a game.

Rows that make no sense on a phone (window, mouse lock, update check, settings folder) are hidden there. **Import game files
again...** opens the import screen over the running program; Esc there leaves everything as it was and returns to the
main menu. It is hidden when the game files are read in place with `--install`.

**Power users** can still edit `settings.txt` (its `[keys]` section holds the keys the Keys page sets; **Open settings
folder** finds it) or give a flag. A flag lasts for one run: a choice you make on the screen replaces the run's flag for that
option, and `--no-mouse-lock`, `--no-intro` and `--no-update-check` still win over the screen's rows for the run.
`--goto settings` opens the screen directly.

## Optional improvements

Tidewater adds a few optional improvements that the original game does not have. **With none switched on, the game looks,
sounds and plays as the original does.** Turn them on in the [Settings screen](#settings) (the Interface, Sound and Gameplay
pages, and Scaling and Widescreen on General). Power users can also use the `[extras]` section of the
settings file (`settings.txt`), or for one run a flag; `--no-extra-NAME` puts an option back to its default.
The idle-worker key is F6; keys can be rebound on the Settings screen's Keys page. How the options work, the `[keys]` file format and how to add an option is in [`docs/design/extras.md`](docs/design/extras.md).

| Option | Flag | Values | Default | What it does |
|---|---|---|---|---|
| `health-bars` | `--extra-health-bars VALUE` | off, hovered, selected, always | off | Hit point bars over units: only those under the pointer, only selected ones, or all (C+L) |
| `idle-worker` | `--extra-idle-worker` | on, off | off | An idle-worker button at the top of the screen and the F6 key: select your next idle worker |
| `game-clock` | `--extra-game-clock VALUE` | off, clock, clock-apm | off | The game clock over the map view, with or without your actions per minute |
| `group-bar` | `--extra-group-bar` | on, off | off | A strip over the map view's top left listing your control groups with their sizes: click one to recall it |
| `smart-attack-move` | `--extra-smart-attack-move` | on, off | off | A right-click on open ground with only fighting units selected attack-moves there instead of moving (C+L) |
| `volume-voices` | `--extra-volume-voices VALUE` | 0 to 100 | 100 | Unit voices, in percent of the sound volume (100 leaves them as the game sets them) |
| `volume-effects` | `--extra-volume-effects VALUE` | 0 to 100 | 100 | Sound effects, in percent of the sound volume |
| `volume-music` | `--extra-volume-music VALUE` | 0 to 100 | 100 | Music, in percent of the music volume |
| `scaling` | `--extra-scaling VALUE` | auto, integer, sharp | auto | How the picture fits the window: automatic (whole-number scales unless they leave wide borders, then any size drawn sharp bilinear), only whole-number scales, or always any size, sharp (touch platforms always use sharp) |
| `rally-points` | `--extra-rally-points` | on, off | off | Right-click with a training building selected to set a rally point: the units it trains go there (S) |
| `production-queue` | `--extra-production-queue` | on, off | off | Buildings queue up to five jobs, paid when queued; Cancel removes the last (S) |
| `selection-limit` | `--extra-selection-limit VALUE` | 9, 12, 18, unlimited | 9 | How many units you can select and command at once: nine as in the original, 12, 18 or no limit (S) |
| `speed-levels` | `--extra-speed-levels VALUE` | off, 1, 2, 3 | off | Game speeds above Fastest, 1 to 3 more levels (Enhanced rules only) (S) |
| `minimap-ping` | `--extra-minimap-ping` | on, off | off | Alt-click on the minimap pings that place on your allies' minimaps, and shows their pings on yours |
| `shift-queue` | `--extra-shift-queue` | on, off | off | Hold Shift while giving a move, attack, attack-move, patrol, repair, harvest or build order to add it to the unit's order queue (up to 8) instead of replacing its order (S) |
| `auto-replay` | `--extra-auto-replay VALUE` | off, menu, always | off | Record every game: `menu` keeps the recording so the game menu can save it, `always` also writes each game's replay to the replays folder of the data directory as it is played |
| `widescreen` | `--extra-widescreen VALUE` | off, 16:10, 16:9, auto | off | A wider game screen, the extra width being map: 16:10, 16:9, or auto to match the window (ignored on touch devices; menus and videos stay 4:3) (C+L) |
| `map-zoom` | `--extra-map-zoom VALUE` | off, wheel | off | `wheel`: the mouse wheel over the map view magnifies it up to 4 times around the pointer and back to the normal view, never showing more map than the original does (on touch devices the pinch is `touch-pinch`) (C) |
| `touch-drag` | `--extra-touch-drag VALUE` | pan, pan-no-glide, box | pan | Touch only. A finger dragged on the map scrolls it (`pan` glides on after a flick, `pan-no-glide` stops where the finger lifts); hold it still a moment first to draw a selection box instead. `box`: a drag draws the box at once, as the mouse does |
| `touch-smart-tap` | `--extra-touch-smart-tap VALUE` | on, off | on | Touch only. With your units selected, a tap on the ground, an enemy or a resource orders them there, as a right click does; a tap on your own unit or building selects it. `off`: every tap is a left click |
| `touch-two-finger` | `--extra-touch-two-finger VALUE` | pan, box, off | pan | Touch only. Two fingers dragged together scroll the map (`pan`), are the corners of a selection box (`box`), or do nothing (`off`; a pinch still zooms if it is on) |
| `touch-pinch` | `--extra-touch-pinch VALUE` | auto, on, off | auto | Touch only. Spreading two fingers magnifies the map view up to 4 times. `auto`: on for phones (a screen whose short side is under 90 mm), off for tablets |
| `touch-hold` | `--extra-touch-hold VALUE` | normal, long, longer | normal | Touch only. How long a finger is held still before a drag draws a box, and before it right-clicks: `normal` 0.3 and 0.45 seconds, `long` 0.4 and 0.6, `longer` 0.5 and 0.75 |
| `touch-buttons` | `--extra-touch-buttons VALUE` | medium, small, large | medium | Touch only. The size of the side bars' buttons: `medium` 9 mm, `small` 7 mm, `large` 11 mm (smaller when a bar is too narrow) |
| `touch-swap-bars` | `--extra-touch-swap-bars` | on, off | off | Touch only. The control-group buttons go on the left and the menu, pause and modifier buttons on the right |
| `touch-menu-tap` | `--extra-touch-menu-tap VALUE` | on, off | on | Touch only. A tap of three fingers at once opens the game menu (the Menu button always does) |
| `pad-speed` | `--extra-pad-speed VALUE` | normal, slow, fast | normal | Controller only. How fast the left stick moves the pointer (holding the left trigger doubles it) |
| `pad-acceleration` | `--extra-pad-acceleration VALUE` | on, off | on | Controller only. A small push on the left stick moves the pointer slowly for aiming and a full push fast; it also slows over units and buttons. `off`: the speed follows the push evenly |
| `pad-dead-zone` | `--extra-pad-dead-zone VALUE` | normal, small, large | normal | Controller only. How far a stick moves before it counts: raise it if the pointer drifts on its own |
| `pad-swap` | `--extra-pad-swap` | on, off | off | Controller only. The right face button clicks and the bottom one right-clicks, for pads with Nintendo's layout |
| `pad-hide-touch` | `--extra-pad-hide-touch VALUE` | on, off | on | Controller and touch. While a controller is connected the touch screen's side bars hide (taps still work, and the shoulder buttons are Shift and Ctrl) |

**C+L** marks an option that gives you an edge (health bars, smart attack-move, widescreen). In a game with other players the host chooses it in the lobby's "Extras" box (from the command line, `--lobby extras=health-bars=always/widescreen=16:9`; the host's own settings stand for the ones not named) and every player, observers included, plays with the host's choice, whatever their own setting. In single-player games and replays you get your own. **S** marks an option that changes the simulation (rally points, the production queue, shift-queued orders,
the selection limit and the extra speed levels): the flag or the setting starts a single-player game with it, and in a
network game the host turns it on in the lobby's "Extras" box and everyone plays with the host's choice. The game is
saved and recorded with it.

## Command-line reference

### `tidewater` (the game)

Run `tidewater --help` for the same list. The optional positional argument is a `.pud` file path or a map name inside the archives.

**Everyday options**

| Flag | What it does |
|---|---|
| `--import <PATH>` | Import the game files from a game folder, a GOG installer, a folder of `.mpq` files or one archive, then exit. The exit code says what went wrong: 10 path not found, 11 DOS edition, 12 installer (not readable, or `innoextract` missing), 13 no game files, 14 no `War2Dat.mpq`, 15 damaged archive, 16 incomplete files, 17 no space, 18 i/o error, 19 CD image. |
| `--reimport` | Open the import screen even though files are already imported, to import again. Esc leaves it as it was. |
| `--version` | Print the version (`dev` for a build that is not a release). |
| `--no-update-check` | Do not look for a newer release at start-up (see [Update](#update)). |
| `--data-dir <DIR>` | Tidewater's data directory (imported files, `Save`, settings). Default: the per-user data directory. |
| `--install <PATH>` | Developer override: read the game files in place instead of the imported copy. Also `TIDEWATER_INSTALL`. |
| `<MAP>` | Start this map at once (a `.pud` path or an archive name). |
| `--setup <SETUP>` | Game setup: `campaign:ENTRY`, or `custom` with choices such as `custom:race=orc,opponents=2,resources=high,terrain=swamp,units=one-worker,fixed-starts,fog=off`. |
| `--no-intro` | Skip the intro video for this run (the Settings screen's Intro video row does it for good). |
| `--extra-NAME`, `--no-extra-NAME` | Switch an optional improvement on, or back to its default, for this run. The names and values are in the [table above](#optional-improvements). |
| `--ui-scale <1-8>` | Open a 640N x 480N-point window instead of the largest whole-number scale that fits. |
| `--save-dir <DIR>` | Where saved games go (default: `Save` in the data directory). |
| `--settings <FILE>` | The settings file (default: `settings.txt` beside the saved games). |
| `--no-mouse-lock` | Do not lock the pointer to the window during a game. Overrides the Settings screen's Mouse lock row for this run. |
| `--second-mouse-style` | Right click centres the view instead of giving a command. |
| `--mouse-scroll-speed <0-6>` | Mouse scroll speed; 6 is fastest. Default: the stored setting. |
| `--key-scroll-speed <0-6>` | Keyboard scroll speed; 6 is fastest. Default: the stored setting. |

**Development aids** (not part of the original game)

| Flag | What it does |
|---|---|
| `--touch-emulation` | The mouse is one finger (Alt, Alt+Shift, Alt+Ctrl at the press add more), and the touch controls show beside the picture. See [Touch controls](#touch-controls). |
| `--touch-dpi <PPI>` | The screen's density in physical pixels per inch, for touch sizes (taps, slop, buttons). Default: 140 points to the inch. |
| `--free-zoom` | Development aid: the mouse wheel zooms the map view anywhere (1/4 to 8 map pixels per screen pixel). The `map-zoom` option is the player's magnifying zoom. |
| `--zoom <N>` | Starting zoom; implies `--free-zoom`. |
| `--goto <SCREEN>` | Open a flow screen directly: `title`, `menu`, `campaign`, `single` (the single-player menu), `custom`, `settings` (the Settings screen), `multi` (the multiplayer menus), `mission:E`, `act:E`, `briefing:E`, `score-victory:E`, `score-defeat:E`, `ending:E`, `credits:tod`, `credits:bdp`, `show-credits` or `video:N`, with E a mission entry (0 to 51). |
| `--demo-outcome <victory\|defeat>` | End the game at once, to see the dialog and score screen. |
| `--demo-move <X,Y>` | Order all your units to a tile at the start. |
| `--demo-fight` | Send every player's units against the nearest enemy within 16 tiles. |
| `--demo-select <N>`, `--demo-type <N>`, `--demo-place <N>`, `--demo-site <X,Y>` | Preselect your first N units, narrow that to one unit type, start in building placement, and click a site. |
| `--record-replay` | Record the game to a `.twrep` replay in the `replays` folder of your user data (see [Replays](#replays)). Off unless given. |
| `--replay <FILE>` | Play a recorded game back, as the recording player saw it. Cannot be combined with `<MAP>`, `--record-replay` or the setup options. |
| `--host [PORT]`, `--join <ADDR>`, `--lobby <SPEC>`, `--observe`, `--name <NAME>` | Network play without the menus. See [Multiplayer](#multiplayer-early). |
| `--script <NAME>`, `--script-out <DIR>`, `--headless` | Run a scripted scene for the frame snapshot test or the end-to-end network check (the `mp-*` scenes). `--script` and `--script-out` go together; `--headless` uses no window or GPU. |

### `twtool` (inspection and verification)

`twtool` reads game data from a path or `--install` given at run time and stores none. Run `twtool <command> --help` for each command's arguments.

| Subcommand | What it does |
|---|---|
| `import-check` | Dry run of the import: classify a path, check the files, print the report, copy nothing. |
| `disc-list` | List the files on a CD image (size and path), to see how a disc is laid out. |
| `mpq-info` | Summarise an archive's block table. |
| `mpq-probe` | Print which of the names on stdin an archive contains. |
| `mpq-extract` | Extract the named files into a directory. |
| `mpq-unnamed` | Dump in-use blocks whose names are not listed. |
| `render-map` | Render a map's terrain to a PNG. |
| `render-grp` | Render every frame of a GRP sprite sheet into one PNG. |
| `grp-sheet` | Draw frame 0 of each GRP named on stdin into one grid. |
| `render-pcx` | Render a PCX image to a PNG. |
| `smk-frame` | Decode a Smacker video up to a frame and write it to a PNG. |
| `smk-audio` | Decode one audio track of a Smacker video to a WAV file. |
| `units` | Print the unit stats table as TSV. |
| `upgrades` | Print the upgrade table as TSV (a map's own, or the default). |
| `strings` | Print a string table (`*.tbl`) as TSV. |
| `scripts` | Measure each unit type's move and attack timing from the animation scripts. |
| `script-ops` | List one unit type's animation script instruction by instruction. |
| `fell` | Fell trees in a field of one forest shape and print the resulting forest table rows. |
| `sim` | Run a map headless: give orders, step some ticks, print where units ended up and the state hash. |
| `snapshot` | Print fingerprints of the simulation over every campaign and custom map; the behaviour snapshot `tests/snapshot.rs` checks. |
| `replay` | Play a `.twrep` replay headlessly, check every state-hash checkpoint and exit non-zero at the first mismatch. |
| `desync-diff` | Play two replays (or one twice with `--local SLOT`) and print the first cycle whose state hash differs and the first differing field. |
| `oracle` | Compare the simulation cycle by cycle with a trace of the original game and report the first record that differs. |

## Multiplayer (early)

Two to eight machines can play one game over TCP. Every machine runs the whole simulation and they exchange only commands, so all copies must be the same build with the same game data; the handshake refuses a mismatch and says which part differs (protocol, build, data tables, game edition or game files).

**From the menus:** Multi Player Game, choose Enhanced or Classic, enter a name, then **Local Network** (games on your network are listed; Create Game opens one) or **Direct IP** (type the host's address, or Create Game). Battle.net is shown greyed out. The game setup lobby has the scenario, up to eight rows with races and chat boxes, and Start Game once two or more are in: you and another player, or you and a Computer row (Available rows are ignored). A joiner without the host's map is sent it automatically.

- **Host's choices.** The host's "Extras" box sets the options that give an edge or change the rules for everyone (see [Optional improvements](#optional-improvements)). Only the host can save, load or return to the chat room from the game menu.
- **Observers** are our own addition. The host picks 0 to 4 places for them in a small "Observers" box under the player rows (none by default), and on the game list a **Watch Game** button next to Join Game lets you watch instead of play. It shows for a game that may have a place (a Local Network game says so on the list; a Direct IP game cannot, so the host refuses with a message when it has none). Watchers are listed under the rows, take no slot, see the whole map, give no orders and do not count for victory or defeat.
- **In the game,** Enter opens the chat line; start a line with `/a ` to send it to allies only. `/diplomacy allies=0,2 vision=0,2 victory=on` sets your alliances, shared vision and allied victory (a stand-in until the diplomacy screen). The game menu does not stop the game for the others. In Enhanced games each player has three pauses and everyone sees who paused and how many are left.

**From the command line** (no menus needed):

```sh
tidewater 'Maps\Cramped BNE.pud' --host --lobby humans=2,races=human/orc,seed=1   # host, waits for joiners
tidewater --join HOST-ADDRESS                                                       # each joiner
```

The default port is 6112: TCP for the game, which the host opens on its firewall (in a two-player game joiners need nothing open; with three or more players, or an observer, each joiner also listens on a free port so it can take over from the host, and the system may ask to allow it), and UDP for the Local Network list, where a game's creator broadcasts a small packet once a second (`TIDEWATER_NO_BEACON=1` turns that off; Direct IP works where broadcasts do not). Write the map before `--host`; `--host PORT` picks another port and `--join ADDR:PORT` follows it. Enhanced rules are the default; Classic is opt-in (`--setup custom:classic`). Leaving the game (Ctrl+Q, Quit to Main Menu or Surrender) shows "Player NAME left the game." to the others; Ctrl+F1 leaves, and Alt+F2 drops players the game is waiting for. When the host leaves a game of three or more, the game goes on: the lowest remaining player becomes the host and the others reconnect to it ("NAME is now the host."); this needs the players to reach one another directly, as on a LAN, and otherwise each plays on alone. Add `--record-replay` on each side, then `twtool desync-diff` on the two files to confirm both saw the same game. A command-line host leaves places for watchers with `--lobby observers=N`, and a watcher joins with `--join HOST-ADDRESS --observe`. Details: [docs/design/network.md](docs/design/network.md).

## Replays

Run with `--record-replay` and the game writes a `.twrep` file to the `replays` folder of your user data (never into the game folder). It holds the map's name and hash, the seed, the setup and the rules, then the commands given each cycle and a state-hash check every 100 cycles. Recording is a flag rather than always on, so nothing is written unless you ask. The `auto-replay` option does the asking for you: `--extra-auto-replay always` (or the `[extras]` setting) writes every game's replay, single-player or network, and `menu` holds the recording in memory and adds a Save Replay button to the in-game menu.

`tidewater --replay FILE` plays it back as the recording player saw it. Your input is ignored except the game menu, pause (Ctrl+P) and speed (`+` and `-`). If the game ever stops matching the recording, playback stops at the first check that fails and says which cycle. `twtool replay FILE` does the same check without a window, and `twtool desync-diff` finds the first cycle and field where two runs differ. A replay is refused on a map that is not the one it was recorded on. Games that were loaded from a save, or that used the `--demo-*` options, are not recorded. A network game records on each side with the same flag.

## Roadmap

What is done, what is early and what is not built. Not built: Battle.net and modem play.

| Track | What it adds | Status |
|---|---|---|
| **Lockstep multiplayer** | Local Network and Direct IP games over TCP. The simulation is deterministic so that this is possible. | Early: menus, lobby, LAN game list, map transfer, chat and pause counts, the game going on when the host leaves, observers (our own addition); no Battle.net or modem |
| **Custom games against the computer** | The Custom Scenario screen: race, opponents, resources, terrain, units and a map list. | Done |
| **Touch controls and phones** | Touch input on desktop, iOS and Android. PlayStation Vita support is under evaluation. | Early: tried in the iOS Simulator and the Android emulator only, not on a device |
| **Optional modern improvements** | Quality-of-life additions, all off by default so the default game matches the original. | Done: see [Optional improvements](#optional-improvements) |

## How it is built

Tidewater is a **clean-room** project. Behaviour comes only from reviewed written specifications in [`docs/specs`](docs/specs/), public format documentation, open-source code under compatible licences, and measurements of the original game's inputs and outputs. No code from the original program and no game data is in this repository. Read [CLEANROOM.md](CLEANROOM.md) for the rules, and [docs/README.md](docs/README.md) for the design documents and the spec index.

The code is a Cargo workspace:

| Crate | Job |
|---|---|
| `crates/formats` | Readers for the game's data: MPQ archives, maps, tilesets, sprites, images, fonts, Smacker video, string and unit tables. |
| `crates/import` | Game-file import (`--import`): game folders, the GOG installer and CD images. |
| `crates/replay` | The `.twrep` replay format and running or comparing replays. |
| `crates/sim` | The deterministic game simulation, with no engine types. |
| `crates/client` | The game client: an engine-neutral core that composes each frame, and the `tidewater` binary, the SDL 3 shell. |
| `crates/twtool` | The inspection and verification tool. |
| `crates/ios`, `crates/android` | The phone apps' native libraries; the Xcode and Gradle projects are in `platform/`. |

## Development

Run this before every commit:

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

**Game-data tests.** Some tests need a real installation or recorded oracle traces. They are gated on `TIDEWATER_INSTALL` and `TIDEWATER_ORACLE_TRACES` and skip when those are unset. All other tests use synthetic data built by our own code; no game file is ever committed.

**Multiplayer end to end.** `TIDEWATER_INSTALL=<data>/game cargo test --release -p tidewater-client --test mp_e2e` starts two or three `tidewater` programs on this machine and plays seven games through the menus (Local Network, Direct IP, Classic, an observer, three players where the host leaves first, you alone against a Computer, two players and a Computer), then checks that every side saw the same game. It takes about 30 seconds in a release build and uses port 6112; details in [docs/design/network.md](docs/design/network.md#running-the-end-to-end-check).

**CI.** [GitHub Actions](.github/workflows/ci.yml) runs on pull requests and on pushes to `develop` and `main`. It checks format, clippy and the tests on Linux (the game-data tests skip there), then builds release binaries of `tidewater` and `twtool` on Linux, macOS and Windows, plus the iOS Simulator library, an unsigned iOS `.ipa` (uploaded as a workflow artifact) and the Android arm64 library.

**Branches and releases.** Work lands on `develop`; merging `develop` into `main` cuts a release, with the version worked out from the commit messages. Details: [docs/release.md](docs/release.md).

**Leak scan.** Run `git config core.hooksPath .githooks` once per clone. The hooks run `tools/leak_scan.py`, which blocks private names, secrets, game-data file types and large files from being committed. Never bypass them.

Commits use conventional commit messages (`feat:`, `fix:`, `docs:`). Contributors and agents should read [CLAUDE.md](CLAUDE.md) and [CLEANROOM.md](CLEANROOM.md) first.

## Licence and notices

*Warcraft*, *Warcraft II*, *Beyond the Dark Portal*, *Battle.net* and *Blizzard Entertainment* are trademarks or registered trademarks of Blizzard Entertainment, Inc. Tidewater is an independent project, not affiliated with, endorsed or sponsored by Blizzard. It contains no Blizzard code or game data; to play, you need your own copy of the game.

License: [MPL-2.0](LICENSE). The MPQ decompressors are ported from StormLib ([MIT](licenses/StormLib-MIT.txt)). Licences of the statically included third-party code are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
