# Touch input

Touch is **our own addition**: the original game has no touch input. The rule behind all of it is that touch only
*translates*: fingers become the mouse and keyboard events the game already understands, so no rule of the game
changes, Enhanced stays the default, and with no touch input every frame, sound and oracle trace is exactly what it
was. This document covers the input layer and desktop emulation; the [iOS](ios.md) and [Android](android.md) shells
turn touch on from the first frame.

## Where it lives

| Module | Job |
|---|---|
| `input` | `Touch { id, phase, pos }` (window points) and `WindowGeom`, which now also carries the safe area, the density and whether the fit is fractional. |
| `gesture` | The state machine: fingers in, mouse frames and view intents out. No clock, no engine. |
| `chrome` | The on-screen controls: layout in the pillarbox, hit tests, the overlay picture. |
| `touchemu` | The mouse as fingers for `--touch-emulation`. |
| `touch` | The core's glue (`Core::touch`): runs the above in the frame's input step. |
| `display` | `Letterbox::fit_area`: the fractional "sharp bilinear" fit inside the safe area. |
| `mouse` | `tap_snap`: the tap tolerance. |
| `shell/sdl.rs`, `shell/app.rs` | SDL finger events, the safe area, the overlay texture, the sharp-bilinear pass, the soft keyboard. |

## Gestures

| Fingers | Gesture | Becomes |
|---|---|---|
| 1 | **Tap**: lift within 450 ms, moved under 3 mm | Left press, then release a frame later; with your units selected, a right click instead off your own units (the smart tap) |
| 1 | **Two taps** on one spot (within 450 ms and 6 mm) | Two clicks at exactly the first tap's point, so the game sees its double click (all of that type on screen, Enhanced) |
| 1 | **Drag** on the map view: moves past 3 mm within 300 ms | **Pan** of the map view, gliding on after a flick |
| 1 | **Hold, then drag** on the map view: 300 ms still (a ring shows), then moves | Left button held from where the finger went down: the selection box |
| 1 | **Drag** anywhere else (minimap, panel, menus) | Left button held from where the finger went down: minimap drag, sliders, lists |
| 1 | **Long press**: 450 ms, moved under 3 mm, then lift | Right click where the finger lifts; a pulse ring at 450 ms says it is ready |
| 2 | Move together | **Pan** of the map view (lists scroll as a wheel outside a game) |
| 2 | Spread or pinch | **Zoom** of the map view, from the original's view up to 4 times that |
| 3 | **Tap**: all up within 450 ms, none moved | F10, the game menu |

**One finger decides late.** A finger down does nothing until it moves past the slop, lifts or reaches 450 ms, so a
second finger can still turn it into a pan or pinch without a stray click. A second finger while one is undecided or
panning cancels it silently; while a drag holds the left button it releases it where the finger is. When one of
several fingers lifts, the rest are ignored until all are up. A cancelled finger releases what it held and nothing
more.

**Pan or box.** Scrolling is what a finger on the map does most, so a plain drag pans, and the selection box needs a
short hold first: still for 300 ms, the finger arms (the same ring as a long press, smaller as it starts), and a drag
after that is the left button's. Kept still to 450 ms it also becomes the long press, but nothing is clicked
while the finger stays down: lifting it there right-clicks, and a drag (however long the hold) still draws the box.
An earlier version right-clicked at 450 ms with the finger down, so a hold held a moment too long before the drag
sent the selection away. Outside the map view (the
minimap, the panel, the menus) a drag is the left button's at once, as before. A flick glides on and slows (a time
constant of 325 ms, from 30 mm/s down to 8 mm/s); any new finger stops it.

**Placing and targeting.** While a building is being placed or a command's target chosen, a finger on the map moves
the pointer instead of clicking, so the outline follows the finger and stays where it lifts. A tap elsewhere moves
the outline there; a tap on the outline (or within 8 mm of the pointer, for a target) confirms with a left click at
the pointer, never under the finger, so the building goes where the outline showed. A finger held within 8 mm of the
map view's edge scrolls the map (60 mm/s) and does not long-press; a long press elsewhere is the right click that
cancels, sent when the finger lifts (as on the map, a drag after the hold carries the outline instead). Because a tap away from the outline only moves it, the status bar says so while touch is in use ("Tap the
outline to build; drag to move it", and "Tap a target, then tap it again to confirm") in place of the game's own
Select Location and Select Target prompts. A tap within 6 mm of a confirming tap and 0.45 s of it (the double-tap
limits) is dropped, so a double tap on the outline confirms once: the second tap would otherwise arrive after
placement ended, and with the builder still selected the smart tap made it a move to the site, which replaced the
build order (the builder walked there and stood).

**Mouse frames.** The core's mouse model drops a release that arrives in the same frame as its press, so the machine
emits one frame at a time (`Gesture::pop`) and the core applies one per game frame: press, then release, then any
pointer move after a press. The pointer exists only while a click or drag is in progress, or while placing or
targeting; otherwise a lifted finger leaves no cursor to scroll the map's edge.

**The smart tap.** With a unit of yours that can move selected, a tap on the map view that is not for one of your own
units is a right click, press and release: ground moves there, an enemy is attacked, a mine or a forest is worked, as
the right-click rules decide (`mouse::smart_tap`). The tap is for the unit whose box it lands on (yours first where
boxes overlap); off every box, for the nearest unit within the 6 mm tap tolerance, so an enemy fighting beside your
units is attacked, not selected. A wall is terrain, not a unit, so a tap on a wall tile (off every box) counts as a
target too, and the right click attacks it as a mouse's would. A tap on your own unit or building still selects it, and so does any tap with nothing of yours selected (an enemy you are looking at gives no
orders). The **plain-click** button (the pointer with a click mark) makes the next tap on the map an ordinary left click, for the rare case the smart
tap guesses wrong; the option `touch-smart-tap` turns it off.

**Sizes in millimetres.** Slop, pan and pinch thresholds, the tap tolerance and the chrome's buttons are millimetres,
turned into points with the display's density. The shell passes the best density it knows (`WindowGeom::ppi`: 140
points to the inch on a desktop, 160 on a phone or tablet, or `--touch-dpi`).

## Tap tolerance

A finger is wider than a unit. A tap on the map that lands on no unit's selection box, but within **6 mm** of one,
selects the nearest unit the local player can pick (`mouse::tap_snap`). The tap is moved to that unit's centre and the
ordinary release rules (`mouse::release`) run on it unchanged, so selection rules, double-click and modifiers all
behave as for a mouse. This is client-only: it changes which point a Select is sent for, nothing in the sim. A tap on a
box, a drag or a mouse click never snaps.

## The chrome

A touchscreen has no Shift, Ctrl or Alt, no pause key and no number keys. The chrome puts them in the black bars
beside the picture (in the safe area). The left bar holds the tools, the right bar the control groups (the option
`touch-swap-bars` swaps them):

| Control | Icon | Does |
|---|---|---|
| Menu | Three lines | F10: the game menu. |
| Pause | Two bars; a play triangle while the game is paused | Pauses or resumes, as the pause key does. |
| Shift, Ctrl, Alt | ⇧, ⌃, ⌥ | A one-shot latch: the key is held for the next click, then let go a frame after that click's release (the rules that read Alt when the button comes up need it held through the release). A second tap within 450 ms of the one that turned it on **locks** it: the key stays held through any number of clicks until one more tap lets it go. A tap after 450 ms, or after a click, turns a one-shot latch off as before. |
| Plain left click | A pointer with a click mark | The next tap on the map is a left click, whatever the smart tap would make of it. |
| Keyboard | A keyboard | Opens or closes the system's soft keyboard (`SDL_StartTextInput`). When the system hides it by itself (its own hide key, Android's Back), SDL's `SDL_EVENT_SCREEN_KEYBOARD_HIDDEN` turns the button off again (checked against `SDL_ScreenKeyboardShown`, so a hide followed at once by a show changes nothing), and the shell closes its text input to match. |
| Groups | The digits 1 to 9 | A tap is the digit: recalls the group, and two taps centre on it, as the keys do. Held 450 ms it is Ctrl and the digit: the selection becomes that group (once, not again at the lift). |
| Idle worker | A pickaxe with a "z" | F6: the next idle worker. Only with the option `idle-worker` on; it sends F6 whatever key that option is bound to. |

How a button looks says what it will do (`chrome::Look`):

| Look | Means |
|---|---|
| Dark, white icon | Off. |
| Dark, yellow outline and icon | Waiting for one click: a one-shot latch, or the plain click. |
| Filled yellow, dark icon | On: the keyboard open, a button under a finger. |
| Filled yellow, dark icon and a lock in the corner | A locked latch: held until tapped again. |

**Picking units one by one.** The lock is for hand-picking units from a mixed group: double-tap Shift, then tap
units to add them to the selection or take them out of it, and tap Shift once when done. It is only Shift held, as
on a keyboard, so no rule changes. Losing the keyboard (the app going to the background) lets every latch go, locked
or not, since the game lets go of every key then.

**The icons** are our own SVGs, white strokes on a 24 x 24 grid with round caps and joins (2 units wide, the digits
2.6), in `crates/client/assets/touch-icons/`; edit them there. No third-party icon set is used. The chrome
rasterises them with [resvg](https://github.com/linebender/resvg) (pure Rust, Apache-2.0 OR MIT; its renderer
`tiny-skia` is BSD-3-Clause, listed in `THIRD_PARTY_NOTICES.md`) at each button's size in physical pixels whenever
the overlay is rebuilt, so they are crisp at any density and button size, anti-aliased, and nothing is prescaled.
That was simpler than committing pre-rendered masks plus a tool to regenerate them, and it builds for every target
(no C code, no system libraries); the SVGs are compiled in (`include_str!`), and resvg's text, font and raster-image
features are off.

Outside a game only the keyboard button shows. A bar shows at least six group buttons (up to nine as the bar's
height allows), and the buttons are laid out in a grid of 9 mm squares (7 or 11 with `touch-buttons`), shrunk to fit
a narrow bar but never under 30 points; if only one bar is wide enough it takes every control, and a window with none
gets the two lists over the picture's left and right edges. A control acts when its finger lifts inside it, except a
held group button, which acts at 450 ms. The chrome is an
RGBA overlay of the window's size that the shell blends over the picture; it is never part of the frame (640 x 480; the game screen is never wider on a touch device, because the `widescreen`
option is ignored there: the pillarbox is where the chrome lives, see `client-core.md`, "Widescreen"), so frames and
snapshots do not depend on it.

## Scaling

`Letterbox::fit_with(size, safe, fit, screen)` fits the picture in the window's **safe area** (clear of notches,
rounded corners and the home bar). In the fractional mode it uses any scale, down to a quarter, and the shell draws it
**sharp bilinear**: the picture is first scaled up by the next whole number with the nearest texel, then scaled down
to the exact size with a linear filter, so pixels stay crisp without shimmering at a fractional scale. A fit that is
a whole number is drawn directly. Touch platforms (iOS, Android) always fit fractionally. On a desktop the client-only
extra `scaling` (values `auto`, the default, `integer` and `sharp`) decides: `auto` keeps a whole number that fills the
area well and otherwise fits fractionally; see `docs/design/extras.md`.

## Zoom and pan of the map view

The view keeps its top-left corner on a whole tile (screen spec, "Layout"), so a pan moves it a tile at a time and
carries the remainder to the next frame. A pinch changes the view's zoom (`MapView::zoom`, map pixels per screen
pixel) between 1 (the original) and 0.25 (4 times magnification), snapping to 1 when close, and re-centres to keep
the map point between the fingers under them; the whole-tile rule leaves it off by up to half a tile at the highest
zoom. Pinch zoom is separate from `--free-zoom`, which stays a wheel-driven development option.

## Touch emulation (`--touch-emulation`)

The mouse acts as one finger, so every path above can be tried without a touchscreen, and the chrome shows.

| Mouse | Fingers |
|---|---|
| Left button down, drag, up | One finger |
| Alt held at the press | A second finger mirrored through the picture's centre (as in the iOS Simulator): drag outward to pinch, sideways to pan |
| Alt and Shift | The second finger moves beside the first: a two-finger pan |
| Alt and Ctrl | A third finger rests at the centre: let go for a three-finger tap |

The modifiers are read when the button goes down. The right and middle buttons and the wheel work as usual, Alt is
not passed to the game, and the mouse lock is off so dragging is not fought.

## Real touchscreens

Fingers on a screen the user touches directly arrive as SDL finger events (`SDL_EVENT_FINGER_*`, positions as a
fraction of the window, scaled to points). SDL's own mouse-from-touch and touch-from-mouse events are turned off
(`SDL_HINT_TOUCH_MOUSE_EVENTS` and `SDL_HINT_MOUSE_TOUCH_EVENTS` to 0) only when emulating or when a touchscreen is
attached, so a desktop mouse is unchanged. A trackpad's fingers (an *indirect* device) are not read as touches: they
are pointer input, and its scroll gestures keep working as the wheel. Touch mode ends when a real mouse moves with no
finger down.

## Options

The Settings screen has a **Touch** page wherever touch is in use (a phone or tablet, `--touch-emulation`, or a
computer whose touchscreen was touched); on a phone or tablet it takes the place of the Keys page. Its rows are the
client-only options of `docs/design/extras.md` whose names start with `touch-`:

| Option | Default | Choices |
|---|---|---|
| `touch-drag` | `pan` | One finger pans and glides; `pan-no-glide`; `box`: a drag draws the box at once, as the mouse does |
| `touch-smart-tap` | `on` | `off`: every tap is a left click |
| `touch-two-finger` | `pan` | `box`: the two fingers are the box's corners; `off` |
| `touch-pinch` | `auto` | `auto` is on for a phone (the screen's short side under 90 mm) and off for a tablet; `on`; `off` |
| `touch-hold` | `normal` | The arm and long-press times: 300 and 450 ms; `long` 400 and 600; `longer` 500 and 750 |
| `touch-buttons` | `medium` | The chrome's buttons: 9 mm; `small` 7; `large` 11 |
| `touch-swap-bars` | off | The groups go left and the tools right |
| `touch-menu-tap` | `on` | `off`: a three-finger tap does nothing (the Menu button still opens the menu) |

The phone rule reads the window's size and the shell's density (`touch::is_phone`), so a desktop window under
`--touch-emulation` counts as a phone when it is small (640 x 480 at 140 points to the inch is 87 mm high).

## Help

The **Touch Controls** help (`touchhelp`) sums up the gestures and the side bars on two pages in a panel that fills
most of the 640 x 480 frame: **On the map**, the gestures in two columns (the gesture in yellow, what it does in
white), and **Buttons**, each control drawn as the bars draw it (`chrome::button_picture`, the same icons and looks,
the one-shot and locked Shift too) beside its name and meaning, then how to pick units one by one with a locked
Shift. Next, Back and OK under the pages are 44 pixels high and take taps 14 pixels round them, a 72-pixel target:
about 9 mm on a phone in landscape (393 points high), more on a tablet. Enter is Next, then OK; the arrow keys turn
the pages and Esc closes them. Every line is one line in `font12x`: a test (`touchhelp`,
`every_line_fits_its_column_in_the_real_font`, run with `TIDEWATER_INSTALL`) measures each with the install's font
against its column and the pages' height against the buttons.

A phone or tablet shows the help by itself the first time a game starts (not a network game or a replay), once ever:
the stored settings remember it with the `touch_help_seen` line (`prefs`). After that it is a button under **Help** in
the game menu (`Screen::TouchHelp`, wherever touch is in use) and the **Touch controls** link on the main menu, above
the Settings link, shown while touch is in use; there the pages cover the darkened menu until OK or Esc. Both use one
`touchhelp::Pager` for the page and its buttons.

## Exit Program on phones and tablets

iOS and Android close apps themselves, and an app that quits on its own looks like a crash there. On a touch-only
platform, Exit Program is greyed out on the main menu (its `x` key does nothing either), it is not in the game
menu's End Scenario dialog, and Ctrl+X does nothing. Android's Back button is Esc (SDL's `AC_BACK` key, with
`SDL_HINT_ANDROID_TRAP_BACK_BUTTON` set so Back no longer closes the activity).

## Scripted scenes

`script::Cmd::Finger(id, phase, x, y)` puts a finger event in at a screen pixel. The scenes `touch-select` (tap near a peasant, then a smart tap
on the ground), `touch-order` (long press orders it), `touch-drag` (hold, then box select), `touch-pinch`, `touch-pan`,
`touch-swipe` (one-finger pan and glide), `touch-help` (a phone's first game shows the help, and the Help menu offers
it), `touch-help-menu` (the main menu's link and page) and `touch-menu` (three-finger tap) run on the default density
and have their own snapshots (`tests/snapshots/touch-*.txt`); `script::Cmd::TouchDevice` makes a run a phone's. To look
at the chrome and the fractional fit, run a scene in a larger window:

```
TIDEWATER_SCRIPT_WINDOW=1000x600 tidewater --script touch-order --script-out DIR --no-mouse-lock [--extra-scaling sharp]
```

It also writes `NAME-full.png`, the whole window with the chrome.

## Deviations from the original

All of touch is ours: the original has no touch, so there is nothing to be faithful to except the game's own rules,
which touch leaves alone. The choices to review are the 300 ms, 450 ms and 3 mm thresholds, the glide, the 6 mm tap
tolerance, the smart tap, the placement rules, the chrome's controls and layout, the pinch range (1 to 4 times, never zooming out
past the original's view), the missing Exit Program on phones and tablets, and ignoring a trackpad's fingers.
