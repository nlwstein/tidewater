# Gamepad input

Gamepads are **our own addition**: the original game has no controller support. As with [touch](touch.md), the pad
only *translates*: its sticks and buttons become the pointer, mouse buttons and keys the game already understands, so
no rule of the game changes, and with no pad connected every frame, sound and oracle trace is exactly what it was. It
is aimed at handhelds (the Steam Deck, the Vita) and at a controller on a desktop, alone or with the mouse and a
touchscreen.

## Where it lives

| Module | Job |
|---|---|
| `input` | `PadEvent` (added, removed, a button, an axis), `PadButton` (named by place: south, east, west, north), `PadAxis`. |
| `pad` | The core's pad step (`Core::pad`): the pointer, the button table, panel mode, the minimap jump, the held keys. |
| `game` | `pad_aim`: what the pointer is over (the minimap, the map view, the command buttons shown, a unit), for friction and panel mode. |
| `extras` | The five `pad-*` options and `PadSettings`. |
| `settings` | The Settings screen's Controller page, shown once a pad has been connected. |
| `controls` | The `controls` option's modes and the rule that resolves them (see "Control modes"). |
| `touch` | Drops fingers in the modes without them; hides the side bars in the Controller and touch mode while a pad is connected (option `pad-hide-touch`). |
| `shell/sdl.rs`, `shell/app.rs` | `SDL_INIT_GAMEPAD`, opening and closing pads, their events; moving the system pointer to the pad's. |

SDL's gamepad layer does the device work: it maps each pad's buttons to one layout (with HIDAPI drivers for the
common pads), so the core only sees buttons by place. The build turns on `sdl-joystick` and `sdl-hidapi` in
`sdl3-sys`'s features.

**The frame's order.** The pad step runs first in the frame's input step, then the touch step, then the frame's events
are applied (`Input::begin_frame`), then the platform's pointer, the touch pointer and last the pad's
(`PadState::place_pointer`). A pad event that comes in sets `PadState::seen`; until then the step returns at once.

## The pointer

The left stick moves the pointer in screen pixels a second: 640 at a full push (`pad::SPEED`), times the
`pad-speed` choice (slow 0.65, fast 1.5), twice that with the left trigger held. A push inside the dead zone
(`pad-dead-zone`: 0.15 of the stick's travel, small 0.08, large 0.25) counts as none; past it the push is rescaled
to 0 to 1. With `pad-acceleration` on, the speed follows the square of the push, so a small push aims, and over a
unit or a command button the pointer slows to 0.45 of its speed (`pad::FRICTION`) so it is easy to stop on one.

**Who has the pointer.** The last device used. Any pad button, a stick push or a trigger wakes the pad, which starts
from where the pointer last was: the mouse or a finger puts it somewhere and the stick moves it on. A mouse that moves
the system pointer more than 1.5 window points from where the pad left it (`pad::TAKEOVER_PT`) takes it back; so does
a finger. While the pad has it, the shell moves the system pointer to the pad's (`PadState::take_warp`), or the mouse
lock's pointer when the lock holds it, so the two never jump apart and the menus' system cursor follows the stick.

## Default mapping

SDL names a pad's buttons by where they sit, not by their labels: **south** is the bottom face button, **east** the
right one, **west** the left one and **north** the top one. So one mapping covers every pad, and the printed labels
differ only by maker:

| Place | Xbox, Steam Deck | PlayStation, PS Vita | Nintendo Switch |
|---|---|---|---|
| South | A | Cross (✕) | B |
| East | B | Circle (○) | A |
| West | X | Square (□) | Y |
| North | Y | Triangle (△) | X |
| L1 / R1 | LB / RB | L1 / R1 (Vita: L / R) | L / R |
| L2 / R2 | LT / RT | L2 / R2 (none on the Vita) | ZL / ZR |
| Start | Menu (☰) | Options (Vita: Start) | + |
| Back | View (⧉) | Create (Vita: Select) | − |
| Stick presses | LS / RS | L3 / R3 (none on the Vita) | left / right stick press |

What each one does, with the options at their defaults:

| Pad | In a game | In the menus and dialogs |
|---|---|---|
| Left stick | moves the pointer; a small push aims slowly, a full push is fast | the same |
| South | left click; held while the stick moves, a selection box | presses the focused item (Enter) |
| East | right click (move, attack, harvest); in panel mode, back to the map | back (Esc) |
| West | panel mode (below) | - |
| North | the pointer to the minimap, and back | - |
| L1 | Shift, held (Shift-click adds to the selection or removes from it) | Shift, held |
| R1 | Ctrl, held (Ctrl-click takes every unit of that type on screen) | Ctrl, held |
| L2 | the pointer twice as fast, held | the same |
| R2 | Alt, held | Alt, held |
| D-pad | control groups 1 to 4 (up, right, down, left): a press recalls the group, twice quickly centres on it; with R1 held it sets the group, with R2 held it centres and recalls | arrow keys; up and down also move the focus ring (see below) |
| Right stick | scrolls the map (past half a push) | the mouse wheel (lists) |
| Start | the game menu (F10); while a menu is open, Esc | Enter |
| Back | pause and resume, as the Pause button | Esc |
| Left stick press | the system keyboard, for chat and save names | the same |
| Right stick press | the next idle worker (F6, option `idle-worker`) | - |

Buttons not listed (Guide, the DualSense touchpad's click, the extra paddles) do nothing. The control groups are
the Enhanced rules' (the default; Classic is opt-in and multiplayer only, and has no digit groups).

A button's meaning is fixed when it goes down (`pad::bind`), so its release always lets go of what its press held.

**Nintendo's layout.** On a Switch pad south is B, so B clicks and A right-clicks. Option `pad-swap` swaps them, making
A (east) the click and B (south) the right click, as Nintendo's own games do.

**The PS Vita** has no L2, R2 or stick presses, so the faster pointer, Alt, the system keyboard and the idle-worker
button are not on its pad (the touch screen and the Pause and Menu buttons still work).

**The d-pad in the menus.** Up and down move the focus ring, as Shift+Tab and Tab do, and confirm (south, or east with
`pad-swap`) or Start presses the focused item: confirm sends Enter rather than a click at the pointer, and back sends
Esc. The pad's focus also stops on the main menu's links (Settings, Touch Controls), which the keyboard's Tab passes
over because F2 opens Settings, and its confirm toggles a game dialog's focused check box or radio button, which Enter
leaves alone (`Input::pad_confirm`). The d-pad still acts as arrow keys too, so lists, pop-ups and the Settings screen
move as with the keyboard; where a screen gives the arrows a meaning of their own, the pad's up and down move the focus
instead: on the Custom Scenario screen left and right change the focused option, and in the multiplayer menus and the
lobby the focus moves except in the game list and an open pop-up. A keyboard's arrow keys are unchanged
(`Input::pad_focus` is set only by the pad).

**Panel mode.** West puts the pointer on the first command button shown; the d-pad steps between the buttons (by
their grid, skipping empty slots), south presses one, and east or west goes back to where the pointer was on the map.
Choosing a command that needs a target ends panel mode, so the stick aims at once. A push of the stick also ends it.

**The minimap.** North puts the pointer on the minimap (at its centre the first time, then where it last was there),
where south moves the view as a click does; north again puts it back on the map where it was.

## Mixed with the mouse and touch

Mouse, touch and pad work together; nothing has to be switched. The shoulder buttons are real Shift and Ctrl, so
holding L1 while clicking the mouse or tapping adds to the selection. While a pad is connected the touch side bars
hide (`pad-hide-touch`, on by default), since the pad has every button they hold; taps on the map still work, and the
bars come back when the last pad goes. The `controls` option below can turn a device off instead.

## Control modes

The `controls` option (Settings, General page, **Controls**) picks which input devices drive the game. The keyboard
and the mouse's own buttons work in every mode; only the pad and fingers can be left out. **Automatic is the
default and behaves as everything above says**: it uses every device that is there.

| Value | Settings word | Pad | Fingers | Touch side bars | Offered when |
|---|---|---|---|---|---|
| `auto` | Automatic | as resolved below | | | always |
| `mouse` | Keyboard and mouse | ignored | ignored | hidden | not a phone or tablet, or a real mouse has been used this run |
| `touch` | Touch | ignored | work | shown | a phone or tablet, `--touch-emulation`, or a finger has touched this run |
| `pad` | Controller | works | ignored | hidden | a pad is connected now |
| `pad-touch` | Controller and touch | works | work | hidden unless `pad-hide-touch` is off | a pad is connected now and touch is there |

**Resolution.** `controls::Devices::resolve` gives the mode in force: the stored choice if its devices are there now,
otherwise Automatic. Automatic picks the controller and touch together when both are there, the controller alone,
touch alone, or the keyboard and mouse. The Settings screen offers only the values in the last column; a stored value
whose device is gone still shows as itself, and its description says Automatic is in use until the device is back.

**Why Automatic changes nothing.** It leaves out only a device that is not there, so there is nothing to drop. The
first finger of a run counts as touch being there before the modes are read (`controls::note_fingers`), so it is
never dropped.

**What ignoring means.** A pad left out still has its connections noted, but its buttons and sticks are dropped, and
whatever it held lets go as when it is unplugged. Fingers left out are dropped before the side bars and the gesture
machine see them (`touch::park`); the left button, latched Shift, Ctrl and Alt, and queued keys let go, and touch
that was in use comes back when fingers are on again. With `--touch-emulation` the shell sends the mouse's buttons
as plain mouse buttons in a mode without fingers (`Core::fingers_enabled`).

**Widescreen.** The `widescreen` option is ignored only while the side bars show (the Touch mode, or Controller and
touch with `pad-hide-touch` off, on a touch device). With the bars hidden the pillarbox is empty, so on a handheld
with a built-in pad Automatic allows widescreen; with no pad it is as before.

## Options

The five `pad-*` options are client options (`C`: they change nothing for other players) on the Settings screen's Controller page,
which shows once a pad has been connected this run:

| Option | Choices | Default |
|---|---|---|
| `pad-speed` | normal, slow, fast | normal |
| `pad-acceleration` | on, off | on |
| `pad-dead-zone` | normal, small, large | normal |
| `pad-swap` | on, off | off |
| `pad-hide-touch` | on, off | on |

`pad-hide-touch` applies in the Controller and touch mode. `controls` (auto, mouse, touch, pad, pad-touch; default
auto) is on the General page and is described under "Control modes".

## Testing

- **Unit tests** in `pad.rs`: the dead zone, the speed curve and friction, the button table, panel steps and the
  pointer's hand-over between the pad and the mouse.
- **The shell** (`shell/app.rs`): `a_virtual_pad_reaches_the_shell_through_sdl` attaches an SDL virtual gamepad,
  presses a button and moves a stick, and checks the events that reach the shell.
- **Scripted scenes** (the frame snapshot, `tests/frames.rs`): `pad-only` plays with the pad alone (aim, select,
  order, panel mode, the minimap, scrolling, groups, the menu, pause); `pad-mixed` mixes it with the mouse (the stick
  moving on from the mouse, the mouse taking the pointer back, L1 held for a Shift-click) and a finger (the bars
  hidden, the stick moving on from the finger); `pad-menu` drives the main menu to the Settings screen's Controller
  page. The scripts' `Cmd::PadAdded`, `PadDown`, `PadUp` and `PadAxis` feed `PadEvent`s as the shell would.

## Deviations from the original

- **Controllers.** The original has no controller support; all of this is new, and none of it reaches the game's
  rules: the pad sends only the mouse buttons and keys a player could press.
- **The touch side bars hide** while a pad is connected in the Controller and touch mode (`pad-hide-touch`).
- **The Settings screen has a Controller page** once a pad has been connected.
