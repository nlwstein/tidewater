# Design: optional improvements ("extras")

**Bottom line.** Tidewater plays and looks like the original unless you switch an extra on. Every extra is **off by
default**; with all of them off the frames, sounds, saved games, replays and oracle traces are byte-for-byte what they
were without the framework. Extras that only change the client live in the settings file and on the command line.
Extras that would change the simulation live in `Rules::extras`, travel with the game, and are never read from local
settings in a game with other players. Enhanced rules stay the target; Classic stays an opt-in for multiplayer.

## The options

The table is generated from the one registry in `crates/client/src/extras.rs` (`REGISTRY`); a test fails if this copy
drifts from it. **Scope** C is client-only. **C+L** is client-only but gives an edge, so in a game with other players
the session sets it for everyone instead of your own setting: in a game with other players the host chooses it in the lobby (see "Multiplayer (C+L)"). **S** changes the simulation: the host
chooses it in the lobby and it travels with the game, saves and replays (see "Sim options").

| Option | Flag | Values | Scope | Default | What it does |
|---|---|---|---|---|---|
| `health-bars` | `--extra-health-bars VALUE` | off, hovered, selected, always | C+L | off | Hit point bars over units: only those under the pointer, only selected ones, or all |
| `idle-worker` | `--extra-idle-worker` | on, off | C | off | An idle-worker button at the top of the screen and the F6 key: select your next idle worker |
| `game-clock` | `--extra-game-clock VALUE` | off, clock, clock-apm | C | off | The game clock over the map view, with or without your actions per minute |
| `group-bar` | `--extra-group-bar` | on, off | C | off | A strip over the map view's top left listing your control groups with their sizes: click one to recall it |
| `smart-attack-move` | `--extra-smart-attack-move` | on, off | C+L | off | A right-click on open ground with only fighting units selected attack-moves there instead of moving |
| `volume-voices` | `--extra-volume-voices VALUE` | 0 to 100 | C | 100 | Unit voices, in percent of the sound volume (100 leaves them as the game sets them) |
| `volume-effects` | `--extra-volume-effects VALUE` | 0 to 100 | C | 100 | Sound effects, in percent of the sound volume |
| `volume-music` | `--extra-volume-music VALUE` | 0 to 100 | C | 100 | Music, in percent of the music volume |
| `scaling` | `--extra-scaling VALUE` | auto, integer, sharp | C | auto | How the picture fits the window: automatic (whole-number scales unless they leave wide borders, then any size drawn sharp bilinear), only whole-number scales, or always any size, sharp (touch platforms always use sharp) |
| `rally-points` | `--extra-rally-points` | on, off | S | off | Right-click with a training building selected to set a rally point: the units it trains go there |
| `production-queue` | `--extra-production-queue` | on, off | S | off | Buildings queue up to five jobs, paid when queued; Cancel removes the last |
| `selection-limit` | `--extra-selection-limit VALUE` | 9, 12, 18, unlimited | S | 9 | How many units you can select and command at once: nine as in the original, 12, 18 or no limit |
| `speed-levels` | `--extra-speed-levels VALUE` | off, 1, 2, 3 | S | off | Game speeds above Fastest, 1 to 3 more levels (Enhanced rules only) |
| `minimap-ping` | `--extra-minimap-ping` | on, off | C | off | Alt-click on the minimap pings that place on your allies' minimaps, and shows their pings on yours |
| `shift-queue` | `--extra-shift-queue` | on, off | S | off | Hold Shift while giving a move, attack, attack-move, patrol, repair, harvest or build order to add it to the unit's order queue (up to 8) instead of replacing its order |
| `auto-replay` | `--extra-auto-replay VALUE` | off, menu, always | C | off | Record every game: `menu` keeps the recording so the game menu can save it, `always` also writes each game's replay to the replays folder of the data directory as it is played |
| `widescreen` | `--extra-widescreen VALUE` | off, 16:10, 16:9, auto | C+L | off | A wider game screen, the extra width being map: 16:10, 16:9, or auto to match the window (ignored on touch devices; menus and videos stay 4:3) |
| `map-zoom` | `--extra-map-zoom VALUE` | off, wheel | C | off | `wheel`: the mouse wheel over the map view magnifies it up to 4 times around the pointer and back to the normal view, never showing more map than the original does (on touch devices the pinch is `touch-pinch`) |
| `touch-drag` | `--extra-touch-drag VALUE` | pan, pan-no-glide, box | C | pan | Touch only. A finger dragged on the map scrolls it (`pan` glides on after a flick, `pan-no-glide` stops where the finger lifts); hold it still a moment first to draw a selection box instead. `box`: a drag draws the box at once, as the mouse does |
| `touch-smart-tap` | `--extra-touch-smart-tap VALUE` | on, off | C | on | Touch only. With your units selected, a tap on the ground, an enemy or a resource orders them there, as a right click does; a tap on your own unit or building selects it. `off`: every tap is a left click |
| `touch-two-finger` | `--extra-touch-two-finger VALUE` | pan, box, off | C | pan | Touch only. Two fingers dragged together scroll the map (`pan`), are the corners of a selection box (`box`), or do nothing (`off`; a pinch still zooms if it is on) |
| `touch-pinch` | `--extra-touch-pinch VALUE` | auto, on, off | C | auto | Touch only. Spreading two fingers magnifies the map view up to 4 times. `auto`: on for phones (a screen whose short side is under 90 mm), off for tablets |
| `touch-hold` | `--extra-touch-hold VALUE` | normal, long, longer | C | normal | Touch only. How long a finger is held still before a drag draws a box, and before it right-clicks: `normal` 0.3 and 0.45 seconds, `long` 0.4 and 0.6, `longer` 0.5 and 0.75 |
| `touch-buttons` | `--extra-touch-buttons VALUE` | medium, small, large | C | medium | Touch only. The size of the side bars' buttons: `medium` 9 mm, `small` 7 mm, `large` 11 mm (smaller when a bar is too narrow) |
| `touch-swap-bars` | `--extra-touch-swap-bars` | on, off | C | off | Touch only. The control-group buttons go on the left and the menu, pause and modifier buttons on the right |
| `touch-menu-tap` | `--extra-touch-menu-tap VALUE` | on, off | C | on | Touch only. A tap of three fingers at once opens the game menu (the Menu button always does) |
| `pad-speed` | `--extra-pad-speed VALUE` | normal, slow, fast | C | normal | Controller only. How fast the left stick moves the pointer (holding the left trigger doubles it) |
| `pad-acceleration` | `--extra-pad-acceleration VALUE` | on, off | C | on | Controller only. A small push on the left stick moves the pointer slowly for aiming and a full push fast; it also slows over units and buttons. `off`: the speed follows the push evenly |
| `pad-dead-zone` | `--extra-pad-dead-zone VALUE` | normal, small, large | C | normal | Controller only. How far a stick moves before it counts: raise it if the pointer drifts on its own |
| `pad-swap` | `--extra-pad-swap` | on, off | C | off | Controller only. The right face button clicks and the bottom one right-clicks, for pads with Nintendo's layout |
| `pad-hide-touch` | `--extra-pad-hide-touch VALUE` | on, off | C | on | Controller and touch. While a controller is connected the touch screen's side bars hide (taps still work, and the shoulder buttons are Shift and Ctrl) |

- **Default** is the value when nothing says otherwise. For a volume, 100 means "leave the sound as the game's own
  dialog sets it", so the original sound dialog keeps its look and meaning.
- **Settings file.** The `[extras]` section of `settings.txt` lists every option as `name=value`, with a comment line
  per option. A missing section, an unknown name or a bad value leaves the default.
- **Command line.** `--extra-NAME` (with a value, except for `idle-worker`, which is a switch) sets an option for this
  run; `--no-extra-NAME` puts it back to its default. The last of a pair wins. The command line is never written back
  to the settings file. `tidewater --help` lists them under "Optional improvements".

## The Settings screen

**Bottom line.** Every option here can be changed in the game, in the Settings screen (`settings.rs`), which the main
menu opens with F2 or its "Settings (F2)" link. The screen is generated from the registry, so a new option shows up
without new screen code. The file and the flags still work and are for power users.

- **Entry.** F2 and a small text link in the version line's font at the main menu's bottom right (a 120 x 28 tap
  target, clear of the version line and the update offer). The original's five main-menu buttons are unchanged: the key
  and the link are a documented departure, like the version line and the update buttons.
- **Rows from the registry.** Each page's registry rows come from `REGISTRY`: the row's name is `Spec::label`, its values
  come from `Spec::kind` (a flag is Off and On, a percent is 0, 10 ... 100 percent with an odd stored value kept and
  shown, a choice is its names), and its description is `Spec::description`, with a note that the change applies from
  the next game for the widescreen and S options. The words of a choice's values (`CHOICE_WORDS`: "Hovered", "16:10",
  "Unlimited") and the page of each option (`extra_page`, a `match` with no catch-all arm) are the only things written in
  `settings.rs`; a test checks that every option is on exactly one page and has words.
- **Rows that are not options.** Window (`fullscreen`), Mouse lock (`mouse_lock`), Intro video (the stored
  `OPENING_FILM` bit, which start-up honours as well as `--no-intro`) and Check for updates (`update_check`) are stored
  settings in `settings.txt`'s top section; the desktop-only ones are hidden on a phone.
- **Applying.** The screen edits a copy. OK writes the rows that were changed (Defaults counts as changing every shown row)
  into the stored settings and the run's live options (`Core::extras`, which also holds the command line), sets the
  mixer's volumes, and writes `settings.txt` at once. A row left alone keeps a command-line override for the run.
- **Window and mouse lock** are read by the shell every frame (`Core::fullscreen_wanted`, `Core::mouse_lock_enabled`),
  so they apply at once; `--no-mouse-lock` and the touch emulation still turn the lock off for a run.
- **Open settings folder** asks the shell (`Core::take_open_requests`) to open a `file://` address with SDL's
  `SDL_OpenURL`. **Import game files again...** opens the first-run screen (`FirstRun::returning`) over the running
  program, letting go of the game files first; Esc or Back returns to the main menu.

- **In a game.** Ctrl+G opens the screen over the game (`gamemenu::run_settings`, the game menu's `Screen::Settings`).
  F2 is the first remembered view's key (and Alt+F2 a network game's drop key), so it cannot be used; Ctrl+G is a chord
  the original's key table leaves unused, and `keymap` reserves it so a `[keys]` line cannot give it to an action. The
  Options dialog (F10, Options) has a small "Settings (Ctrl+G)" text link under Previous, 160 x 24 tap target, in the
  small font like the main menu's link; it opens the screen with the game as the place Back returns to. The menu
  stops a single-player game while it is open; a network game keeps running (`core::game_ticking`) with the game's own
  input off, as the original keeps a network game running under its menus. The Gameplay rows show the rules the game
  runs with (`ClientExtras::of_sim` from `Rules::extras`), greyed and unchangeable, under the note "Fixed for this
  game: they were set when it started."; Defaults leaves them, OK never writes them. "Import game files again..." is
  hidden; every other row works as on the main menu.

## What each one does

- **Health bars** (`health-bars`). A three-row bar over a unit's selection box, as wide as the box, filled in the
  information panel's health colours (green from 75 percent, yellow from 50, red below; the panel's empty colour behind).
  `hovered` shows the bar of the unit under the pointer, `selected` those of selected units, `always` every visible unit.
  Drawn in the map view (`view.rs`) above units and effects, under the fog. Rules in `overlay.rs`.
- **Idle worker** (`idle-worker`). A button "Idle N (F6)" in the resource bar, right of the food item, and the F6 key
  (the original leaves F6 unbound). Either selects the next idle worker (a peasant or peon of yours with no order),
  going through them in creation order and wrapping, and centres the view on it. It sends the normal `Select`
  command, as a click would. Rules in `idle.rs`.
- **Game clock** (`game-clock`). The game's own clock (`M:SS`, `H:MM:SS` from the first hour) in the map view's top
  right corner; `clock-apm` adds your actions per minute (every command you give, selecting included, over the clock),
  shown from the first full second. Rules in `overlay.rs`.
- **Control-group bar** (`group-bar`). A strip of small buttons over the map view's top left corner, starting 4 pixels in
  from the corner, one for each control group 0 to 9 that has units (`1:5` is group 1, five units; units no longer in the
  game are not counted). A click on a button sends what the group's number key sends (`mouse::GroupKeys::press`: select
  the group, and a second click within the double-press time centres the view). It reads the synced groups
  (`World::selections`), the lists the number keys recall. **Where it sits:** that corner holds none of the original
  interface. The message list starts 194 pixels down the screen, the notice line is at the bottom, and the game clock
  (when on) is at the top right; ten buttons are at most 400 pixels long, so they never reach the clock. Rules in
  `groupbar.rs`.
- **Smart attack-move** (`smart-attack-move`, C+L). A right-click on open ground (no unit under the pointer) with only
  fighting units selected sends the Attack button's command for the ground (`Command::Issue`, command 8, no target),
  which the sim already turns into an attack-move, instead of the right-click's move. "Fighting" means every selected
  unit is yours, can attack, and is not a worker, an oil tanker or a building; one peasant in the selection keeps the
  normal move. The simulation is not changed. It follows the session in a game with other players because it saves an
  order. Rules in `attackmove.rs`.
- **Separate volumes** (`volume-voices`, `volume-effects`, `volume-music`). Master trims on top of the game menu's
  digital and music volumes, applied in the mixer (`audio.rs`, `Trims`). Voices are unit speech and acknowledgements;
  effects are every other sound (a screen's sounds and narration included); music is the jukebox. At 100 the mixer
  does exactly what it did before (the gain is not touched). The original sound dialog is unchanged.
- **Fractional scaling** (`scaling`, C). `auto` (the default) uses the largest whole number of window pixels per screen
  pixel when it covers at least 90 % of the limiting dimension of the window's safe area (width or height, whichever
  binds), and otherwise fills the area by a fraction, drawn sharp bilinear: so a window sized to a whole scale (the
  default window, a scripted run) looks as it always did, while a full screen on a notched laptop or a resized window
  with a poor whole fit fills. `integer` always draws at the largest whole number that fits, with black around.
  `sharp` always fits by a fraction of the safe area: the picture is scaled up by the next whole number with the
  nearest texel and then scaled linearly down, so pixels stay crisp. Touch platforms use `sharp` whatever the setting
  says (`docs/design/touch.md`). The settings file stores the value by name, so a file written before `auto` existed
  (`scaling=integer`, `scaling=sharp`) reads as it did. Rules in `display.rs` (`Fit`, `fractional_for`,
  `Letterbox::fit_with`).
- **Rally points** (`rally-points`, S). With a training building of yours selected (a town hall, barracks or
  another building that trains units, not a research-only one), a right-click on the map sets its rally point: on
  open ground, the units it trains walk there; on a gold mine or a tree, workers harvest it; on a friendly unit they
  follow it; on an enemy they attack it. A small flag in the owner's colour marks the point while the building is
  selected. The command is `Command::Rally` (appended last, in the turn like any other), the point lives in
  `World::ext` and is hashed, saved and compared only when the option is on. The point is dropped when the target
  goes. Rules in `crates/sim/src/rally.rs`.
- **Production queue** (`production-queue`, S). A building that is training, researching or upgrading takes up to four
  more jobs (five including the one running). **Paid when queued:** the cost is taken at once and refunded on cancel,
  and the food check is the usual one. Cancel (slot 9) removes the last waiting job first, the running one when none
  waits. Waiting jobs show as small icons under the production icon in the information panel. Research bits are set
  when the job is queued, so a second copy of a queued research is refused. Nothing queues behind an upgrade (a
  town hall or tower becoming another building). A job that cannot start when its turn comes (no food, building lost
  its requirement) is refunded and skipped. Rules in `crates/sim/src/queue.rs`.
- **Selection limit** (`selection-limit`, S). 12, 18 or unlimited units in a selection, a box select, a control
  group and a command. The synced selection keeps its nine places (`select::PLACES`); the units past nine are kept
  in `ext.more` and are not saved (a load starts with nothing selected, as before). The information panel keeps its nine
  icons and draws "+N" over the last one for the units beyond them. Under the original limit nothing changes, bit
  for bit. The mouse rules (`mouse.rs`) take the limit from the world.
- **Speed levels** (`speed-levels`, S, Enhanced rules only). 1 to 3 more speed levels above Fastest, reached with the
  ordinary speed keys and the game speed slider (in multiplayer too, through `Command::Speed`). The lobby's slider
  still stops at Fastest.

  | Level | Name | Cycle | Cycles a second |
  |---|---|---|---|
  | 8 | Fastest | 13 ms | 77 |
  | 9 | Fastest +1 | 10 ms | 100 |
  | 10 | Fastest +2 | 8 ms | 125 |
  | 11 | Fastest +3 | 6 ms | 167 |

  The network's turn length `K` scales with the cycle (`lockstep::turn_length`). A slow computer simply runs behind
  at these speeds, the scheduler's limit of three cycles a frame still holds. Under Classic rules the option does
  nothing.
- **Minimap ping** (`minimap-ping`, C). Alt-click on the minimap in a network game sends `Msg::Ping` (a message of its
  own outside the turns and the state hash, like chat). Allies see a blinking ring in the sender's colour on their
  minimap for about five seconds, and you see your own. At most one ping every 12 cycles per player; each machine
  checks the alliance against its own table. In a single-player game or a replay the click does what it always did.
  Rules in `ping.rs`.
- **Shift-queued orders** (`shift-queue`, S). With the option on, holding Shift while you give a move, attack,
  attack-move, patrol, repair, harvest or build order (right-click, a target-mode click, or a building site) adds the
  order to the end of each selected unit's queue instead of replacing its order. Selection keys behave as always.
  The client wraps the command in `Command::Queued`, which travels through the turn stream like any command; the sim
  does the rest in `waypoints.rs`. A machine whose option is off ignores the wrapper, but the option is a lobby-set sim
  option like the others, so every machine has the same value.
  - **A plain order clears the queue**, and so does Stop. A Shift-order to an idle unit just runs.
  - **When the next one starts.** At the start of a cycle a unit with a queue and no order of its own (idle, nothing
    pending) takes the first queued order. So an order "completes" when the unit falls idle: a move or follow on
    arrival, an attack when the target dies or is lost, an attack-move on arrival, a repair when the target is
    whole or gone, a build when the building is finished, a harvest when the source is used up and nothing else
    is found. A patrol never ends, so queue it last. A queued order whose target has gone is skipped, and a queued
    build is checked again when it starts (a refused site is skipped).
  - **Cap: 8** orders waiting behind the current one per unit; more are dropped. A unit that dies loses its queue.
  - **Building sites.** A Shift-click on a site keeps placement mode on, so you can place several buildings in a row.
  - **Drawn** while the option is on: for each selected unit of yours, a small hollow square at every queued
    destination, joined by a dotted line from the unit and from one point to the next (`view::draw_waypoint`).
  - **State and hash.** The queue lives in `World::ext` (`State::orders`), so it is hashed, fingerprinted and saved
    only while an option is on, and never when every option is off: a game with every extra off is byte-identical to
    one without the feature.
- **Replay saving** (`auto-replay`, C). A client-only option, off by default (`--record-replay` still works on its own).
  `always` records every game, single-player or network, and writes the file to the `replays` folder of the user data
  directory as the game is played, through `Storage::write_replay` (the same file `--record-replay` writes).
  `menu` records in memory only and adds a **Save Replay** button to the in-game menu that writes the game so far
  (and carries on writing), so a game you decide to keep after the fact is not lost. The default menu is unchanged
  while the option is off. A game started from a save, or with a `--demo-*` option, is not recorded, as before.
- **Widescreen** (`widescreen`, C+L). Off by default. `16:10` makes the game screen 768 x 480, `16:9` 864 x 480 (853.3 rounded
  to a whole tile) and `auto` the tile multiple nearest the window's shape, from 640 to 864. The screen stays 480 lines
  high, so the interface art keeps its pixel size and the map view only gains width: 18 or 21 tiles across instead of
  14, nothing zoomed out and nothing more shown above or below. Menus, briefings, videos, the score screen and the game
  menu stay 640 x 480 in the middle with black beside them. The option is ignored on a phone or tablet, with
  `--touch-emulation`, and while touch is in use, where the pillarbox holds the touch buttons. Because the wider view
  is an edge, the lobby's value is used for everyone in a game with other players. The widths, the layout and the
  pointer mapping are in `docs/design/client-core.md`, "Widescreen"; the code is `widescreen.rs`.
- **Magnifying zoom** (`map-zoom`, C). Off by default, which leaves the mouse wheel doing nothing over the map view, as in
  the original. With `wheel`, a wheel step away from you over the map view zooms in around the pointer, 1 to 2 to 4
  times, and a step toward you zooms back out to the original's view and stops there (`game::pan_and_zoom`,
  `game::magnified`): it can only magnify, never show more map than the original, so it is fair in a game with other
  players (client-only, no lobby setting). The zoom is the map view's own (`MapZoom`, `MapView::zoom`, map pixels per
  screen pixel: 1, 1/2, 1/4), the machinery `--free-zoom` and a touch pinch (`game::touch_view`, 1 down to 1/4) already
  use, so the pointer, selection, the minimap's view box and the edge scroll follow the zoomed view. Steps are powers of
  two so map pixels stay whole blocks of screen pixels; a zoom a pinch left in between goes to the nearest step first.
  The point under the pointer stays under it (to within the tile grid, which the view is kept on). The wheel over the
  minimap or the interface does nothing. Turning the option off in the Settings screen during a game puts the view back
  to the original's. `--free-zoom` stays a development aid with its own range (1/4 to 8, wheel anywhere) and wins when
  both are on. The scene `extras-zoom` shows it.

## Rebindable keys

Not an option with a switch: the `[keys]` section of `settings.txt` rebinds the game's keys and the command buttons'
keys. The Settings screen's **Keys** page edits the section (see "The Keys page" below); the file stays the format. With no `[keys]` section every key is the original's (`keymap.rs`: the key table `mouse::key_action`, the buttons'
hotkeys from the string table, F6 for the idle worker). The game writes the section back exactly as it found it, so a
mistake in it is never lost.

```
[keys]
pause=ctrl+u
button-move=j
slot-1=q
idle-worker=ctrl+i
```

- **The line.** `action=key`, one per action. A key is optional `ctrl+`, `alt+` and `shift+` then a letter, a digit,
  `f1` to `f12` (not `f10`), `minus`, or `equal` (`plus` and `numpad-add` mean the same). Escape, Enter, Space, the
  arrow keys and F10 can't be rebound.
- **Game keys** need `ctrl+` or `alt+` with a letter (a bare letter belongs to the command buttons). Names:
  `speed-up`, `speed-down`, `pause`, `centre-primary`, `group-set-0` to `group-set-9`, `group-recall-0` to
  `group-recall-9`, `group-show-0` to `group-show-9` (Alt+digit), `goto-view-1` to `goto-view-3`, `remember-view-1` to
  `remember-view-3`, `game-menu`, `options`, `help-menu`, `keyboard-help`, `sound-dialog`, `speed-dialog`,
  `preferences`, `tips`, `save-game`, `load-game`, `restart`, `quit`, `quit-dialog`, `toggle-music`, `toggle-sounds`,
  and `idle-worker` (the key of the `idle-worker` option; F6 with any modifiers by default).
- **Command buttons** take a bare letter or digit. `button-move`, `button-attack`, `button-patrol`, `button-stop`,
  `button-stand-ground`, `button-harvest`, `button-repair`, `button-return-goods`, `button-unload`,
  `button-attack-ground`, `button-demolish`, `button-build-basic` and `button-build-advanced` give the button with that
  command a key of its own (its original hotkey stops working). Every other button is reached by place with `slot-1` to
  `slot-9`, the 3 × 3 panel read left to right and top to bottom, whatever the button is (so `q w e / a s d / z x c`
  works as a grid). A slot key wins over a button's own letter.
- **Conflicts.** A key on two actions, or a key an action you did not rebind still uses, is refused: the game prints
  `settings: [keys] pause=Ctrl+S: Ctrl+S is already used by toggle-sounds; the line is ignored` at start-up and the
  action keeps its original key. The first line wins; a second line for the same action is refused too. Rebinding an
  action frees its original keys, so two actions can swap (in the file; the Keys page binds one at a time). Two buttons that show at the same time with the same
  hotkey letter are not caught (which buttons show depends on the selection); the first in panel order acts.
- **Checks.** Rules and messages: `keymap.rs` tests. With no section the key map hands every key on unchanged (a test
  tries every key and modifier). The frame scene `extras-keys` rebinds Move, pause and the idle key.

### The Keys page

The fifth page of the Settings screen (`settings.rs`, hidden on touch-only platforms, `Env::touch`) lists every
action the section can name (`keymap::rows`: 19 game keys and dialogs, 6 views, 30 control-group keys, the idle worker,
13 command buttons and the 9 panel places, 78 rows) in the Select scenario dialog's list and scroll bar look, 8 rows
of 24 pixels, scrolled by the wheel, the bar or the arrow keys. A row shows the key the file gives it in gold, else the
original's in grey (`own letter` for a command button, `-` for a panel place).

- **Choosing a key.** A click, or Enter (or Space) on the focused list, puts the screen in "Press a key... (Esc
  cancels)"; the next key that is not a modifier alone binds the selected row. The chord is the key with Ctrl, Alt and
  Shift as held (`keymap::Chord::pressed`, the same key names the file reads).
- **The rules are the file's.** The new line goes through `KeyMap::parse` last (`keymap::with_binding`), so it is the
  one that is refused: another action's key, a key a not-rebound action still uses, the Settings key (Ctrl+G, which
  `keymap` reserves) and a wrong shape (a bare letter for a game key, a chord for a button) all leave the lines as they
  were and say why in the description area, naming the action that has the key. **There is no one-step swap**: because
  an action keeps its original key until it is rebound, `speed-up=minus` is refused while `speed-down` still has minus.
  A swap takes two steps through a spare key; rebinding an action frees its original keys.
- **Backspace** on the list removes the row's line (its own key again). **Reset keys** removes every line for an
  action and keeps comments and lines that name no action, as the file keeps whatever it holds (`keymap::reset_lines`).
- **OK and Cancel.** The page edits a copy of the section's lines (`Values::keys`). OK parses them into the prefs'
  `keys` (`Prefs::keys`, read by the game every frame), so `settings.txt` holds the lines exactly as the file's text
  format has them; a Keys page that was not touched leaves the section as the file had it, typos included. Cancel and
  Esc discard. Defaults on the other pages leaves the keys alone.
- **Checks.** Unit tests in `keymap.rs` (every row is a name the file reads, a chord prints as it parses, binding and
  refusing) and `settings.rs` (capture, conflict, shape, reset, scrolling, the round trip through `Prefs::to_text` and
  `Prefs::from_text`); the frame scenes `settings-screen` and `settings-ingame` show the page.

## Sim options

`tidewater_sim::Extras` (`crates/sim/src/extras.rs`) is the set of options that change the simulation: `rally_points`,
`production_queue`, `selection_limit` (0 for nine, 12, 18, or `UNLIMITED`), `extra_speeds` (0 to 3) and `order_queue`. It is a field of
`Rules`, so it is:

- **hashed** with the world (`World::state_hash`), so two machines with different options desync at once; the state
  and its extra data (`World::ext`: rally points, queues, the selection beyond nine places) are in the hash and the
  fingerprint **only when an option is on**, so a game with every extra off hashes exactly as it did before;
- **saved** with the rest of the rules in a saved game;
- **carried in a replay's header** (`RulesTag::extras`), and playback starts the world from the header's value;
- **carried in the lobby's start message** (`LobbySpec::extras`, `GameConfig`), so every machine starts with the host's
  choice. The lobby shows an "Extras" box only when at least one sim option is on in it (or was on in the host's own
  settings), so a lobby with none looks as it always has; only the host can change it
  (`HostRoom::set_extras`).

A game never reads sim options from the local settings unless it is a new local game (`extras::local_sim_extras`).
A replay uses the recording's; a loaded game keeps the ones it was saved with.

**Versions.** Sim options are part of the file formats and the protocol, so changing them bumps all three: the saved game
(`savefile::VERSION`, 24), the replay (`tidewater_replay::format::VERSION`, 6) and the network protocol
(`net::proto::PROTOCOL`, currently 8). The C+L choice rides in the same lobby messages and also bumped the protocol, but not the saved game or the replay (below). With every extra off the option data is empty or skipped, so nothing else in a file or the
hash differs.

## Multiplayer (C+L)

`extras::effective(local, session)` picks a game's values. A local session returns `None` from
`Session::lobby_client_extras` and the player's own settings are used. Any other session returns its own set, and
for the C+L options that set wins over the local settings; the C options stay each player's own.

- **The host chooses.** Every row of the registry with scope C+L (`extras::lobby_specs()`, so a new C+L option appears
  with no other change) is a row in the lobby's "Extras" box, below the S rows, and the host cycles it like them. The
  box starts from the host's own settings and, like the S rows, shows only once any option is on. Joiners and observers
  see the box read-only. From the command line, `--lobby extras=NAME=VALUE/NAME=VALUE` names C+L options (anything
  else, or a value the option does not take, is refused); the host's own settings and `--extra-NAME` flags stand for the
  ones not named.
- **How it travels.** `GameConfig::client_extras` and `LobbyView::client_extras` hold the values that are not the
  default, as `(name, value)` pairs (`ClientExtras::lobby_values`). Names, not positions, so a table with other rows
  never shifts the meaning; a name this program does not know, a C option's name or a value out of range is ignored when
  read (`ClientExtras::from_lobby_values`). The config goes to every machine in the Welcome (command line) and the Go
  (menu lobby), observers included. This bumped `PROTOCOL` to 7.
- **Who uses it.** `NetSession::lobby_client_extras` returns the host's values (`Params::client_extras`), so
  `game::extras` gives every player the same health bars, smart attack-move and widescreen.
- **Replays.** A `.twrep` does not record them, and a replay hands over the defaults (off). They change what one
  player sees, not the game, and a recording is watched by one person, so there is no fairness to keep; leaving them out
  keeps the replay version at 6. (The header still records the S options, which are the game.)
- **Tests.** `extras.rs` (`the_lobby_carries_only_the_c_plus_l_values_that_differ_and_reads_back_what_it_knows`, and
  `a_non_local_session_sets_the_c_plus_l_options_and_leaves_the_c_ones`), `net/lockstep.rs` (every player and the observer
  end up with the host's values whatever their own), `net/room.rs` (the choice reaches the joiner's view and both configs),
  `net/lobby.rs` (`--lobby extras=`) and `lobbyscreen.rs` (the Extras list and who may change it).

## How to add an option

1. **Add a row** to `REGISTRY` in `extras.rs` (and an `Id` variant and the `COUNT`): name, kind, default (off), scope
   and a one-line description. The settings file, the flags and the help text follow.
2. **Add a typed accessor** on `ClientExtras` (like `health_bars()`), so the rest of the client never handles raw numbers.
3. **Read it where it acts** and keep the off path untouched: the code for the default must not run, or must be a
   no-op to the bit (see how `audio::trimmed` skips the multiply at unity). Put the rule in a small module with unit tests
   (like `overlay.rs`, `idle.rs`).
4. **Choose the scope.** If the option shows information the original hides, or lets one player act faster than another
   could, use C+L. Read it through `game::extras(c)`, not `c.extras`, so a session can override it.
5. **Test it with the option on**: unit tests for the rules, and a scripted scene whose `extras` field turns it on
   (`script.rs`, scene `extras`). A new scene is a new snapshot entry; the old scenes must not change.
6. **Put it on a Settings page**: give it a `label` in the registry row, a page in `settings::extra_page` (the build
   fails until you do) and, for a choice, its words in `settings::CHOICE_WORDS` (a test fails without them).
7. **Document it**: copy `extras::doc_table()` into this file (a test checks), and add the flag to the README's
   "Optional improvements".
8. **If it touches the sim**, it is not a client option: add a field to `tidewater_sim::Extras` instead (and a registry
   row only for the local default), bump the save and replay versions, and make sure it is hashed, saved and in the
   replay header.

## Checks that keep the defaults honest

- The frame snapshot (`crates/client/tests/frames.rs`) runs its original scenes with every extra off and is never
  re-blessed for an extra (the main menu's frames changed once, for the Settings link, which is not an extra); the scenes `extras`, `extras-bars`, `extras-groups`, `extras-attack` and `extras-keys` turn options on and have their own entries; the scene `settings-screen` shows the Settings screen and `settings-ingame` the Settings screen over a game.
- `Trims::default()` leaves the mixed sound identical; the audio snapshot of the original scenes shows it.
- The oracle traces and the whole-map snapshot run on the sim, which an empty `Extras` does not change.
