# Network play: lockstep over TCP

**Status: early.** Two to eight Tidewater programs can play one game over TCP, started from the multiplayer menus or
from the command line. Every machine runs the whole simulation and exchanges only commands. The behaviour is
`docs/specs/net/lockstep.md` and `docs/specs/net/leave-and-drop.md`; the menus and lobby are
`docs/specs/ui/multiplayer-menus.md` and `docs/specs/ui/game-setup-lobby.md`. The wire format is Tidewater's own,
because the original's packets are out of scope (`lockstep.md`, "Scope").

## Play from the menus

Main menu, **Multi Player Game**: choose Enhanced or Classic, enter a name, then pick a connection method.

- **Local Network** (in the original's IPX place) lists the games found by the beacon (below) and has a Create Game
  button. **Direct IP** (in Direct Connect's place) asks for the host's address and port, or Create Game, which
  listens. **Battle.net** is drawn greyed out and cannot be chosen; there is no Modem row.
- **Create Game** opens the game setup lobby: the scenario, the eight rows (yourself, other players as they join,
  computers, closed rows), each player's race and chat box, and Classic mode's Ready boxes. **Start Game** is grey until
  at least two participants (the creator plus joiners or Computer rows; Available rows are ignored) are in, every joiner has the map and, in Classic mode, everyone is ready.
- **A joiner** picks a game (or types an address), is shown the host's map and rules in the lobby and takes the
  host's choices; it changes only its own race, Ready box and chat line.
- Code: `mpmenu.rs` (the menus), `mpwidgets.rs` (edit boxes and pop-ups), `lobbyscreen.rs` (the lobby), `net/room.rs`
  (the lobby's two ends, polled once a frame, no threads of their own beyond the handshake).

## Play over a LAN

The host starts the game and waits; each joiner connects to the host's address.

```sh
# Host (listens on port 6112 unless you name another):
tidewater 'Maps\Cramped BNE.pud' --host --lobby humans=2,races=human/orc,seed=1
# Joiner (the map, rules and options come from the host):
tidewater --join HOST-ADDRESS            # or HOST-ADDRESS:PORT
```

- **Write the map before `--host`.** A bare `--host` followed by a word reads the word as a port. `--host=7000` and
  `--host 7000` name a port.
- **Default port: 6112 (TCP).** Open it on the host's firewall; joiners connect out and need nothing open. The command
  line has no discovery: a joiner needs the host's address. (The Local Network menu finds games itself; see "LAN
  discovery".)
- **The host sets the lobby**; joiners accept it by joining. `--setup custom:classic,fog=off` picks the rules and the
  game's options; `--lobby` picks the rest (below). The host prints the game it hosts and each joiner prints the game it
  was shown, with Classic rules marked "opt-in".
- **Enhanced rules are the default.** Classic rules are only ever chosen on purpose, with `classic` in `--setup`.
- **Record each side** with `--record-replay` (the file is `p1-NAME-TIME.twrep`, `p2-...` and so on, in the replay
  folder). `twtool desync-diff A B` on two sides' files finds nothing when the game stayed in step.
- **Both programs must be the same build with the same game data.** The handshake refuses a joiner that differs and
  says which of the protocol, the simulation build, the data tables (`unitdata.dat`, `upgrades.dat`, `script.bin`,
  `ai.bin`), the game edition or the game files differs ("Compatibility check"). A joiner without the host's map is
  sent it ("Map transfer").

| `--lobby` choice | Meaning | Default |
|---|---|---|
| `humans=N` | Human players, 2 to 8. The host is slot 1. | 2 |
| `races=human/orc` | One race per human, in slot order, or `map` for the map's own. | the map's |
| `computers=N` | Computer rows after the humans. | 0 |
| `fixed` | Fixed Order: each human takes the start slot of its row. | off |
| `speed=0..8` | Game speed (8 is Fastest). | 8 |
| `row=0..15` | Network delay row, which sets the turn length (below). | 3 |
| `seed=N` | The game's seed. The host's choice, sent to every machine. | the host's clock |
| `observers=0..4` | Places for watchers (below, "Observers"). The game starts when they are filled too. | 0 |
| `extras=NAME=VALUE/...` | The client-only options that give an edge (C+L), for every player: `extras=health-bars=always/widescreen=16:9`. The host's own settings stand for those not named. | the host's settings |

## How it fits together

```
  game screen (game::tick)
        |  submit / ready_commands / checkpoint       crates/client/src/session.rs  (Session trait)
  NetSession (net/lockstep.rs)  -- turn engine: queue, turns, sync check, stalls, drops
        |  Wire trait: send / poll / forward          net/tcp.rs
  HostWire  <---- TCP, one connection per joiner ---->  ClientWire        net/proto.rs  (messages, framing)
```

- **Threads, not async.** Each connection has a reader and a writer thread feeding channels; the game thread never
  blocks on the network once a game runs. The handshake before the window opens does block, on purpose.
- **Star topology.** Joiners connect only to the host (a new one, if the host leaves: "When the host leaves"). The host stamps each packet with the slot it came from (a joiner
  cannot send as another player) and passes on what it accepts. The host is not a game server: it decides nothing about
  the game, every machine runs everything, and the only thing it alone decides is a drop (below).
- **The session is the only seam.** `NetSession` implements `Session`; `game::tick` gained a few calls (`observe_speed`,
  leave notices, the deadline reset) that other sessions leave at their defaults. A network game's replay is the merged,
  ordered commands the session handed out, so it replays on one machine through `ReplaySession`.

## The protocol

A frame is a `u32` little-endian length and that many bytes of `postcard`. At most 1 MiB. Variants are only added at
the end; `Hello` is first so a program of another protocol version can still read it far enough to refuse.

| Message | Direction | Carries |
|---|---|---|
| `Hello` | joiner to host | Identity (protocol, simulation build, data hash, edition, archives hash), the name asked for, and whether the joiner wants to observe |
| `Refuse` | either | Why the game cannot go on |
| `Welcome` | host to joiner | The joiner's slot and the whole game: map name and hash, seed, setup, lobby, speed, delay row, names |
| `Ready` | joiner to host | The game is acceptable here (the map is found, with the same hash) |
| `Start` | host to joiners | Every slot filled; play. Carries every slot's name |
| `Turn` | any | A player's packet: slot, sequence number, one optional command |
| `Sync` | any | The sender's state digest for a turn |
| `Drop` | host to joiners | Slot and the sequence number its packets stop at |
| `DropRequest` | joiner to host | Please drop this slot (Alt+F2) |
| `Lobby` | host to joiners | The whole lobby (map, rows, options), sent whenever it changes |
| `SetRace`, `SetReady`, `Say` | joiner to host | The joiner's race, Ready box and chat line |
| `Go` | host to joiner | Play: the joiner's slot and the game as the lobby settled it |
| `HaveMap` | joiner to host | Whether this machine has the lobby's map (by hash) |
| `MapRequest`, `MapOffer`, `MapChunk`, `MapDenied` | see "Map transfer" | The map, in chunks |
| `Chat` | any (the host relays) | An in-game chat line: sender slot, allies only or not, text |
| `Mesh` | host to all | The game's key; a joining player should open a socket for a new host |
| `Listen` | joiner to host | The port of that socket (0: none) |
| `Peer` | host to all | Where a player can be reached if the host goes |
| `Rejoin`, `Resume` | to and from the new host | The hand-over's handshake (see "When the host leaves") |
| `Resend`, `ResendSync` | to the new host | Another player's packet or digest the new host lacks |
| `Lost` | new host to all | A player that did not come back: dropped at a packet number, its digests no longer checked |

The protocol number is **8** (`net::proto::PROTOCOL`); it changes whenever a message or `Command` variant does. A
joiner of another protocol is refused with a message naming both versions: the host reads only the first varint of the
`Hello` to know that it cannot read the rest.

## The turn engine

- **Packets and turns** (`lockstep.md`, "Turns and the order of commands"). Every human has a queue of packets, one
  optional command each. A turn carries out one packet from every human, in slot order, each acting as the slot it came
  from; a remote command whose player is not its sender's slot is ignored. Turns 0 and 1 both run before cycle 0; after
  that a turn runs every K cycles. Surrender and Quit (draw) are never turn commands: they are local to the machine
  that made them.
- **Latency.** The queue is topped up to 2 at each turn start (limit 26, a full queue drops the command), so a command
  read at counter c runs in the turn before counter `K * ceil(c / K) + K`. The vectors in the spec (MP-L6 and the rest)
  are unit tests.
- **Turn length.** `K = ceil(T * 13 / L)` for Enhanced rules (L the speed's cycle length in ms) and `ceil(T * 60 / S)`
  for Classic. T comes from the delay row. The default is the spec's gap-fill for Direct IP, row 3 (K = 8 at speed 8, about
  104 ms a turn); the creator's row applies to every player. Row 4 (K = 12) is the traced Direct Connect game, which
  the tests use for the spec's vectors; `--lobby row=4` picks it. Local Network games use the spec's
  row 1 (the original's IPX row: K = 2 at speed 8, about 26 ms a turn).
- **Game speed.** The world starts at Normal. The lobby's speed reaches every machine as one `Command::Speed`
  command in the first turn (it carries the speed, not a direction), so the recording replays at the same speed. The
  speed keys send the current speed one up or down, and the Speeds dialog sends the slider's value; a value above the
  top speed is ignored by the simulation (`orders/orders.md`, NC1 and NC2). Turn 1's length already uses the lobby's speed.
- **Stalls** (`lockstep.md`, "Stalls"). A turn waits until every human's packet for it has arrived. Over 2 s the cursor
  is the watch; over 7 s a "Waiting for players" notice names who. Ctrl+F1 leaves the game (the others see a drop);
  Alt+F2 drops the players still missing, on every machine at the same turn. A wait over 200 ms resets the cycle
  deadline, so the game does not run a burst of cycles to catch up. A shorter wait is caught up.
- **Sync check, every turn, wider than the original's.** At the start of turn n each machine sends the digest of its game
  state (`World::state_hash`: players, units, map, vision, the random generator). At turn n + 1 the digests of turn n
  are compared. A machine whose digest differs is dropped, in place of its packet, with "Player X was dropped.", on
  the turn the original drops on (the second turn after the divergence). Each machine drops the other; neither keeps a
  game the other cannot be shown to share. Replays still carry the usual checkpoint every 100 cycles.

## LAN discovery

The Local Network game list finds games with a small UDP beacon. Code: `net/beacon.rs`.

- **Who sends.** The creator of a Local Network game, while its lobby is open and has a free row, once a second, to the
  broadcast address on **UDP port 6112**. A machine that only looks sends nothing. A Direct IP game is never announced.
- **Who listens.** The game list, from the moment it opens until it closes. Listeners share the port (`SO_REUSEADDR`,
  and `SO_REUSEPORT` on Unix), so two programs on one machine can both watch. A game stays listed 4 s after its last
  packet.
- **The packet** is built by hand (not postcard) so any later version can still read it. All numbers are little endian.

  | Bytes | Field |
  |---|---|
  | 4 | magic `TWLN` |
  | 1 | beacon version, 2 |
  | 1 | kind, 1 = announce |
  | 2 | the network protocol (8) |
  | 2 | the TCP port the lobby listens on |
  | 1 | players in the lobby |
  | 1 | rows that can hold a player |
  | 1 | free places for observers (0: none, or all taken) |
  | 1 + n | the creator's name, at most 24 bytes of UTF-8 |
  | 1 + n | the map's name, at most 48 bytes of UTF-8 |

- **Foreign packets are ignored.** Anything that is not exactly this layout (another program's, another version,
  truncated, trailing bytes, bad UTF-8, port 0) is dropped silently. The sender's address is the game's address. A beacon
  only says where to connect: the TCP handshake still decides whether the join happens, so a forged beacon can cost a
  joiner a refused connection and nothing more.
- **Switch it off** with `TIDEWATER_NO_BEACON=1` (the tests and scripted runs do). Some networks drop broadcasts; Direct
  IP works there.
- **Test.** The packet round trips and the foreign-packet cases are unit tests. A real broadcast test runs only with
  `TIDEWATER_TEST_BROADCAST=1`, because a CI container may have no broadcast route.

## Compatibility check

The `Hello` carries an Identity. The host compares it with its own and refuses with the first difference, in this order:

| Difference | Message |
|---|---|
| Protocol | "different network protocol versions (host N, joiner M)" |
| Simulation build | "different game builds" |
| Mods | "different game data or mods (host mods: ...; joiner mods: ...)" |
| Data tables | "different game data or mods (the unit, upgrade, script or AI tables differ)" |
| Edition | "different game editions" |
| Archives | "different game files (the two installations' archives differ)" |

- **The edition and the archives hash come from the import manifest** (`<data>/game/manifest.json`, read by
  `tidewater_import::Manifest`): its edition name, and a hash of the path and SHA-256 of every `.mpq` it lists. It is
  never 0 and is compared only when both sides have a manifest, so an installation without one is judged by the data
  tables alone.
- **Mods** (protocol 9). The identity carries a hash of every file of the enabled mods (`ModSet::hash`: names, lengths
  and bytes, in mod order; 0 with no mod) and the mods' names and versions. The hash is always compared and the names
  only appear in the message. The data-table hash reads through the same reader as the game, so a table mod changes it
  too; the mods difference is reported first because it explains the table difference. See [../mods.md](../mods.md).
- **The refusal reaches the person.** The joiner's menu shows the host's message in the error box; a command-line
  joiner prints it.

## Map transfer

A joiner without the host's map (the same file contents, by hash) is sent it by the host. Code: `net/room.rs`,
`lobbyscreen.rs`.

1. The joiner answers every `Lobby` with `HaveMap { hash, have }`. The host's Start button stays grey while any joiner
   says no (`StartError::MissingMap`).
2. A joiner that lacks the map sends `MapRequest { hash }`. The host answers `MapOffer { hash, size }` and then
   `MapChunk { hash, offset, bytes }` in order, **16 KiB** each; or `MapDenied { hash, reason }`.
3. **Limits.** At most **4 MiB** (a larger map is never offered); at most **3 requests** per joiner per map; the host
   sends only the map it has open. The joiner checks that the size fits, that chunks arrive in order and add up to
   the size, that the content hashes to the lobby's map hash (`tidewater_replay::start::map_hash`) and that it parses
   as a map, before it keeps anything. A failed transfer is shown in a notice and nothing is written.
4. **Where it goes.** `Storage::write_received_map` writes `<user dir>/received-maps/<name>-<hash>.pud`. The map is
   never added to the installation. At Go the joiner plays from that copy; the host's name for the map means nothing there.
5. The Scenario box shows "Asking for the map..." and "Receiving the map... N%" while it happens (a gap-fill).

## Chat

An in-game chat line is a `Chat` message, not a command: it is in no turn and not in the state hash, so chat cannot
cause a desync. Code: `chat.rs`, `net/lockstep.rs`.

- **Sending.** Enter opens the chat line, Enter sends it. A line that is a cheat word is a cheat; any other line goes
  out as chat. A joiner sends to the host, which stamps the sender's slot and relays to the other joiners.
- **Recipients** (the messages spec leaves them open; a gap-fill): everyone, or only the sender's allies when the line
  starts with `/a ` or `/allies `. Each receiving machine checks the sender against its own alliance table.
- **Showing.** A received line is "name: text" in the message list, colour 2. Under Enhanced rules the sender's own line
  is added at once; under Classic rules the sender sees the "chat sent" notice (network entry 66).
- **Filtering.** Both ends keep printable ASCII only and at most 99 characters, whatever the sender's program did.
- **The lobby has its own chat**: each row's chat box, sent on Enter (`Say`).

## Diplomacy

`Command::Diplomacy` (`game/diplomacy.md`) carries the sender's whole alliance row toward players 0 to 7, its
shared-vision set and its allied-victory setting in its own packet, so it is a turn command and is synced like any
other. A remote diplomacy command whose player is not its sender's slot is dropped by the same slot filter as every
command, which is DP2 (nobody changes another player's settings). The client has no diplomacy screen yet: a chat line
starting `/diplomacy [allies=0,2] [vision=0,2] [victory=on|off]` sends it, keeps the sender in its own set and allied
with itself, and leaves out fields at their current value. The sets and settings are in the state hash and the save.

## Holding workers while a building is placed

`Command::HoldWorkers` and `Command::ReleaseWorkers` (`orders/orders.md`, "Holding and releasing workers") name only the
sender, and act on the sender's selected workers, so each is a turn command like any other and travels in the sender's
packet. The client sends Hold when it enters building placement and Release when it leaves placement without placing
(Cancel, a right-click, a click off the map, the game menu, a lost selection). Placing the building sends the build
command, and every unit command and every build releases the sender's selected held workers first, so the client sends
no Release for those. A held worker freezes at its next animation step, which keeps it from walking into the site
(single player and multiplayer alike). Both commands are in the replay format (since version 5) and the state hash covers the
frozen state, not the save (a load releases every unit).

## Pause counts and leave messages

- **Pause and Resume** are separate synced commands (`Command::Pause`, `Command::Resume`), so a pause while paused and
  a resume while running do nothing. The pause key, button and the phone's background pause send Resume while the game
  is paused.
- **The game menu does not stop a network game** (`ui/game-menu.md`, "Multiplayer"): the game goes on underneath for
  the player who opened it, with that player's game input off (`core::game_ticking`), so nobody waits for them.
- **Enhanced multiplayer gives each player 3 pauses** (`World::pauses`). A pause with none left does nothing. The counts
  join the state hash in that mode only, and Enhanced saves keep them.
- **Notices** (Enhanced multiplayer only): network entry 86 with the pauser's name and the pauses left, and entry 87 with
  the resumer's name. Single player and Classic show none.
- **Leave notices.** Entry 64 for a player who left, 65 for one who was dropped, and entry 63 for either when the player
  has no units or buildings left.

## Leaving and dropping

- **Left.** Surrender or quit sends a `Leave` command in the leaver's own packet. Every machine carries it out in the
  turn and shows "Player X has left the game." (`network.tbl` entry 64). `World::leave_game` makes the slot nobody;
  Enhanced rules also clear the leaver from shared vision, Classic mode only empties the slot.
- **Dropped.** A drop is a `Leave` made up in the missing player's place, shown as "was dropped" (entry 65). It comes
  from the sync check, a lost connection, Ctrl+F1 or Alt+F2. The host numbers it into the dropped player's packet
  stream (`Drop` with the count of that slot's packets it accepted), so every machine carries it out in the same turn.
  The host only passes on packets it counted, which is what makes the count exact.
- **The dropped machine** is told, ends its game with a surrender and shows that it left.

## When the host leaves

`net/leave-and-drop.md`, "When the host leaves": the first remaining player who can host becomes the host and the game
goes on. In Tidewater's star the host also relays every packet, so the remaining machines hand the relay on to that
player and reconnect to it. Code: `net/tcp.rs` (the mesh table, dialling, taking over), `net/lockstep.rs` (the rejoin).

- **The mesh table, at the start.** In a game with three or more humans, or with an observer, the host sends every
  machine `Mesh` with a random key for the game. Each joining player then opens a listening socket on a free port and
  answers `Listen` with its port; the host sends everyone `Peer` with that player's address (the address the host sees
  its connection come from, and that port). A two-player game without observers sends none of this and opens no socket.
- **Who takes over.** When a machine loses the host's connection, it takes the players still in the game as it sees
  them, in slot order, without the host: the spec's "first remaining connected player who can host" (observers never
  host, nor a player whose socket could not open). It tries them in order: connect (3 s), send `Rejoin` with the key,
  wait for `Resume` (5 s). A player that has left has closed its socket, so the connection is refused and the next one
  is tried. A machine that reaches its own slot takes over: its wire becomes a host's wire that accepts on its socket.
  This runs on a thread of its own; the game waits for the turn as for any late player ("Waiting for players").
- **No command lost or run twice.** Every machine keeps the last 256 turns of every player's packets and digests, its
  own included. `Rejoin` carries the next turn, how many packets of each player this machine holds, and the drops it
  was told of. The new host answers `Resume` with its own counts and sends the rejoiner what it lacks (packets in order,
  digests, drops); the rejoiner sends back (`Resend`, `ResendSync`) what the new host lacks, which passes it on to the
  others. Packets are numbered per player, so a copy that arrives twice is ignored by its number.
- **The players who did not come back.** Once every player it expects has rejoined, or after 10 s, the new host drops
  the old host and every player that did not rejoin, each at the most packets any machine held of it (`Lost`). Every
  packet before that number is somewhere among the machines that rejoined, and a machine that has already run a turn
  used only packets before it, so every machine runs the same turns. A player whose leave the new host already holds is
  not waited for. A lost player's digests are no longer checked (some of them may have died with the host's machine);
  this changes a turn only if that player was already out of step.
- **What the players see.** A host that quits sends its leave first, so the others show "Player X has left the game."
  (entry 64); a host whose connection just ends is "was dropped" (entry 65). The machines then show "X is now the host."
  (our notice; the spec has none), after that leave notice; a player left on its own sees none. From then on the new
  host is the one the game menu asks for ("Host rights").
- **Observers** reconnect to the new host the same way and get the same catch-up. The players never wait for them.
- **When it can't be done.** If the table never arrived, or no player can be reached (a firewall, a NAT), a machine
  falls back to the old behaviour: it drops everyone else where it stands, plays on alone, and says "Nobody could take
  over from the host: you play on alone." A new host that nobody reaches drops the others after 10 s and says the same.
  Two groups can then form, each in step within itself (the machines that reached one player, and the ones that did not).
- **Not handled.** A host that leaves at the very start, before the table arrived (fallback above); an observer that
  ran a turn past the drop number before it rejoined (it would leave step; it only watches); a player the old host had
  dropped but not yet told, who then takes over (two groups, as above). The rejoin trusts the key: any machine of the
  game could forge another's packet during the rejoin, as it could already lie about its own.

### Host rights

`ui/game-menu.md`, "Who may restart, return or load", and `game/save-load.md`: in a network game only the host may
choose Return to Chat Room, Save Game or Load Game; anyone else gets network entry 28 with the host's name, in a box with
an OK button. The host is the current one (the new host after a hand-over). While there is none (between losing the host
and the new one, or after the fallback) the check lets anyone through, as the spec says for a game with no host. Return
to Chat Room goes to the main menu whoever chooses it, since Tidewater has no chat room. An observer is never refused:
its choices touch only its own machine. Code: `Session::host`, `menu::host_check` (LV13 is its test), `gamemenu::gate`.

## Observers

A host can leave places for watchers: `--lobby observers=N` (0 to 4, off by default) and a watcher joins with
`tidewater --join HOST-ADDRESS --observe`. The menus have them too, as our own addition (the original has no
observers):

- **In the menu lobby** the host picks the number of places in the "Observers" box under the player rows (`LobbyView::observers`,
  `HostRoom::set_observers`; none by default). Lowering it sends the watchers beyond it away. Joiners see the box
  read-only once the game has places, and the names of the watchers on a line under the rows (`LobbyView::watchers`). The
  box and the line are drawn only when they have something to say for a joiner, so a lobby without observers looks as the
  original's. Unlike the command-line host, the menu lobby starts when the host presses Start, with whoever is in: the
  places are limits, not seats to fill.
- **On the game list** a **Watch Game** button sits under Disconnect, for a game that may have a place. The Local Network
  beacon carries the free places (`Announce::watch`, beacon version 2), so the list knows; Direct IP cannot know before
  it connects, so the button shows and the host refuses with "this game has no place for an observer" when it has none.
  `room::handshake_as` sends the Hello with `observer` set; an observer's number is `OBSERVER_BASE` plus its place.
- **An observer in the lobby** has no row, race, Ready box or chat line (its race box reads "Observer"). It needs the
  map like a player and asks for it the same way; the host does not start while an observer lacks it.
- **The host decides.** A host without places refuses an observer with "this game has no place for an observer", on the
  command line and in the menu lobby alike. A player who joins when the players are all in is refused as full. A
  command-line host starts when every player slot and every observer place is filled.
- **Slots.** An observer takes slot 8 plus its number (`OBSERVER_BASE`). It is not in the lobby's humans, so it has no
  start position, no name in the players' list, and no part in victory or defeat.
- **It runs the sim.** The host relays every `Turn` to it, as to any joiner, and it steps the same cycles. It starts from
  the host's player, so its world is the one the host's machine builds.
- **It sends nothing.** Its session submits no command, sends no `Turn`, `Sync`, `Chat`, ping or leave, and never needs
  a turn from itself. It does not wait for the others' sync checks, since it has no state digest of its own to send. The
  host ignores any message from an observer's link and does not treat its leaving as a player leaving. The players never
  wait for it.
- **It sees everything.** `World::observing` is display only, not game state, and is never hashed. `reveal_view()` (the
  reveal-map cheat or an observer) makes the map, the minimap, buildings and units draw with no fog and no remembered
  ghosts, and lets the mouse pick any unit. The sim itself reads only `rules.reveal_map`, because that setting changes
  which units are hidden and so what a command may target. An observer's game therefore stays in step with the players'.
- **The interface.** The HUD is the host player's, the Surrender button is hidden on the end screen, and the victory and
  defeat dialogs are not shown. Its notice line tells a game's end as it does for anyone else ("The game has ended",
  "Player N has been defeated"), never as a "Victory!" of its own, and shows none of the watched player's own notices
  (`messages::local_event_notice`).

What is left: joining after the game has started (an observer attaches only before the start), a view with the choice
of player, and a spectator interface of its own.

## Spec gaps and choices

- **C+L options are the host's.** The lobby carries the host's simulation options (`tidewater_sim::Extras`) and the
  client-only options that give an edge (`GameConfig::client_extras`: health bars, smart attack-move, widescreen, any
  C+L row of the registry). `NetSession::lobby_client_extras` returns them, so every player, observers included, plays
  with the host's choice (`docs/design/extras.md`, "Multiplayer (C+L)"). They are not in the replay header. Smart
  attack-move changes the commands a player sends, but the commands still travel as ordinary commands, so peers stay in step.

- **The host leaving** is "When the host leaves" above: the original's players each hold every connection and have no
  host to lose, so handing the relay on is Tidewater's own mechanism for the spec's rule.
- **The "Waiting for players" dialog** is a line on the notice bar; the dialog is a lobby-phase screen.
- **Entry 63** shows when a leaving player has nothing left, judged on the machine that shows it at the time the leave
  notice appears; the victory check still reads the local player in places (the open questions in
  `multiplayer-determinism.md`).
- **Lobby gaps.** Start needs at least two participants, so the creator alone with one Computer row can start (the turn engine runs with no peer and opens no hand-over table); the Cheat Codes box is cosmetic; Random races are settled by the host
  from the seed; rows are packed into slots even in Fixed Order; a network game starts without the sound fade; the lobby
  reuses the menu art, so the logo shows behind the rows and the version line is hidden; Start is greyed, not hidden,
  when it cannot be used.
- **The player list** (a per-player summary screen) is not built.
- **IPv6** works for `--join [::1]:PORT`, and the listener binds IPv4 only.

## Tests

- **Unit tests** (`net/proto.rs`, `net/tcp.rs`, `net/lockstep.rs`, `net/lobby.rs`, `net/room.rs`, `net/beacon.rs`,
  `mpmenu.rs`, `lobbyscreen.rs`, `chat.rs`) cover the message round trips, the handshake and its refusals, the
  turn-length tables, the latency vectors, stalls with an injected clock, drops, leaves and the sync check, on a
  stand-in world over loopback TCP; the lobby's two ends, map transfer and its limits, chat relay, the beacon packet
  and each menu screen.
- **Frame snapshots** (`tests/frames.rs`, scenes `multi` and `lobby` in `script.rs`) are the menus and the lobby drawn
  on synthetic art, compared as text; bless them with `TIDEWATER_SNAPSHOT_BLESS=1` after a deliberate change.
- **`crates/client/tests/room.rs`** runs the lobby over loopback with each machine on its own thread and no game files:
  two joiners entering, races and chat, a map sent in chunks and checked, Start held back until it arrives, and the
  refusals for another game data table, edition or archives.
- **Observer tests** are in `net/tcp.rs` (the handshake, the refusals, relayed turns, an observer not heard),
  `net/lockstep.rs` (two players and an observer stay in step and the observer sends nothing), `net/lobby.rs`,
  `net/room.rs` and `sim/vision.rs` (the observer's view changes nothing the sim hides).
- **`crates/client/tests/mp_e2e.rs`** is the end-to-end check: separate `tidewater` programs on one machine, each in
  its own folder (a data folder linking to the game files, its own settings, saves and recordings), each running a
  scripted scene (`mp-*` in `script.rs`) through the real menus, headless. Seven games, one after another: Local Network
  found on the game list (two players), Direct IP with the address typed, Classic rules with Ready, an observer joining
  a two-player game with Watch Game, and three players where the host leaves first, the two joiners hand the game on
  and play on together, and then the new host leaves; the creator alone with one Computer row; and two players with a Computer. In each the creator
  picks a map and sets rally points and smart attack-move in the Extras box; everyone gives orders, chat goes both
  ways and a player leaves. The test then checks each program's `<scene>-events.txt` (the screens it reached, its seat,
  the options in force, the messages it showed) and compares the recordings cycle by cycle as `twtool desync-diff`
  does. Below, "Running the end-to-end check".
- **`crates/client/tests/net.rs`** plays real games between two to four machines in one process over loopback TCP on the
  smallest installed map, with scripted commands from every side: hashes at every turn, recordings compared with the
  same checks as `twtool desync-diff`, a forced divergence, a delayed peer, a joiner with a different map, Classic
  rules, and the host leaving three- and four-player games (quitting, and its connection dying) with the others in step
  to the end. It needs `TIDEWATER_INSTALL` and does nothing without it.
- **Hand-over tests** are in `net/lockstep.rs` (three and four players, the host quitting or its connection dying, a
  second hand-over, the host and the next in line leaving at once, an observer following the new host, a two-player
  game unchanged), `net/proto.rs` (the messages) and `menu.rs` (entry 28 follows the host).

### Running the end-to-end check

```sh
TIDEWATER_INSTALL=<data>/game cargo test --release -p tidewater-client --test mp_e2e
```

- **About 30 seconds in a release build.** A debug build draws every frame unoptimised, so the same seven games take
  about four minutes there (they run with the other client tests). `TIDEWATER_MP_E2E_TIMEOUT` (seconds) changes how
  long a game may take.
- **Skipped without game files** (`TIDEWATER_INSTALL` unset), which is how CI runs it. The Local Network games also
  skip, with a message, when a broadcast beacon does not come back to this machine (a container with no broadcast
  route); Direct IP still runs there.
- **It uses the real ports**, TCP and UDP 6112, so the games run one at a time and nothing else on the machine may
  hold port 6112. While a lobby is open its beacon goes out on the machine's network like any Local Network game's.
- **What a run leaves**, in `target/tmp/mp-e2e/<game>/<scene>/`: `out/<scene>-events.txt`, the shots as
  `out/<shot>-composed.png` (the game list, the lobbies, the game on each side), `out/save/replays/*.twrep` and the
  program's `log.txt`. A failure prints every program's events and log.
- **By hand**, each scene runs on its own: start the creator's first, then the others, each with its own data folder
  (holding `game`) and output folder, for example
  `TIDEWATER_SCRIPT_PICTURES=shots tidewater --data-dir A --script mp-lan-host --script-out A/out --headless --record-replay`
  and the same with `mp-lan-join` and `B`. Leave out `--headless` to watch them in windows.
