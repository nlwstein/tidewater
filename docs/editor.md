# Map editor (prototype)

**Bottom line.** `tidewater-editor` opens a `.pud` map, shows it with your installation's own tiles and unit
sprites, paints terrain with automatic transitions, places and moves units, edits players and the description, and
saves a map that Tidewater loads and plays. It is a first prototype: there are no walls, no unit or upgrade data
editing, and the region map it writes after a terrain change is our own rebuild (see [What doesn't work
yet](#what-doesnt-work-yet)). It bundles no game art: it reads the art from the installation you point it at. It is built from source; the releases don't include it.

## Run it

```sh
cargo run --release -p tidewater-editor -- --install <path-to-game> [MAP.pud]
```

| Option | What it does |
|---|---|
| `--install PATH` (or `TIDEWATER_INSTALL`) | The *Warcraft II: Battle.net Edition* installation, or a folder with its `.mpq` archives, to read the tiles and sprites from. Required. |
| `MAP` | The map to open. Without one, a new 64 × 64 forest map. |
| `--game PATH` (or `TIDEWATER_GAME`) | The game program **Play in Tidewater** starts. Without it, `tidewater` next to the editor's own program (both are in `target/release` after a release build of the workspace). |
| `--play-data-dir DIR` | The data directory the play test gives the game (`--data-dir`), so a play test never touches your own settings and saves. Without it, `tidewater-editor-play` in the system's temporary folder. |
| `--size WxH` | The window size in points (1400x900 by default). |
| `--actions SCRIPT` | Development aid: scripted actions, see [Scripted actions](#scripted-actions). |

## Use it

| Part | How it works |
|---|---|
| **Canvas** | The map with its real tiles and unit sprites in player colours. Right or middle drag scrolls; the mouse wheel (or a pinch) zooms around the pointer, from 1/16 to 4 times; the arrow keys scroll. The status bar shows the tile under the pointer, its tile number and the unit there. |
| **Minimap** | Top right. Click or drag in it to move the view; the white frame is what the canvas shows. |
| **Terrain** | Pick a brush (deep water, water, dark coast, coast, dark ground, ground, forest, rock) and a size (1 × 1, 3 × 3, 5 × 5 tiles), then click or drag on the canvas. The tiles around the stroke adjust so that every edge joins, adding the transitions the tileset needs: forest painted into water gets ground and coast around it. A drag is one undo step. |
| **Units** | Pick a player under **Units for** (Player 1 to 8, or Neutral) and a type from the palette: land units, ships, air units, buildings, heroes, and the neutral types (start location, gold mine, oil patch, critter, circle of power, dark portal, runestone, skeleton, daemon). The palette shows the player's race. The outline under the pointer is white where the unit fits and red where it doesn't; a click places it. Gold mines, oil patches and the other neutral types go to the neutral player. Each player has one start location: placing a second one moves it. |
| **Select** | With **Select** (or Esc), click a unit to select it and drag it to move it (it only drops where it fits). The panel on the right edits its owner, a mine's or oil patch's amount (gold mines and oil patches store multiples of 2,500) or whether a computer player's unit is active. Delete or Backspace removes it. |
| **Players** | Map > Players: each player's race, controller (human player, computer, passive computer, rescue passive or active, nobody), and starting gold, lumber and oil, with a button that copies player 1's resources to all. A race change switches the player's start location to that race. |
| **Map properties** | Map > Properties: the description (31 characters). |
| **Files** | File > New (32, 64, 96 or 128 tiles; forest, winter, wasteland or swamp), Open (a path, or a list of the installation's `Maps` folder), Save (Cmd+S or Ctrl+S) and Save as. |
| **Undo** | Edit > Undo and Redo, or Cmd+Z, Cmd+Shift+Z and Cmd+Y (Ctrl on Windows and Linux). |
| **Play in Tidewater** | Saves a copy of the map as `editor-playtest.pud` in the play-test data directory and starts the game on it (`tidewater --install <install> --data-dir <dir> <map>`), as a custom game with the map's own settings. The map you are editing is not saved by this. To check a map without a window, `tidewater --script playtest --script-out DIR --headless MAP` runs it for ten seconds and, with `TIDEWATER_SCRIPT_PICTURES=1`, writes `playtest-start-composed.png` and `playtest-later-composed.png` (delete the `frame*.png` files it also writes). |

A new map has only terrain. To play it, give each playing player a start location and either place their
starting units (a town hall and peasants, say) or start it with "one worker only" (`tidewater ... --setup
custom:units=one-worker`).

## What is saved

Saving rewrites only what changed and keeps every other section of the file as it was, unknown sections and their
order included: a shipped map opened and saved without edits is the same file byte for byte (all 153 maps of the
`Maps` folder in the tested installation; `crates/editor/tests/install.rs`).

| Section | Written when |
|---|---|
| `DESC`, `OWNR`, `SIDE`, `SGLD`, `SLBR`, `SOIL`, `UNIT` | That part changed |
| `MTXM` (tiles) | The terrain changed |
| `SQM ` (movement map) | The terrain changed: rebuilt from the tileset's group flags, which equals the shipped movement map on every shipped map |
| `REGM` (region map) | The terrain changed: rebuilt by our own rule, see below |
| Everything else (`UDTA`, `UGRD`, `ALOW`, `AIPL`, `SIGN`, `ERAX`, unknown sections) | Never: kept as loaded |

### The region map

The region map numbers each connected area of land and water, and marks forest, rock and walls
([occupancy spec](specs/movement/occupancy.md), "How passability changes"). How the original numbers the areas is
not specified yet. The editor rebuilds it as 8-connected areas of one class (water, walkable land, shore), each
numbered in scan order, water from 0 and land from 0x4000. On the shipped maps this matches the stored region map
exactly on 38 of 153 maps. The class (water area, land area, forest, rock, wall) agrees on every tile except some
shore tiles (the water-to-coast groups): the shipped maps leave about one in twenty of those unassigned (0xFFFA),
by a rule we haven't found. On the other maps the areas are also cut or numbered differently; for example the
3 × 3 tiles under some gold mines appear as areas of their own. Tidewater's simulation only compares region values for
equality, so a rebuilt map plays in Tidewater, but **the original game may need the exact numbering**: a map whose
terrain you changed is not yet guaranteed to work there. A map whose terrain you didn't change keeps its own region
map.

## How terrain painting works

Each tileset group record lists the terrain at the ends of its four edges, and on every shipped map neighbouring
tiles agree on their shared edge. So the map is a grid of corner terrain codes and each tile is the group whose four
corners match. The groups that mix two codes say which terrains may touch; they form a chain (water, coast, ground,
with forest, rock and dark coast off it). A stroke sets corners; a repair pass moves each neighbouring corner that
can't touch its new neighbour one step along the chain, outward until everything joins; then each changed tile takes
the group of its corners, keeping its own tile when the group is unchanged. Details: `crates/editor/src/terrain.rs`.
Tests check that random strokes on synthetic and real tilesets always leave a valid tiling.

## What doesn't work yet

- **Painting ignores units:** terrain painted under a unit leaves the unit where it was, even on water.
- **Walls** (human and orc) can't be painted; walls already on a map are kept until you paint over them.
- **Placement rules** are simplified: land units on land, ships and oil patches on water, buildings on buildable
  land, shipyards and refineries partly on water, flyers anywhere. The original's spacing rules (halls 3 tiles from a
  gold mine, platforms on oil patches) are not checked.
- **Unit data, upgrades and restrictions** (`UDTA`, `UGRD`, `ALOW`) and the computer players' scripts (`AIPL`) are kept
  but not editable.
- **Light and dark variants:** the brushes choose a tile variant by position; there is no plain, filler or random
  choice.
- **No native file dialogs or menu bar**: Open and Save as take a typed path or a pick from the installation's maps.
- **The region map** may not be what the original game expects, see above.
- **Map size and era** are fixed after New.

## Scripted actions

`--actions` runs a script one step per frame, for demonstrations and screenshots. Pointer steps go in as real input
events, so they exercise the same code as the mouse. Commands are separated by `;`; positions are in tiles (with
fractions).

| Command | What it does |
|---|---|
| `click X Y`, `drag X1 Y1 X2 Y2`, `hover X Y` | Pointer actions with the left button |
| `key [cmd+][shift+]NAME` | A key press, with egui's key names (`Z`, `Delete`, `Escape`) |
| `view X Y ZOOM` | Centres the view on a position at a zoom |
| `tool select`, `tool terrain CODE SIZE`, `tool unit KIND` | Picks a tool (terrain codes 1 to 8; `KIND` decimal or `0x` hex) |
| `player N` | The player new units go to (1 to 8, 16 for neutral) |
| `window NAME` | Opens `players`, `map`, `new`, `open` or `save` |
| `wait N`, `save PATH`, `shot PATH`, `play`, `quit` | Waits N frames, saves the map, writes the window's picture to a PNG, plays the map in Tidewater, closes |

Example: `--actions "view 20 20 1; tool terrain 2 5; drag 15 15 25 18; shot water.png; quit"`.

## Code

The crate is `crates/editor` (`tidewater-editor`), built on egui and eframe (pinned, OpenGL backend):

| Module | Job |
|---|---|
| `doc` | The map being edited over the file's sections; saving only what changed |
| `terrain` | The corner-code painter and the tiling check |
| `derived` | The movement and region maps |
| `catalog` | Unit names, palette groups, footprints and placement rules |
| `history` | Undo and redo |
| `assets` | The tileset atlas, minimap colours and sprites, from the installation |
| `app` | The window |
| `script` | Scripted actions |

The lossless section list it saves through is `tidewater_formats::pudfile`.
