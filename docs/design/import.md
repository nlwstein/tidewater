# Game-file import

Tidewater contains no game data. `crates/import` (no SDL, no `unsafe`) finds the user's *Warcraft II: Battle.net
Edition* files, checks them and copies the ones the client needs into Tidewater's data directory. It covers the
library and the command line, the first-run screen, the GOG installer and CD images (each below); the phone apps' file
pickers are in [ios.md](ios.md) and [android.md](android.md).

## Decision: always copy

An import **copies** the files into `<data>/game/`; it never keeps a pointer to the user's install. A copy survives
the install being moved, uninstalled or on a CD that isn't in the drive, and gives one place the client reads from on
every platform. `--install DIR` stays as a developer override that reads in place.

## Pieces

| Module | Job |
|---|---|
| `source` | Classifies a path: game folder (searched 6 levels deep, any letter case, breadth first; covers a GOG folder, a Wine prefix's `drive_c`, a `.app` bundle and a mounted CD, which is just a folder), single `.mpq`, folder of loose `.mpq` files. Reads a GOG installer (`setup_*.exe`, or its `.bin` part, which finds the `.exe`; see "GOG installer" below) and refuses any other `setup_*.exe` (`InstallerUnsupported`). Reads a CD image through `disc` (see "CD images" below) and refuses a DOS-edition copy (`DATA/*.WAR`; `DosEdition`). |
| `validate` | Opens every archive; requires `War2Dat.mpq`; reads key files the client loads at start (`rez\unitdata.dat`, `Art\font10x.fnt`, `rez\network.tbl`, the Forest tileset). Missing `Install.mpq` or `Maps/` are warnings ("no videos or music", "no multiplayer map list"); `War2Patch.mpq` is optional and silent. Returns a `Report` with player-facing messages. |
| `disc` | `Disc`: a read-only ISO 9660 + Joliet reader over the usual image files; `DiscFiles`: the disc as a `SourceFiles` (see "CD images" below). |
| `inno` | `InstallerReader`: the GOG installer as a `SourceFiles` (see below). |
| `copy` | Copies the allowlist (`*.mpq` at the top, `Maps/**`) into `<data>/.import-<pid>`, hashing as it goes, then swaps it in as `<data>/game`. The old import is moved aside first and restored if the swap fails; the temporary folder is always removed. |
| `manifest` | `game/manifest.json`: source kind, edition guess, each file's path, size and SHA-256, the import time (UTC). The known-hash table (`KNOWN_WAR2DAT`) is empty: no hashes collected yet. |

Only the allowlist is copied because the installation reader gives loose files priority over the archives, so stray
files must not come along.

## Command line

- `tidewater --import <path> [--data-dir <dir>]`: validate, copy, print the report and a summary, exit.
- `tidewater --reimport`: open the first-run screen even though `<data>/game` exists (Esc leaves it as it was).
- `twtool import-check <path>`: the same checks and report, copies nothing.
- `tidewater` without `--install` uses `<data>/game` when it exists, else opens the first-run screen (a run without a
  window prints how to import and exits with 1).
- `--data-dir` moves the whole per-user directory (`game`, `Save`, the settings file); `--save-dir` and `--settings` still override their parts.

Exit codes: 0 success, 10 path not found, 11 DOS edition, 12 installer, 13 no game files, 14 no `War2Dat.mpq`, 15
damaged archive, 16 incomplete files (a key file is missing or unreadable), 17 not enough space, 18 other i/o error, 19 CD image (unreadable, or the disc holds an installer). (12 covers every installer problem: not an Inno Setup installer, `innoextract` missing, extraction failed.) (2 is clap's,
for a bad command line.)

## First-run screen

With no `--install` and no imported copy, a run with a window opens the first-run screen (`client/src/firstrun.rs`,
core state `AppState::FirstRun`) instead of printing the import message. A scripted or headless run still prints the
message and exits. The screen is not part of the original; it exists only because the game data isn't there yet, so
every screen of the original stays as it was.

- **Look.** It draws into the usual 640 x 480 frame with its own 8 x 8 bitmap font (`firstrun/font.rs`, public domain,
  credited in `THIRD_PARTY_NOTICES.md`; no game font exists before the import) and plain rectangles.
- **Automatic search** (`firstrun/search.rs`, on a worker thread). It looks at the children of the usual folders whose
  names look like the game (`Warcraft`, `War2`) and runs each through `classify` and `validate`; the ones that pass are
  listed. Windows: `Program Files`, the GOG Galaxy and GOG.com folders, `GOG Games` on `C:` to `G:` (no registry:
  reading it would need `unsafe`). macOS: `/Applications`, `/Volumes`, `~/Applications`, `~/GOG Games`, `~/Games`, and
  `drive_c` of Wine, CrossOver and Whisky prefixes. Linux: `~/GOG Games`, `~/Games` and Wine, Lutris and Bottles
  prefixes.
- **Choosing.** "Choose game folder...", "Choose GOG installer..." and "Choose CD image..." open the system's folder or
  file dialog (`SDL_ShowOpenFolderDialog`, `SDL_ShowOpenFileDialog`; the `sdl-dialog` feature of `sdl3-sys`), and a
  file or folder dropped on the window (`SDL_EVENT_DROP_FILE`) does the same. Both come into the core as input events
  (`Input::picked`, `Input::dropped`) and go through the same import, so a GOG installer is read like any source and a CD
  image is read like any source. The core asks for a dialog with `Core::take_pick_requests`; the shell
  shows it. Nothing else of SDL is known to the core.
- **Importing.** The import runs on a worker thread through `Backend::import` (`DesktopBackend` calls
  `classify_with`, `validate` and `import`); the screen shows its progress (see [Progress](#progress)), then the result line ("Imported
  Warcraft II: Battle.net Edition (GOG). Videos and music: yes. Maps: 153.") for about a second and a half (Enter
  skips it), then loads the imported files and goes on into the normal start-up (or the map or `--goto` screen the
  command line asked for).
- **Errors.** The import crate's message is shown wrapped, with a Back button (Enter or Esc) to the list.
- **Input.** Mouse and keyboard: Up, Down and Tab (Shift+Tab backwards) move, Enter chooses, Esc goes back or quits;
  hovering focuses and a click chooses.
- **Tests.** The rules are tested through the core's input with a fake `Backend`; the scripted scene `firstrun` (a
  demo backend and a fake list of installs, so it needs no game data) has a frame snapshot in
  `tests/snapshots/firstrun.txt`.
- **Phones and the Vita.** iOS and Android use a shared folder and their own pickers instead of SDL's dialogs ([ios.md](ios.md), [android.md](android.md)); the Vita scans its data folder and has no picker ([vita.md](vita.md)).

## Progress

**Bottom line.** Every step of an import reports its progress through one callback (`tidewater_import::Progress`), and
the first-run screen and `--import` show the step by name, so a long read never looks stalled. It is the same code on
every platform.

| Step (`Step`) | When | What it measures |
|---|---|---|
| `ReadingDisc` | A CD image: its game files are copied out to a temporary folder (`DiscFiles::stage`) | Bytes written of the listed files' total, each chunk (and each file as it starts) |
| `Unpacking` | A GOG installer: `innoextract` unpacks it to a temporary folder | The temporary folder's size, measured every 200 ms, against the sizes `innoextract --list` gives |
| `Checking` | `validate` opens the archives and reads the key files | Nothing (`total` is 0) |
| `Copying` | `import` copies into `<data>/game`, hashing as it goes | Bytes copied of the total |

`classify_with` reports the first two (`classify` is it with no callback); the caller reports `Checking` before
`validate`. A game folder or an archive has no first step.

**The screen.** The importing page names the step ("Step 1 of 3: Reading the CD image", or "Step 1 of 2" when there is
nothing to read or unpack first), with animated dots. The bar fills per step, with the percentage, the megabytes done
of the step's total and the file it is on. A step with no size (`Checking`) slides a block along the bar instead. A
line under the bar says what the step does (reading a CD image "can take a minute or two"). The animation runs from the
frame count, so it moves even while no report comes (a file in a disc's installer archive is unpacked whole when it
opens), and scripted scenes stay the same every run.

## GOG installer

`classify` accepts the installer `setup_*.exe` (or its `-1.bin` part; the `.exe` beside it is used), and a folder that
contains one. Any `.exe` counts when it is an Inno Setup installer: a Windows program with the text `Inno Setup Setup
Data (` in its first 8 MiB. Another `setup*.exe` is `InstallerUnsupported`.

`inno::InstallerReader` implements `SourceFiles` for it. It drives the `innoextract` program (zlib licence; found
through `TIDEWATER_INNOEXTRACT`, then the `PATH`, then `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`): `innoextract
--list` finds the top-level `*.mpq` entries, then one `innoextract --include ... --output-dir` run extracts only those
and `Maps` into a staging folder under the system temp folder (`tidewater-installer-<pid>-<n>`). That folder is then
an ordinary game folder for `validate` and `copy`; `Source.staged` holds it and removes it when the last copy of the
source is dropped. So `validate` runs on the staged files before anything is swapped into `<data>/game`, and the
manifest's source kind is `gog-installer`. Extraction reports `Step::Unpacking` (see [Progress](#progress)); the GOG installer takes about 3
seconds for its 500 MB (a whole import is about 4 seconds on a laptop). The temp folder needs room for the extracted
archives (about 500 MB) on top of the copy.

Errors, all exit code 12: not an Inno installer (`InstallerUnsupported`), `innoextract` not found
(`InnoextractMissing`, says how to install it), or `innoextract` failed (`InstallerFailed`, with its last lines; an
Inno version it can't read shows up here, and the message adds a hint when no `.bin` part is next to the `.exe`).
Every message ends by offering the fallback of installing the game and choosing the folder.

**Why not a Rust reader.** The plan was the `innospect` crate (Apache-2.0, pure Rust). Version 0.2.0 parses this
installer (Inno Setup 5.6.2, 963 entries) but can't read external `setup-1.bin` slices ("not currently supported" in
its source; its README lists disk spanning as untested), and GOG's installer keeps all its file data in the `.bin`.
A pure-Rust path therefore needs slice support added upstream or a fork (decision pending). `InstallerReader` is the
seam: replacing the two `innoextract` calls leaves `source`, `validate` and `copy` untouched. The platforms that can't
run a helper program (Vita, sandboxed stores) need that replacement.

**Tests.** Unit tests need no game data: marker detection, `.bin` to `.exe`, listing parser, error paths, and a
stand-in `innoextract` script (unix) that exercises staging and cleanup. `crates/import/tests/gog_installer.rs` runs
the real installer when `TIDEWATER_GOG_INSTALLER` names it (skipped otherwise, and when `innoextract` is absent);
with `TIDEWATER_INSTALL` set it also compares every imported file's SHA-256 with an installed copy.
`twtool import-check <setup.exe>` takes the installer path like any other.

## CD images

`classify` accepts a disc image and `tidewater --import` / `twtool import-check` take it like any path. `crates/import/src/disc.rs`
is our own small read-only ISO 9660 reader (no new dependency); its module doc cites the public format sources.

**What is supported.**

| Image | How it is read |
|---|---|
| `.iso`, most `.img` | 2048-byte sectors |
| `.bin` through its `.cue` (BIN/CUE) | the sheet's first data track (`MODE1/2352`, `MODE2/2352`, `MODE1/2048`, `MODE2/2336`); `FILE` names match in any letter case; `INDEX 01` (or `INDEX 00`) gives the track's start, so a data track after audio works |
| `.bin` beside its `.cue` | the sheet is used even when you choose the `.bin` (a sheet that can't be read or names another file is ignored) |
| `.bin` alone | raw 2352-byte sectors (mode 1 and mode 2 form 1), found by signature; mode 1 is the first guess |
| `.mdf` (or the `.mds`, which finds the `.mdf`) | a plain sector dump, 2352 or 2448 bytes (with sub-channel data); the `.mds` content is not needed |
| `.nrg` | Nero footers, version 1 (`NERO`) and 2 (`NER5`): the track offsets come from the `ETNF`/`ETN2` chunks |

The sector layout is always decided by finding `CD001` in sector 16 (trying 2048, 2352, 2448 and 2336-byte sectors and
the data offsets 0, 8, 16 and 24), not by the extension, so a mislabelled image still works. Names come from the Joliet
tree when there is one (UCS-2 big-endian), else from ISO 9660 level 1 or 2 (`;1` and a trailing dot removed). Lookups
ignore letter case. HFS on hybrid discs is ignored, Rock Ridge is not needed, multi-extent files are joined.

**Finding the game.** Like a folder search: the shallowest folder (up to 6 levels) holding `.mpq` files, `War2Dat.mpq`
before `Install.mpq` before any other, plus its `Maps` folder. `DiscFiles` implements `SourceFiles` for exactly those files.
Because `validate` opens archives by path, `classify` first copies them from the image into a staging folder (the same
`Staging` the GOG installer uses, removed when the last copy of the `Source` goes), so the source kind is `cd-image` and
`validate` and `copy` run unchanged. Files are streamed by extent (32 sectors at a time); the image is never held in
memory. The temp folder needs room for the archives (about 500 MB) on top of the copy.

**When the game is inside an installer archive.** The Battle.net Edition CD has no loose `.mpq` files. Its 468 MB
`INSTALL.EXE` is a small Windows program with an MPQ archive directly behind it (the header is on a 512-byte boundary,
which `formats::mpq::Archive` already handles). `disc/installer.rs` looks for a large `.exe` or `.mpq` near the top
whose bytes contain an MPQ header and which holds `Files\War2Dat.mpq`. The archive is read in place through a seekable
`ExtentReader`. It lists:

- `Install.mpq`: the whole disc file, program and all (an installed game keeps the same file under that name, with the
  videos and music; the reader skips the program);
- `War2Dat.mpq` (and `War2Patch.mpq` when present): read out of the archive from `Files\`;
- `Maps/*.pud`: the 26 stock `* BNE.pud` maps from `Maps\`, by name. The archive holds about 127 more maps, but it
  stores names only as hashes, so their names can't be recovered and they are left out (an installed game doesn't have
  them in `Maps` either).

The archive on the disc is the game as first sold. The later patches (`UPDATES\*.EXE` on the disc) are programs that
patch an installed game; Tidewater doesn't run them, so a disc import has no `War2Patch.mpq` and a `War2Dat.mpq` that is
14 entries short of, and 6 entries different from, the patched one in a GOG install. The key checks and the scripted
campaign scene give the same frames as a GOG install.

**When the game is not loose and not in an archive.** A disc with no `.mpq` files and no such archive gets a specific
error (exit 19): `DiscHoldsInstaller` when it has `SETUP.EXE`, `INSTALL.EXE`, `.CAB` or `.MSI` files ("this disc holds
an installer ... install the game from the disc, then choose the installed folder"), `DosEdition` for `.WAR` files, else
`NoGameFiles`. An image that can't be read (no `CD001`, cut short, a cue sheet naming a missing file or listing no data
track, looping directories) is `DiscImageUnreadable` (also 19) with the reason. The fallback for every case is the old
advice: mount the image (or copy its files to a folder) and choose that.

**Finding out what a disc holds.** `twtool disc-list <image>` prints every file's size and path (the layout and the name
kind go to stderr). Run it on a disc to see at once whether the archives are loose or only inside an installer.

**Robustness.** Reads past the end of the file are errors, a directory seen twice is a loop, and depth (32), entries
(200,000), one directory's size (16 MiB) and the descriptor scan are capped, so a damaged image ends with an error,
not a panic or a hang.

**Tests** (all synthetic: a test-only ISO writer in `testiso.rs` makes primary + Joliet volumes, nested and multi-sector
directories and multi-sector files, then wraps them in raw sectors, `.mdf` and `.nrg` framing): listing and reading,
Joliet against ISO names, every framing through signature and through a cue sheet (audio-first, mixed case), `.mdf`/`.mds`,
Nero v1 and v2 with a leading offset, a multi-extent file, a large directory, classify plus a full import into a data
folder with the manifest, installer and DOS discs, junk images, truncated images, a directory loop, and a byte-flipping
fuzz loop; a disc whose game is in an installer archive (as `.iso`, MODE1 and MODE2 `.bin` and a `.cue`), seeking inside
a disc file, a `.bin` beside a `.cue` with audio first, `MODE1/2048` sheets, a `.bin` with no sheet, and the errors for a
sheet naming a missing file or listing only audio. A real Battle.net Edition BIN/CUE has been imported by hand; no real disc data is in the repository.

## Free space

There is no portable free-space query without `unsafe` or a new dependency, so there is no check before copying. A
full disk surfaces as a write error, which is reported as "Not enough free space" and cleans up the temporary
folder; an earlier import is untouched.

## Sources

The copy step reads through the `SourceFiles` trait (`list()` relative paths and sizes, `open(rel)`), with a plain
folder (`FolderFiles`) and the GOG installer (`InstallerReader`, above) as implementations. The CD image (`DiscFiles`, above) is a third; `copy::import_from` takes any of them unchanged. Validation opens archives by
path, so a source that isn't a folder first stages the archives in a temporary folder and validates them there.
