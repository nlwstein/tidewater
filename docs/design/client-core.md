# Design: an engine-neutral client core

**Bottom line.** The client is a plain-Rust core that the host calls once per frame:
`advance(real_time_elapsed, platform) -> &Frame`. The shell only shows the frame (640 × 480, or wider in a game with
the `widescreen` option), hands over input, pulls mixed audio, provides the files, and knows the time. **SDL 3 is the
only shell**: the `tidewater` binary is the core in an SDL 3 window, through our own thin wrapper over `sdl3-sys`, and
the iOS, Android and PS Vita apps run the same shell code. Every build needs CMake and a C compiler (see the
[README](../../README.md#build-from-source)).

## Decisions

- **One shell, SDL 3.** It covers desktop, iOS, Android and the PlayStation Vita with one loop, and it also reaches
  console-style Linux handhelds such as the Steam Deck. Our own wrapper (`shell/sdl.rs`) holds every `unsafe` block
  of the program; no third-party SDL wrapper crate is used.
- **The frame is RGBA.** The interface and the flow's screens use their own colours (button fills, outlines) and grey
  fades that are not palette entries, so an indexed frame would change those pixels. Revisit after a spec read of
  `docs/specs/ui/*` gives the original's palette colours.
- **The core draws the pointer on every screen,** not only in a game, per `ui/screen-and-cursors.md` "Pointer shapes".
- **The mixer's output is 22,050 Hz stereo, 16-bit,** the rate of the music files (`ui/music.md`); the shell resamples
  if the device needs it.
- **Touchpad scrolling is faster than a wheel.** SDL reports a touchpad's scroll as fractions of a wheel step, which
  the shell passes on as steps. It matters only for the `--free-zoom` development aid; the shell would scale it if
  someone finds it too fast.
- **Gamepads translate, as touch does.** The pad's rules live in the core (`pad.rs`), so every port behaves the same; see [Gamepads](#gamepads) and `gamepad.md`.

## Core API

The host drives the loop. The core never blocks, never reads the clock and never touches a device.

```rust
// `core.rs`:
pub struct Core {
    pub input: Input,         // the shell puts the system's events in between frames
    pub audio: Audio,         // the mixer; `audio.output()` is what the device callback pulls (`Output::mix`)
    pub frame: Frame,         // the screen composed by the last `advance`
    pub cursor_visible: bool, // false: the screen draws its own pointer
    pub title: Option<String>,
    pub exit: bool,           // the program asked to end normally
    /* flow, game, interface, storage */
}

impl Core {
    /// Runs one frame, `elapsed` real time after the last; returns the screen to show.
    pub fn advance(&mut self, elapsed: Duration, platform: &mut dyn Platform) -> &Frame;
    /// "Saving": the settings, on a normal exit.
    pub fn store_settings(&self);
    /// The app goes to the background: pauses a running game, silences the mixer, writes the settings.
    pub fn suspend(&mut self);
    /// The app is back: restarts the mixer and lets go of held keys.
    pub fn resume(&mut self);
}

// `compose::Frame`: RGBA pixels with their own size (640 × 480, or wider in a game with `widescreen`).
pub struct Frame { pub rgba: Vec<u8>, pub width: usize, pub height: usize }
```

| Decision | Why |
|---|---|
| One call per frame, real time in | The game's cycle runs on a fixed step (`timing-and-rng.md`); the core keeps the accumulator, so every shell gets the same cycle timing. |
| A frame's game time is capped at 250 ms | A long frame (a debugger, a suspended laptop) runs at most 250 ms of game, and the game's steps run before the frame's systems. |
| `mix` takes `&self` | The audio callback runs on its own thread. The core sends sound starts to the mixer through a queue; the mixer owns its play positions. |

## Input model

The shell turns its events into one `Input` per frame, in screen pixels. Touch and gamepad rules live in the core, so
every shell behaves the same.

```rust
// `input.rs`, abridged:
pub struct Input {
    pub keys: Buttons<Key>,        // held keys, with this frame's edges
    pub mouse: Buttons<Button>,    // left, right, middle and the others
    pub typed: Log<KeyEvent>,      // key presses with the text they typed
    pub wheel: Log<Wheel>,         // wheel steps
    pub pointer: Option<(f32, f32)>, // window points
    pub window: Option<WindowGeom>,
    pub focused: bool,
    pub touches: Vec<Touch>,       // id, phase, window position (points)
    pub pads: Vec<PadEvent>,       // pads added and removed, their buttons and axes
    /* scroll, tap radius, mouse-as-finger events, dropped and picked files */
}
```

| Device | Maps to | Notes |
|---|---|---|
| Mouse | pointer, buttons, wheel | The mouse lock (`display`, "Mouse lock") stays in the shell. |
| Keyboard | key events with text | Text typed with a key feeds the save dialog and the chat line. |
| Touch | tap: left click; hold 450 ms: right click; drag: box select; two fingers: pan; pinch: zoom 1 to 4 times; three-finger tap: F10 | Our own rules (not part of the original); see "Touch" below and `touch.md`. |
| Gamepad | left stick: the pointer; south: left click; east: right click; right stick: scrolling; shoulders: Shift and Ctrl; Start: F10 | Our own rules; see [Gamepads](#gamepads) and `gamepad.md`. |

## Touch

`docs/design/touch.md` is the design; this is where it sits in the core. Touch only
*translates*: fingers become the mouse and keyboard input above, so the game's rules never see a finger and a run with
no touch is frame for frame what it was.

- **Input.** The shell (or a scripted scene, or the emulation) puts `Touch { id, phase, pos }` events in with
  `Input::touch`; `pos` is in the window's logical points, as the pointer is. `WindowGeom` carries what touch needs
  besides the size and scale: the safe area, the display's density (`ppi`, to turn millimetres into points) and whether
  the picture fits fractionally.
- **Where it runs.** `touch::step` runs in step 2 of a frame, after a scripted run's input and before the frame's events
  are applied. It does nothing until a finger appears or `--touch-emulation` is on. It lays the chrome out, feeds the
  fingers to the chrome (which takes the ones that land on a control) and the rest to the gesture machine, applies one
  of the machine's frames (buttons, keys, the tap-tolerance radius) and collects its pan and zoom. The pointer is set
  from the fingers after the platform's (`TouchState::place_pointer`).
- **What the game takes.** `game::touch_view`, with the game's other input systems, moves the map view by whole tiles
  for a pan, zooms it for a pinch and pauses for the chrome's pause button; `mouse_input` hands a tap's release to
  `mouse::tap_snap` first. Outside a game a pan becomes mouse-wheel lines for lists.
- **Drawing.** The long-press pulse is drawn into the 640 x 480 frame after it is composed (so it is in snapshots). The
  chrome is not: it is an RGBA overlay of the window's size (`TouchState::overlay`) that the shell blends over the
  picture, rebuilt only when `TouchState::revision` changes.
- **Scaling.** `WindowGeom::letterbox` fits the picture in the safe area, by a whole number or (`fit`: `auto`, `sharp`) by any
  fraction; the shell draws a fractional fit sharp bilinear (`display`, "Fractional scale").
- **The shell.** `shell/sdl.rs` reads finger events from screens the user touches directly, turns SDL's mouse-from-touch
  events off when it does, reports the safe area, and draws the overlay and the sharp-bilinear pass. With
  `--touch-emulation` the left mouse button and motion go to the core as `Input::mouse_touches` instead of as a button.

## Audio

The mixer (`audio.rs`, no engine types) decodes PCM WAV files (8 or 16 bits, mono or stereo, any rate, resampled once to
22,050 Hz by linear interpolation) and mixes its voices in whole numbers: a 16.16 gain each, the sum clipped to 16 bits.
The shell pulls it through `Output::mix` from the device's callback, so the mixer sits behind a lock.

| Part | How |
|---|---|
| Groups | A voice belongs to a group and stops with it: the music's track, a game's effects (stopped when the game ends), a screen's own sounds (stopped when the flow leaves the screen) and the act screen's sound (also stopped by its fade of all sound). |
| Effect levels | A sound effect's level is its gain (`level / 255`); the flow's sounds play at full gain, once. Overlapping sounds all play. |
| Music | `music.rs` fades in 28 steps over 540 ms, the first at once. The jukebox reads the mixer's play position, and the mixer loops a track from its loop point on the exact sample. |
| Video sound | A decoding thread feeds the mixer, starting where the picture is. |
| Scripted runs | No device: the recorder mixes each frame's share after the frame (367 or 368 sample frames at 60 frames a second, the fractions carried). |

The snapshot hashes sound *starts* (frame, a hash of the sound's bytes, loudness, mode) and the mixed samples too (the audio snapshot).

## Storage

Everything (`crates/client/src/storage.rs`) the core reads or writes outside itself goes through one
trait the shell provides:

```rust
pub trait Storage: Send + Sync {
    /// The installation's files, opened for reading with seeking (the MPQ reader seeks); `None` when there is none.
    fn install(&self, name: &str) -> io::Result<Option<Box<dyn ReadSeek>>>;
    /// A map the user named by its own path on the command line; `None` when there is no such file.
    fn user_map(&self, path: &str) -> io::Result<Option<Vec<u8>>>;
    fn maps(&self) -> Vec<String>;                                 // the installation's .pud maps, for the Custom Scenario list
    fn saves(&self) -> Vec<String>;                                // file names, in the folder's order
    fn read_save(&self, file: &str) -> io::Result<Vec<u8>>;
    fn write_save(&self, file: &str, bytes: &[u8]) -> Result<(), WriteError>; // whole, or no file
    fn delete_save(&self, file: &str) -> io::Result<()>;
    fn read_settings(&self) -> Option<Vec<u8>>;
    fn write_settings(&self, bytes: &[u8]) -> io::Result<()>;
    fn has_screenshot(&self, name: &str) -> bool;
    fn write_screenshot(&self, name: &str, bytes: &[u8]) -> io::Result<()>;
    fn write_replay(&self, name: &str, bytes: &[u8]) -> io::Result<()>;
    fn write_received_map(&self, name: &str, hash: u64, bytes: &[u8]) -> io::Result<String>; // a map sent by a host
    fn wall_clock_secs(&self) -> u64;                              // for seeds and save times
}
```

`storage::Desktop` is the desktop's: the installation folder or the imported copy, the save folder, the settings file and
the user folder, and the system clock. The methods take `&self` so that the core and the installation reader can share
one `Arc<dyn Storage>`; a platform that has to change state to read or write keeps it inside. `savegame` is free of
engine types: the shell gathers the running game's parts and `savegame::capture` encodes them. A scripted run's `Desktop`
has a fixed clock, and the snapshot hashes every file a scripted run writes (its saved games and screenshots).

## Lifecycle

| Event | Core does |
|---|---|
| `suspend()` | Pauses the game (as the pause key), stops the mixer, writes the settings. |
| `resume()` | Restarts the mixer. The game stays paused until the player resumes it. |
| Focus lost | `Input::focused = false`: the palette stops cycling (`palette::Gate`). |

Mobile shells call `suspend` when sent to the background and `resume` when they return ([ios.md](ios.md), [android.md](android.md)).

## Widescreen

The `widescreen` option (C+L, `docs/design/extras.md`; `widescreen.rs`) makes the game screen wider than 640 pixels.
The extra width is map. Off is the default and leaves every frame byte for byte what it was.

**Widths.** The screen stays 480 lines high, so the interface art keeps its pixel size and the map view keeps its
height. Every width is 640 plus whole tiles (32 pixels), so the view always shows whole tiles across, as the
original's 14 do and as the view position (a whole tile) assumes.

| Setting | Screen | Map view | Notes |
|---|---|---|---|
| `off` | 640 × 480 | 14 tiles | the original |
| `16:10` | 768 × 480 | 18 tiles | exactly 16:10 |
| `16:9` | 864 × 480 | 21 tiles | 1.8: 853.3 pixels, the original 16:9, rounded to the nearest whole tile |
| `auto` | 640 to 864 × 480 | 14 to 21 tiles | the whole tile nearest the window's safe-area shape, never wider than `16:9` (a window narrower than 4:3 gets 640) |

**Fairness.** The view only shows more map to the right: the height, the zoom and the interface's size are the
original's. That is still an edge, which is why the option is C+L: a session can override it for everyone (`game::extras`).
In a game with other players the host's lobby value is used for everyone (`extras.md`, "Multiplayer (C+L)").

**Where the width lives.** `Core::screen` is the screen's size this frame, set at the start of `advance`
(`core::screen_size`) and again after the flow's transitions, since a game starting or ending changes it. It is 640 × 480
unless the state is `Game` with a sim and the option says otherwise. The core stamps it into `WindowGeom::screen`
before anything reads the window, so `WindowGeom::letterbox` (and with it every pointer mapping) and the
shell's own letterbox agree. The shell reports the last frame's size in the same field and the core overwrites it. The
frame (`compose::Frame`) carries its `width` and `height`, `Clip::SCREEN` is "the whole frame, whatever its size", and
`compose_screen` centres the Interface and Flow planes on the screen's centre. The default path costs a size
comparison per frame.

**Layout.** `game::hud_layout` lays the interface out for `Core::screen` and again when it changes (`auto` and a resized
window). `hud::Layout::new` already took any canvas: the panel, minimap, information panel and command buttons stay
where they were against the left edge; the top and bottom bars and the map viewport widen. The top bar's resource
items, the status line and the idle-worker button keep their positions from the left (the resource line is not spread
across the wider bar, so a 16:9 game has the same resource line with more bar to its right); the message list and the
game clock keep their distance from the view's left, bottom and top edges; the control-group strip is anchored to the
view's top left. A view that grows or shrinks keeps its left edge (`hud_layout` moves its centre by half the change), so
a window resize does not jump the map.

**What maps through the width.**

| What | How it follows |
|---|---|
| Mouse to map | `Hud::cursor` and `MapView::pointer_map` go through `WindowGeom::letterbox().screen_point`, which clamps to the screen's own width |
| Selection boxes, building placement, the view rectangle | Use the layout's viewport and `MapView::geom`, so they are as wide as the view |
| Minimap's view rectangle and a press on it | `minimap::view_rect` / `view_for_press` take the view's size in tiles from `MapView::geom` (18 or 21 across) |
| Edge scrolling | `screen::edge` takes the canvas (`Layout::canvas`), so the right edge is the wide screen's |
| Keyboard scroll, the clamp to the map | `MapView::keep_on_map` uses the view's real size |
| Screenshots (PrintScreen) | `screenshot::write_shot` writes the frame at its own size |
| Menus, briefings, videos, the score screen | Are 640 × 480 pictures centred on the wide frame with black beside them; `frontend::pointer` uses `Letterbox::flow_of` to take the pointer back to their own pixels. Outside a game the screen is 640 × 480 anyway, so only the game's own dialogs (game menu, save and load, outcome) are ever centred in a wider frame |

**The window.** The window keeps whatever width a game gave it. When the screen widens and the window shows whole-number
scales, it grows to hold the screen at the scale it had (`fit_window`), if the display has room, and the smallest size
follows the screen; a window that fills at any size (`--extra-scaling sharp`, touch, or `auto` where the whole scale fits poorly, as in a
full screen) only changes its letterbox. After a
game ends the 640 screen sits in the middle of the wider window with black beside it.

**Touch.** On a touch device a game screen wider than 640 hides the touch buttons that sit in the pillarbox
(`touch::chrome_hidden`); the choice, and what is lost without them, is in `touch.md`, "Widescreen or the side
buttons". The gesture machine and the chrome's layout work in the letterbox's screen, whatever its width, so fingers map
correctly on it.

**Tests.** The width rules (`widescreen`'s tests), the letterbox with a wide screen (`display`), the clipping of a wide
frame (`compose`), the layout (`hud`), the view's pointer mapping (`mapview`), the minimap rectangle (`minimap`), the
right edge (`screen`) and the touch override (`touch`, `controls`) are unit tests. The frame snapshot has two scenes, `widescreen`
(16:9: the start, a selection box past the old right edge, the pointer scrolling at the new edge, the game menu in the
middle) and `widescreen-16x10`; no existing snapshot changed.

## Platform trait

The core sees only `Input`, `Storage`, the time it is given and, during a frame, `core::Platform`:

```rust
pub trait Platform {
    /// The window: its size in physical pixels and its scale, `None` without one.
    fn window(&self) -> Option<WindowGeom>;
    /// The pointer in the window's logical points and whether the window has the focus, once a frame after the keys
    /// are applied. `game_pointer`: the game draws its own pointer, so the platform may lock the system's.
    fn pointer(&mut self, keys: &Buttons<Key>, game_pointer: bool) -> (Option<(f32, f32)>, bool);
    /// A run with no real window (the headless scripted loop): the window the core sees is exactly the screen's size.
    fn headless(&self) -> bool { false }
}
```

A shell does six things around `Core::advance`:

| Operation | SDL 3 shell (`crates/client/src/shell/app.rs`) | What another shell writes |
|---|---|---|
| Input events | `Shell::take_events`: key (with its typed text), button, wheel and focus-lost events into `Core::input` between frames | the same calls: `Input::key`, `button`, `wheel`, `focus_lost`; `key_of` is the key map |
| Frame | `core.advance(elapsed, &mut shell)` once a frame, the real time since the last | the same |
| Window and pointer | `impl Platform for Shell`; the mouse lock (`display::lock_wanted`) inside `pointer`: relative mode on macOS, the grab elsewhere | `impl Platform` |
| Show | the frame into one streaming texture, drawn at the whole-number scale (`display::Letterbox`) on black; `cursor_visible`, `title` after the frame | upload `Core::frame` (RGBA, its own `width` × `height`: 640 × 480 or wider) and scale it |
| Sound | an SDL audio stream whose callback calls `Output::mix` for what the device needs; SDL converts the rate | the device callback calls `Output::mix` (22,050 Hz stereo) |
| Files, exit | `storage::Desktop` (`host::launch`); `store_settings` on `Core::exit` or the window's close; then the loop ends | the same `Desktop`; end the loop on `exit` |

The frame's size can change between frames (a game with `widescreen` starts or ends): the shell compares
`Core::frame`'s size with the last and remakes its texture (`Renderer::set_size`); see [Widescreen](#widescreen).

A scripted run needs one more call after each frame: `script::record(run, frame, output, exiting)`, which also says
when the run is done; with a window, `script::compare_shot` takes the window's own picture of each shot.

## Scripted scenes and snapshots

Scripted scenes run the core without a person: a fixed frame time (1/60 s), a fixed wall clock, the scene's input in
place of the system's and no sound device (`--script NAME --script-out DIR [--headless]`, `script.rs`). They pin down
the frame and sound behaviour.

| Part | What it does | Where |
|---|---|---|
| Compositor | Places pictures on the screen: nearest texel at pixel centres, transparent texels skipped, grey tint in linear light, fades applied exactly on the CPU. No engine types. | `crates/client/src/compose.rs` |
| Scripted scenes | Feed the scene's input into the core's `Input` (`input.rs`). | `script.rs` |
| Frame snapshot | Hashes every frame (runs of equal frames on one line), every shot and every sound start, without a window. Skips without `TIDEWATER_INSTALL`; `TIDEWATER_SNAPSHOT_BLESS=1` blesses. | `tests/frames.rs`, `tests/snapshots/` |
| Audio snapshot | A fingerprint of the mixed sound per second. | `tests/snapshots/NAME-audio.txt` |
| Window comparison | Ignored test: runs the scenes in the SDL window at one pixel per screen pixel and compares the window's own pixels (read back before each shot is shown) with the composed frame at each shot. It opens windows on top for about a minute and a half. | `tests/frames.rs` (`-- --ignored`) |

Run the frame snapshot with `cargo test --release -p tidewater-client --test frames` and `TIDEWATER_INSTALL` set. An
existing scene's snapshot changes only with the reason in the commit that re-blesses it.

## The SDL 3 shell

The `tidewater` binary is the core in an SDL 3 window. SDL 3 is built from its source (`sdl3-sys` with
`build-from-source`, `link-static`, `no-default-subsystems`, `sdl-audio`, `sdl-render`) and linked into the binary, so no
SDL install is needed.

| Part | Where | Why |
|---|---|---|
| The core | `crates/client`, the library (`lib.rs`) | It forbids `unsafe` code itself (`#![forbid(unsafe_code)]`). |
| The shell | `crates/client`, the `tidewater` binary: `src/shell/main.rs` (its `main`), `src/shell/app.rs` (the loop, input, mouse lock, sound) and `src/shell/sdl.rs` (our thin safe wrapper over `sdl3-sys`, every `unsafe` block of the program) | `tidewater-ios` and `tidewater-android` compile `app.rs` and `sdl.rs` too, so desktop and phones run one code path. |
| Shared start-up | `crates/client/src/host.rs` | Start-up from the command line (`host::launch`), the headless scripted loop, the mouse lock's pointer (`LockedPointer`), frame statistics (`FrameStats`, reported with `TIDEWATER_FRAME_STATS=1`). |

**Why the shell lives in the client crate.** The frame snapshot starts `CARGO_BIN_EXE_tidewater`, which Cargo gives only
to tests of the package that has the binary; one shell needs no crate of its own; and the library forbids `unsafe` while
only `shell/sdl.rs` allows it (the crate's lint is `deny` so that one module can). The cost is that building the client
library also builds SDL. If a port ever needs the core without SDL, the shell moves into its own crate then.

### Gamepads

`gamepad.md` is the design. The build turns on `sdl-joystick` and `sdl-hidapi` in `sdl3-sys`'s features (the
workspace `Cargo.toml`). `Sdl::init` adds `SDL_INIT_GAMEPAD`; `poll` opens a pad on `SDL_EVENT_GAMEPAD_ADDED`, closes
it on `..._REMOVED` and returns its button and axis events, which `take_events` hands to the core as `PadEvent`s
(`Input::pads`). The core's pad step (`pad::step`) runs first in the frame's input step and turns them into the
pointer, buttons and keys; after `Core::advance` the shell moves the system pointer to the pad's
(`PadState::take_warp`), or the mouse lock's pointer while the lock holds it.

## Time and the flow's states

**Time.** The core's clock (`clock.rs`) takes each frame's real time from the shell. Whole steps of the capped frame time
run the game's cycles in `Core::advance`, before the frame's systems; the step is 64 a second until a game sets its own.

**One order.** `Core::advance` calls the frame's steps and `core::frame_systems` calls the systems one after another, with
the points where their deferred changes (`core::Commands`) take effect written in it. The order matters (for example
music starts before a screen's cue, and a game's outcome is watched before its dialog is drawn), so change it only with
the snapshots in view.

**The flow's states** are `states.rs` (no engine types): the screen that shows, the change asked for (the last ask in a
frame wins; asking for the screen that shows leaves and enters it again), and the hooks each screen runs when it is left
and entered, as data (`states::on_exit`, `states::on_enter`). A change takes effect between the frame's input and its
game cycles, at most one a frame:

1. The state becomes the new one; the leaving hooks already see it.
2. The leaving screen's hooks run. Leaving a game: `camera_clear`, `close_dialog`, the game menu's close, `end_game`.
3. The leaving screen's layers and sounds (`frontend::OwnedBy`) go, the last made first.
4. The entering screen's hooks run. Entering a game: `stop_music`, `camera_clear`, `start_game`, `setup`.

Each hook is a plain function (`core::run_hook`), and its deferred changes take effect as it ends.

## Known limits

| What | Why |
|---|---|
| `--zoom` above 1 in a big window | The map is composed at screen resolution, so zooming out at 3× shows no extra detail. Development aid only. |
| A hook or system that cannot run | The plain functions return early on a missing part and log nothing. |
| Memory on handhelds | The core uses about 145 MB in a mission on a desktop. The Vita build asks for extended memory ([vita.md](vita.md)); check any other handheld against its memory. |
| A full 640 × 480 RGBA composition per frame on handhelds | On a desktop the whole frame, upload and draw take 2.4 to 2.6 ms; a slower CPU may need changed-layer composition or an indexed frame (the Vita is expected to hold about 30 fps, see [vita.md](vita.md)). Measure on the device. |
