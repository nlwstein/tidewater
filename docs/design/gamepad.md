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
| `touch` | Hides the side bars while a pad is connected (option `pad-hide-touch`). |
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
| South | left click; held while the stick moves, a selection box | left click |
| East | right click (move, attack, harvest); in panel mode, back to the map | right click |
| West | panel mode (below) | - |
| North | the pointer to the minimap, and back | - |
| L1 | Shift, held (Shift-click adds to the selection or removes from it) | Shift, held |
| R1 | Ctrl, held (Ctrl-click takes every unit of that type on screen) | Ctrl, held |
| L2 | the pointer twice as fast, held | the same |
| R2 | Alt, held | Alt, held |
| D-pad | control groups 1 to 4 (up, right, down, left): a press recalls the group, twice quickly centres on it; with R1 held it sets the group, with R2 held it centres and recalls | arrow keys |
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

**Panel mode.** West puts the pointer on the first command button shown; the d-pad steps between the buttons (by
their grid, skipping empty slots), south presses one, and east or west goes back to where the pointer was on the map.
Choosing a command that needs a target ends panel mode, so the stick aims at once. A push of the stick also ends it.

**The minimap.** North puts the pointer on the minimap (at its centre the first time, then where it last was there),
where south moves the view as a click does; north again puts it back on the map where it was.

## Mixed with the mouse and touch

Mouse, touch and pad work together; nothing has to be switched. The shoulder buttons are real Shift and Ctrl, so
holding L1 while clicking the mouse or tapping adds to the selection. While a pad is connected the touch side bars
hide (`pad-hide-touch`, on by default), since the pad has every button they hold; taps on the map still work, and the
bars come back when the last pad goes.

## Options

All five are client options (`C`: they change nothing for other players) on the Settings screen's Controller page,
which shows once a pad has been connected this run:

| Option | Choices | Default |
|---|---|---|
| `pad-speed` | normal, slow, fast | normal |
| `pad-acceleration` | on, off | on |
| `pad-dead-zone` | normal, small, large | normal |
| `pad-swap` | on, off | off |
| `pad-hide-touch` | on, off | on |

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
- **The touch side bars hide** while a pad is connected (`pad-hide-touch`).
- **The Settings screen has a Controller page** once a pad has been connected.
