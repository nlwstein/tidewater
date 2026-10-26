# Tidewater documentation

**Start with the [project README](../README.md)** to install, import your game files and play. This page indexes the design documents and the behaviour specifications behind the code.

## Design documents

Design documents explain how the code is organised and why. They live in [`design/`](design/).

| Document | What it covers |
|---|---|
| [Game-file import](design/import.md) | How `--import` finds, checks and copies your *Warcraft II* files into Tidewater's data directory: game folders, the GOG installer and CD images, the first-run screen and the exit codes. |
| [An engine-neutral client core](design/client-core.md) | How the client is a plain-Rust core that a shell calls once per frame, the SDL 3 shell, the platform and storage seams, widescreen, and the frame and audio snapshot tests. |
| [Network play](design/network.md) | Lockstep over TCP: the menus, running a host and joiners (`--host`, `--join`), the turn engine, the sync check, leaving and dropping, the host handing over, observers, the protocol, and the spec gaps. |
| [Multiplayer determinism](design/multiplayer-determinism.md) | What keeps the simulation identical on every machine in a lockstep game, the rules new sim code must follow, and which local-player reads are still open questions. Also the session layer and the `.twrep` replay format. |
| [Optional improvements](design/extras.md) | The registry of optional extras, the Settings screen and rebindable keys, how the settings file, flags and lobby carry them, and how to add one. |
| [Touch input](design/touch.md) | Gestures, the on-screen controls, scaling, and desktop touch emulation. |
| [Tidewater on iOS](design/ios.md) | Building the iPhone and iPad app, signing through an xcconfig, importing game files on a phone, background pause and resume, and the limits. |
| [Tidewater on Android](design/android.md) | Building the Android app, importing game files with the system picker, landscape and background pause, and the limits. |
| [Tidewater on the PS Vita](design/vita.md) | Building and installing the Vita app, where the game files go, and the limits. |
| [Cutting a release](release.md) | The develop and main branches, how the version is chosen, what gets published, and how self-update works. |

## Specifications

Specifications describe **what** the original game does, never how its code does it. They are the only source for engine behaviour, and each one is reviewed against the [clean-room rules](../CLEANROOM.md) before it is added. They live in [`specs/`](specs/), one folder per area.

| Area | What it specifies |
|---|---|
| [`specs/core`](specs/core/) | Timing and random numbers, unit slots and creation order, unit animation scripts, corpses and wrecks. |
| [`specs/game`](specs/game/) | The life of a game, setup from a map, victory and defeat, rescue, cheat codes, campaign flow (briefings and end screens), saving and loading, diplomacy. |
| [`specs/orders`](specs/orders/) | The order list, commands, pending orders and target acquisition; transports (boarding, landing, unloading). |
| [`specs/movement`](specs/movement/) | Tile steps and arrival, path search, occupancy and passability, placing a unit on a free tile. |
| [`specs/combat`](specs/combat/) | Damage, armour, attack timing and missiles; attack-ground and demolish. |
| [`specs/spells`](specs/spells/) | Casting, effects, durations and summoned units. |
| [`specs/economy`](specs/economy/) | Harvesting, construction, training, research and food, the tech tree, repair, walls. |
| [`specs/vision`](specs/vision/) | Fog of war, sight and detection. |
| [`specs/ai`](specs/ai/) | The computer player: its model, scripts, workers, training and research, army and attacks, spell casting. |
| [`specs/ui`](specs/ui/) | Screen layout and pointers, the map view, minimap, selecting and commanding, command buttons, the information panel, building placement, messages, the game menu, the multiplayer menus, lobby and score screen, sounds, music, palette effects. |
| [`specs/net`](specs/net/) | Lockstep turns, stalls, leaving and dropping players. |
| [`specs/oracle`](specs/oracle/) | The per-cycle trace format used to compare the simulation with recordings of the original game (`twtool oracle`). |

## Related files

| File | What it is |
|---|---|
| [`CLEANROOM.md`](../CLEANROOM.md) | The rules that keep the code our own work, and the record that shows it. |
| [`CLAUDE.md`](../CLAUDE.md) | Instructions for AI agents working in the repository, including the crate layout. |
| [`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md) | Licences of the third-party code linked into the programs. |
