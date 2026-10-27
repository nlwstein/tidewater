# Design: multiplayer determinism

**Bottom line.** Two machines that run the same commands from the same start compute the same `World::state_hash` every
cycle, whichever player is local and whatever the pointer width. This is tested. Three reads of the local player still
change game state in a network game; they need a spec ([Open questions](#open-questions)).

## What makes the sim deterministic

- **Integer state only.** The sim has no floats, no clock and no `HashMap` or `HashSet`; every container iterates in a
  fixed order.
- **One random generator.** `World::rng` is seeded from the game's seed and drawn in the order the timing spec fixes.
- **One input.** The only thing that changes game state between cycles is the list of `Command`s passed to
  `World::step`. A `Command` is serialisable (`serde`), so a network turn can carry it.
- **A pinned hash.** `state_hash` is FNV-1a over the game state. Lengths and enum discriminants (`usize`, `isize`) hash
  as 64-bit little-endian numbers, so a 32-bit device matches a 64-bit one. `the_state_hash_is_pinned` in
  `crates/sim/src/world.rs` fixes one value; change it only on purpose.
- **A perspective test.** `crates/sim/tests/perspective.rs` runs one two-player game twice, with each player local in
  turn, feeds both runs the same scripted commands and compares the hash after every cycle (4,000 cycles). It needs
  `TIDEWATER_INSTALL` and skips without it.

## Rules for new code in `crates/sim`

1. **No `HashMap` or `HashSet`.** Use `BTreeMap`, `BTreeSet` or a sorted `Vec`.
2. **No floats, clocks or threads** in anything that touches game state.
3. **No local-player reads for game state.** `vision.local` (and `Started`, `Setup::local`) may drive what the
   player sees and hears, never what happens. If a spec says a rule depends on the local player, add the open-question
   comment below and list it here.
4. **Every mutation is a `Command`.** The client never writes to the world between cycles. A new player action gets a
   `Command` variant, handled in `World::apply`, that checks the sender owns what it touches and ignores bad ids or
   slots (a remote peer sent it).
5. **Anything hashed must be derivable from the commands.** A field the hash skips (sound cues, minimap memory, flashes,
   `Cheats::used`) is per machine and must never feed back into hashed state.

Mark a game-state read of the local player that the specs don't settle with
`// Network games: open question (see docs/design/multiplayer-determinism.md)`.

## Every read of the local player in `crates/sim`

| Where | What it does | Class |
| --- | --- | --- |
| `sound.rs` (`local`, `local_owner`, `unit_place`, `construction_sound`, `ready_sound`, `acknowledge`) | Picks which sounds play | Display only |
| `minimap.rs` (`mark_attacked`, `remember_building`, `remember_looks`, `forget_seen_buildings`, `on_minimap`) | Attack marks, remembered buildings, minimap dots; none is hashed | Display only |
| `rescue.rs` (`flash_pass`, `flash_colour`) | Selection-box flash; `Flash` is not hashed | Display only |
| `vision.rs` (`local_in`, `on_minimap`, explored and visible maps) | Display maps for the local player | Display only |
| `site.rs` `bad_site_tiles` | Marks tiles on the placement overlay | Display only |
| `setup.rs` `create_map_units`, `starting_view` | The starting view | Display only |
| `cheats.rs` `cheat_command` | Sets `Cheats::used`, per machine and not hashed | Display only |
| `vision.rs` `set_fog_of_war` | Rewrites the "seen" bits (hashed) | Game state, single player only; the client skips it when `rules.multiplayer` |
| `sound.rs` `select_sound` (critter kill) | The 10th click kills a critter, with random draws | Game state, single player only; `single_player()` is false when `rules.multiplayer` |
| `victory.rs` `start_campaign` | Campaign mission set-up | Game state, campaign only (single player) |
| `site.rs` `site_check`, step 3 "Unexplored" | Refuses a site the local player hasn't explored, when the builder is theirs | **Game state, open question** |
| `victory.rs` `check` | Checks win and loss for the local player and knocks the others out | **Game state, open question** |
| `setup.rs` `Start::alliances` | A rescue-active slot starts allied with the local player only | **Game state, open question** |

## Writes to the world outside the command stream

| Where | Status |
| --- | --- |
| Passenger click, `game.rs` `tick` | `Command::UnloadOne`, applied first in the cycle |
| `set_fog_of_war`, `gamemenu.rs` | Skipped in network games (the spec disables the radio there) |
| `start_cheats`, `cheats.rs` | Skipped in network games; network cheats use `Command::Cheat` |
| Critter kill, `select_sound` | Never runs in network games |
| Surrender, `gamemenu.rs` (`Confirm::Surrender`, `QuitDraw`) | `Command::Surrender` and `Command::QuitDraw`, applied at once through `Sim::apply_now`. What a *network* surrender does (leave notices, allies) is still `TODO(multiplayer)`, `docs/specs/net/leave-and-drop.md` "Surrender and quit (draw)" |
| Demo options (`--demo-move`, `--demo-outcome`, `--demo-fight`) | Development only; they never run in a network game, and a run that uses one is not recorded |
| Fog switch in Preferences, critter kill | Not reproducible by a replay. The recording notes the event (`Session::note_unsynced`), and playback prints the note |
| `prepare_save`, `restore` | Single-player save and load only so far. Network saves (`docs/specs/game/save-load.md`) need a review for local-player reads before they ship |

## The session layer

`crates/client/src/session.rs` sits between the game screen and the sim. Each cycle `game::tick` hands the commands the
local player gave to `Session::submit`, then asks `Session::ready_commands(tick)` what to pass to `World::step`.
`None` means "do not step yet".

- **`LocalSession`** returns exactly what was submitted, the same cycle. With `--record-replay` it also writes each
  cycle's commands and a checkpoint to the replay.
- **`ReplaySession`** ignores `submit` (except pause and speed, which hold or rate-limit the viewer) and returns the
  recorded commands for the cycle. It compares each checkpoint with the recorded hash and stops at the first mismatch.
- **`NetSession`** (`docs/design/network.md`) plugs in behind the same trait: `submit` queues the command for the next network turn,
  `ready_commands` returns `None` until every player's turn packet has arrived and then the slot-ordered commands of
  the cycle (`docs/specs/net/lockstep.md`). `Session::immediate` is for commands that must take effect at once; a
  network session returns `None` there and carries the command in a turn (except leaving, which is local).

## The replay format

A `.twrep` file (`crates/replay`) is: the magic `TWREPLAY`, a `u16` format version (`tidewater_replay::format::VERSION`, currently 7; version 6 files still load), the body length as `u64`, the
body, and an FNV-64 checksum of everything before it (all little-endian). The body is a `postcard` encoding of
`Replay`: a header (sim build, map file name, FNV-64 of the map bytes, seed, setup, lobby, rules, start cheats, local
slot, game data identity), the commands by cycle, the checkpoints (`state_hash` after every 100th cycle) and notes about out-of-band
events. A cycle is named by `World::tick` before the step. The map hash is only 64 bits, enough to catch the wrong file
but not an attacker; a stronger hash is a version bump away.

**Game data identity (format version 7).** The header also records which game data the game ran on
(`tidewater_formats::identity::DataIdentity`): the FNV-64 data hash the multiplayer lobby compares
(`unitdata.dat`, `upgrades.dat`, `script.bin` and `ai.bin`, each with its name and length), and the active mods (their names, versions and the hash of every file, `tidewater_formats::mods`, [mods.md](../mods.md)). Playing a replay compares the recorded identity with the installation's first (`Header::check_data`, run by
`new_world`, `twtool replay` and `tidewater --replay`) and stops with "this replay was recorded with different game
files" and which part differs: the data hash (other game version or edition: the tables are not the same) or the mod
set. Without that check the only symptom is an opaque out-of-sync report at the first checkpoint. `twtool replay
--force` and `tidewater --force-replay` play it anyway, with a warning. A version 6 file has no identity; it loads with
the identity unknown, nothing is compared, and the checkpoints decide as before. New replays always record it.

## Finding a desync

`twtool desync-diff A B` (or `A --local SLOT`) runs both replays cycle by cycle, prints the first cycle whose state
hashes differ, then compares players, units, map and vision piece by piece (`World::first_difference`) and prints the
first differing field. Run the same replay from two seats to check that no code reads the local player.

## Open questions

- **"Unexplored" in network games.** Does the builder's own machine refuse a site, or does the check go away? As it
  stands, the owner's machine can refuse a site that its peers accept. In-turn build commands skip the check
  already; the later check when the builder arrives does not.
- **The victory check in network games.** `check` finishes the game for the local player only. A network game needs
  one answer per player, the same on every machine, including the order in which players are knocked out.
- **Rescue alliances.** Whether a rescue-active slot allies with every human or with the first one.

The perspective test avoids all of these on purpose: no player is knocked out, the skirmish map it picks has no rescue
slots, and every building site is next to the player's hall.
