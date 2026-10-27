# Mods

**Bottom line.** A mod is a folder of loose files that replace some of the game's own files (pictures, sounds, maps, data tables). Put it in the `mods` folder of Tidewater's data directory, turn it on in Settings, and restart. Mods are data only; they cannot run code. This is version 1: it replaces files, it does not add new kinds of units. See [Current limitations](#current-limitations).

## What a mod is

A mod is one folder inside `mods`:

```text
mods/
  big-axes/
    mod.toml              optional: name, version, description, authors
    rez/unitdata.dat      replaces the game's unit table
    Art/...               replaces pictures, sprites, tilesets
    Maps/MyMap.pud        adds a map to the Custom Scenario list
```

- **Same names as the game.** Files sit at the same path the game uses inside its archives, such as `rez/unitdata.dat`. Letter case does not matter, and you can use `/` or `\`. A file the mod does not have comes from the game as usual.
- **Mods win over the game.** A mod's file is read before the game's own loose files and archives.
- **Folder name is the mod's name.** Use letters, digits, spaces, `-`, `_` and `.` (up to 48 characters, not starting with a dot or a space). Anything else, such as a path, is refused.
- **`mod.toml` is optional.** It only labels the mod:

```toml
name = "Big axes"
version = "1.2"
description = "Grunts hit harder."
authors = ["Ann", "Bo"]
```

Only that small part of TOML is read: `key = "text"` lines, `authors` as a list on one line, and `#` comments. `version` matters beyond the label: it is shown to other players and recorded in replays.

## Where the folder goes

The `mods` folder sits **next to** the `game` folder in the data directory. It must not be inside `game`, because importing the game files again replaces `game` whole and would delete the mods.

| System | Mods folder |
|---|---|
| macOS | `~/Library/Application Support/Tidewater/mods` |
| Windows | `%LOCALAPPDATA%\Tidewater\data\mods` |
| Linux | `~/.local/share/tidewater/mods` (or under `$XDG_DATA_HOME`) |
| PS Vita | `ux0:data/Tidewater/mods` (not yet tried on a Vita) |

`--data-dir <dir>` moves the data directory, and the `mods` folder with it. Make the `mods` folder yourself if it is not there.

## Turning mods on

**In the game.** Open Settings (F2 on the main menu). When there is at least one mod in the folder, a **Mods** page appears. Each row holds one mod: choose a mod in a row, or None. **A lower row overrides the rows above it**, and every mod overrides the game. OK saves the list; it applies the next time Tidewater starts. The Mods page holds up to six mods.

**In `settings.txt`.** The same list is the `[mods]` section, one folder name per line, first line lowest priority:

```text
[mods]
big-axes
better-sounds
```

A mod named there that is no longer in the folder is reported and skipped, so a deleted mod cannot stop the game from starting.

**On the command line.** `tidewater --mod NAME` turns a mod on for one run, after the ones in settings. Repeat it for several. A wrong name is an error here. This is meant for development.

## Multiplayer and replays

- **Everyone needs the same mods.** The game compares a fingerprint of every mod file (and of the unit, upgrade, script and AI tables) when someone joins. If it differs, the host refuses with "different game data or mods" and lists the host's mods and the joiner's, such as `host mods: big-axes 1.2; joiner mods: none`. The fingerprint covers the files' names and contents and the order of the mods, so copy the whole folder, and use the same order.
- **Maps from a mod** work: the host's map is sent to the joiners as usual.
- **Replays record the mods.** A replay stores the fingerprint of the game files and the mod names and versions it was recorded with. Playing it with other game files or other mods stops at the start with "recorded with different game files" and says which part differs: the data hash (another game version or edition) or the mod set. `tidewater --force-replay` and `twtool replay --force` play it anyway, and it will most likely go out of sync. `twtool replay` takes `--mods-dir` and `--mod` to turn the replay's mods on.
- **Rules.** Mods work with either rule set. Enhanced rules are the default; Classic is an opt-in for multiplayer only.

## Safety

Mods are data. Tidewater reads the files the game asks for and runs nothing from a mod. A mod name cannot point outside the `mods` folder, and a file name cannot climb out of a mod. A mod can still change the game's data, such as unit costs, so only use mods you trust, and keep in mind that the files are not checked for being well-formed beyond what the game's own readers do: a broken table gives an error or odd behaviour, not code execution.

## Current limitations

- **Files only.** A mod replaces or adds whole files. There is no way yet to change one number without shipping the whole table, and the tables are the game's own bytes (so a mod that includes `rez/unitdata.dat` contains Blizzard's data; share such a mod with care).
- **No new unit types, spells or buildings.** The unit types are fixed by the game's map format, and the command buttons and spell logic are in the program's code. A mod can change how existing ones look and behave through their tables.
- **No scripting.** There is no mod language or API.
- **Some data is risky to change.** Tilesets must agree with the movement data stored in maps, and the computer player's script and AI tables are only partly understood. Pictures, sounds, strings and maps are the safe ground.
- **No mod manager or download.** You copy the folder yourself. There is no list of mods, no updates and no dependency handling.
- **Restart to apply.** Changing the list in Settings takes effect on the next start.
- **Desktop first.** On iOS and Android the data directory is the app's private folder, which you cannot reach with a file manager, and the import picker only copies game files. There is no way yet to bring a mod into the app, so mods are for the desktop builds (and probably the Vita, untried). A phone can still join a network game only if its mods match, which in practice means no mods.
- **At most six on the Settings page**, though `settings.txt` can list more.
- **The mod fingerprint reads every mod file** the first time a game or lobby needs it, so a very large mod adds a short wait at the start of a game.

## Future plans

None of this is promised or scheduled.

- **Text overrides for the unit and upgrade tables.** A `units.toml` of changes by unit name (for example `grunt.damage = 12`), so mods can be shared without any of the game's bytes. It needs a table of field names and a writer.
- **New unit types and spells,** which needs the button sets and caster logic to be data, not code.
- **A sharing format:** one file per mod, with a checked manifest, a version, and a list of what it changes, plus something to install it from a download.
- **Map-editor integration:** a mod's maps, and mods that carry the map editor's tilesets and unit sets.
- **A way to bring a mod onto a phone** through the same picker the import uses.
- **Switching mods without a restart,** and a mod list in the multiplayer lobby that the host's choice sets for everyone.
