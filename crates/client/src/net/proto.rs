//! The wire protocol: what two Tidewater programs say to each other (`docs/design/network.md`).
//!
//! The original's packet layouts are out of scope (`net/lockstep.md`, "Scope"): this is Tidewater's own protocol. A
//! frame is a `u32` little-endian length and that many bytes of [`postcard`] encoding of one [`Msg`]. The first thing
//! a joining program sends is [`Msg::Hello`], which is the first variant on purpose: a program speaking another
//! [`PROTOCOL`] can still decode it far enough to say why it is refused. Variants are only ever added at the end.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};
pub use tidewater_formats::identity::{ModTag, data_hash, describe_mods, fnv_extend};
use tidewater_sim::Command;
use tidewater_sim::setup::{Lobby, Setup};

/// The protocol version. Two programs with different versions refuse each other at the handshake.
///
/// Version 2: the identity names the game edition and the archives' contents (from the import manifest), and the lobby
/// can send a map to a joiner that lacks it. Version 3: the optional improvements that change the simulation
/// (`tidewater_sim::Extras`) have fields and travel in the lobby, and the rally command exists. Version 4: Shift-queued
/// orders (`Command::Queued`, one more byte in `Extras`) and observers (`Msg::Hello` says whether the joiner only
/// watches, and observers have slots from [`OBSERVER_BASE`] up). Version 5: the diplomacy command exists
/// (`game/diplomacy.md`). Version 6: the Hold workers and Release workers commands exist, and the speed command carries the
/// new speed instead of a direction. Version 7: the lobby also carries the host's choice of the client-only options that
/// give an edge (**C+L**, `crate::extras`: [`GameConfig::client_extras`], [`LobbyView::client_extras`]), so every
/// player runs the game with the same ones, and the menu lobby has places for observers ([`LobbyView::observers`],
/// [`LobbyView::watchers`]). Version 9: the identity also carries the enabled mods ([`Identity::mods`],
/// [`Identity::mods_hash`]).
pub const PROTOCOL: u16 = 9;

/// The first slot number of an observer: a game has eight player slots (0 to 7), so observers are 8, 9 and so on. An
/// observer's slot only names its connection; it is in no turn and no state.
pub const OBSERVER_BASE: u8 = 8;

/// The most observers one game takes.
pub const MAX_OBSERVERS: usize = 4;

/// The largest map a creator sends or a joiner accepts, in bytes. The installation's biggest maps are well under a
/// megabyte.
pub const MAX_MAP_BYTES: usize = 4 << 20;

/// The size of one map chunk.
pub const MAP_CHUNK: usize = 16 << 10;

/// The port a game is hosted on when `--host` names none, and the one a joiner tries when the address has none.
pub const DEFAULT_PORT: u16 = 6112;

/// The largest frame accepted: a command is a few hundred bytes at most, a configuration a few kilobytes.
pub const MAX_FRAME: usize = 1 << 20;

/// Who a program is, compared at the handshake. Two machines play together only if all three match.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// [`PROTOCOL`].
    pub protocol: u16,
    /// The simulation build (`tidewater_replay::format::sim_build`): a different build may compute a different game.
    pub sim_build: String,
    /// A hash of the data tables the simulation reads (unit, upgrade, script and AI tables). Different game data
    /// computes a different game.
    pub data_hash: u64,
    /// The edition the import manifest names (empty when the installation has no manifest, as when it was not
    /// imported). Compared only when both sides have one.
    pub edition: String,
    /// A hash of the manifest's archive entries (their names and SHA-256), 0 when there is no manifest. Compared only
    /// when both sides have one: two copies of the same game files hash alike.
    pub archives: u64,
    /// A hash of every file of the enabled mods, 0 with no mod (`tidewater_formats::mods::ModSet::hash`). Compared
    /// always: mods change what the game loads even where the tables stay the same (pictures, sounds, maps).
    pub mods_hash: u64,
    /// The enabled mods, in order, to name them to the player in a refusal. Not compared on their own: the hash decides.
    pub mods: Vec<ModTag>,
}

impl Identity {
    /// This program, over data hashing to `data_hash`, with no import manifest.
    pub fn ours(data_hash: u64) -> Identity {
        Identity {
            protocol: PROTOCOL,
            sim_build: tidewater_replay::format::sim_build(),
            data_hash,
            edition: String::new(),
            archives: 0,
            mods_hash: 0,
            mods: Vec::new(),
        }
    }

    /// This identity with the enabled mods: their hash and names.
    pub fn with_mods(mut self, mods_hash: u64, mods: Vec<ModTag>) -> Identity {
        self.mods_hash = mods_hash;
        self.mods = mods;
        self
    }

    /// This identity with what the import manifest says: the edition, and `archives` (see
    /// [`manifest_archives`]).
    pub fn with_manifest(mut self, edition: &str, archives: u64) -> Identity {
        self.edition = edition.to_string();
        self.archives = archives;
        self
    }

    /// The refusal for a joiner of another protocol, in words for the player.
    pub fn protocol_reason(host: u16, joiner: u16) -> String {
        format!(
            "different network protocol versions (host {host}, joiner {joiner}): both players need the same version of Tidewater"
        )
    }

    /// Why `other` cannot play with `self`, if it cannot, in words for the player.
    pub fn mismatch(&self, other: &Identity) -> Option<String> {
        if self.protocol != other.protocol {
            return Some(Identity::protocol_reason(self.protocol, other.protocol));
        }
        if self.sim_build != other.sim_build {
            return Some(format!(
                "different game builds (host {}, joiner {})",
                self.sim_build, other.sim_build
            ));
        }
        if self.mods_hash != other.mods_hash {
            return Some(format!(
                "different game data or mods (host mods: {}; joiner mods: {})",
                describe_mods(&self.mods),
                describe_mods(&other.mods)
            ));
        }
        if self.data_hash != other.data_hash {
            return Some("different game data or mods (the unit, upgrade, script or AI tables differ)".to_string());
        }
        if !self.edition.is_empty() && !other.edition.is_empty() && self.edition != other.edition {
            return Some(format!(
                "different game editions (host {}, joiner {})",
                self.edition, other.edition
            ));
        }
        if self.archives != 0 && other.archives != 0 && self.archives != other.archives {
            return Some("different game files (the two installations' archives differ)".to_string());
        }
        None
    }
}

/// The game the host sets up and every machine starts the same way. This is the lobby of `ui/game-setup-lobby.md`
/// as far as this phase has one: given on the host's command line, and shown to the joiners as the host chose it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GameConfig {
    /// The map as the installation (or the host's user map) names it. A path never travels.
    pub map_name: String,
    /// FNV-1a 64 of the map file (`tidewater_replay::start::map_hash`); a joiner without that exact file refuses.
    pub map_hash: u64,
    /// The game's seed, chosen by the host (the seed exchange).
    pub seed: u32,
    /// Enhanced or Classic rules, and the game's options (fog of war among them).
    pub setup: Setup,
    /// The humans' slots and races, the Computer rows, Fixed Order. `local` is the receiver's own slot, set when the
    /// receiver reads it.
    pub lobby: Lobby,
    /// The game speed (0 to 8) the world starts at: the lobby's "Game Speed".
    pub speed: u8,
    /// The network delay row (`net/lockstep.md`, "Which row"): sets the turn length.
    pub row: u8,
    /// The options that touch the simulation.
    pub extras: tidewater_sim::Extras,
    /// The host's choice of the C+L options (`crate::extras::ClientExtras::lobby_values`): the values that are not the
    /// default, by name. Every machine, observers included, plays with these whatever its own settings say; empty means
    /// every one at its default.
    pub client_extras: Vec<(String, u8)>,
    /// Each slot's name (empty for slots without a human), indexed by slot.
    pub names: Vec<String>,
}

/// One message. Variant order is part of the protocol.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Joiner to host, first: who the joiner is and the name it wants. `observer` asks for a place to watch rather than
    /// play: the joiner runs the game and sees all of it, but gives no commands and is not a player.
    Hello {
        identity: Identity,
        name: String,
        observer: bool,
    },
    /// Either side: the game cannot go on, and why.
    Refuse { reason: String },
    /// Host to joiner: your slot, and the game.
    Welcome { slot: u8, config: Box<GameConfig> },
    /// Joiner to host: the game is acceptable here (the map is found, with the same hash).
    Ready,
    /// Host to every joiner: every slot is filled and ready; play. `names` are the slots' names, indexed by slot.
    Start { names: Vec<String> },
    /// A player's packet for turn `seq` (`net/lockstep.md`, "One packet per player per turn"): one command or none.
    /// The host stamps `slot` on the way through, so a joiner cannot send as another player.
    Turn { slot: u8, seq: u32, cmd: Option<Command> },
    /// The digest of the sender's game state at the start of turn `turn`, checked at the next turn (`net/lockstep.md`,
    /// "Sync check").
    Sync { slot: u8, turn: u32, digest: u64 },
    /// Host to every joiner: player `slot` is dropped. Its packets from `at_seq` on do not count; its packet `at_seq`
    /// is a drop (`net/leave-and-drop.md`, "Rules for Tidewater": every machine carries the drop out in one turn).
    Drop { slot: u8, at_seq: u32 },
    /// Joiner to host: please drop `slot` (Alt+F2 while waiting for it).
    DropRequest { slot: u8 },
    /// Host to every joiner, whenever the lobby changes: the whole lobby (`ui/game-setup-lobby.md`). The first one
    /// follows the joiner's [`Msg::Ready`].
    Lobby(Box<LobbyView>),
    /// Joiner to host: my race, for my own row.
    SetRace { race: u8 },
    /// Joiner to host: my Ready box (Classic mode).
    SetReady { ready: bool },
    /// Joiner to host: the line in my row's chat box.
    Say { text: String },
    /// Host to a joiner: play. `slot` is the joiner's slot in `config`, which is the game as the lobby settled it.
    Go { slot: u8, config: Box<GameConfig> },
    /// Joiner to host: whether this machine has the map with this hash (the lobby's, or the one that just changed).
    /// The host does not start a game until every joiner has it.
    HaveMap { hash: u64, have: bool },
    /// Joiner to host: please send the map with this hash.
    MapRequest { hash: u64 },
    /// Host to joiner, answering a request: the map is `size` bytes (at most [`MAX_MAP_BYTES`]) and follows in
    /// [`Msg::MapChunk`]s of [`MAP_CHUNK`] bytes, in order.
    MapOffer { hash: u64, size: u32 },
    /// Host to joiner: the next bytes of the map being sent. `offset` is where they go.
    MapChunk { hash: u64, offset: u32, bytes: Vec<u8> },
    /// Host to joiner: the map will not be sent, and why.
    MapDenied { hash: u64, reason: String },
    /// A chat line during the game, from the player in `slot` (the host stamps it, and relays the line to the others).
    /// It is not a command: it is not in any turn and not in the game's state hash (`docs/design/network.md`,
    /// "Chat"). `allies` limits it to the sender's allies; each machine checks that against its own alliance table.
    Chat { slot: u8, allies: bool, text: String },
    /// A minimap ping (option `minimap-ping`) at tile `(x, y)` from the player in `slot` (the host stamps it and
    /// relays it). Like [`Msg::Chat`] it is not in any turn and not in the game's state hash; each machine shows it
    /// only if the sender is its ally.
    Ping { slot: u8, x: u16, y: u16 },
    /// Host to every machine at the start of a game of three or more humans, or with observers: the game's key for a
    /// rejoin, and a joining player should open a socket a new host can be reached on (`docs/design/network.md`, "When
    /// the host leaves").
    Mesh { key: u64 },
    /// Joiner to host, answering [`Msg::Mesh`]: the port this player listens on for a hand-over (0: it could not open
    /// one, so it cannot take over).
    Listen { port: u16 },
    /// Host to every machine: player `slot` can be reached at `addr` if the host goes.
    Peer { slot: u8, addr: String },
    /// To the machine taking over after the host is lost: who this is, the game's key, the next turn it will carry out,
    /// how many packets of each slot it holds, and the drops it was told of (slot, packet number).
    Rejoin {
        slot: u8,
        key: u64,
        turn: u32,
        have: Vec<(u8, u32)>,
        drops: Vec<(u8, u32)>,
    },
    /// The new host to a rejoiner, first: taken in. `have` is how many packets of each slot the new host holds, so the
    /// rejoiner sends what it lacks.
    Resume { host: u8, have: Vec<(u8, u32)> },
    /// A rejoiner to the new host: player `slot`'s packet `seq`, which the new host lacks. Unlike [`Msg::Turn`] its slot
    /// is not the sender's.
    Resend { slot: u8, seq: u32, cmd: Option<Command> },
    /// A rejoiner to the new host: player `slot`'s digest of turn `turn`.
    ResendSync { slot: u8, turn: u32, digest: u64 },
    /// The new host to every machine: player `slot` did not come back after the hand-over. Its packets from `at_seq` on
    /// do not count and its packet `at_seq` is a drop, as for [`Msg::Drop`]; its digests are no longer checked.
    Lost { slot: u8, at_seq: u32 },
}

/// What a row of the lobby holds (`ui/game-setup-lobby.md`, "Rows").
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Available,
    Computer,
    Closed,
    Human,
}

/// One of the lobby's eight rows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RowView {
    pub state: RowState,
    /// The human's name (empty for the other states).
    pub name: String,
    /// The human's race: [`tidewater_sim::setup::race`] human, orc or neutral (Random).
    pub race: u8,
    /// The Ready box (Classic mode); in Enhanced rules a human is always ready.
    pub ready: bool,
    /// The line in the row's chat box.
    pub say: String,
    /// The human has the lobby's map (the host's row always has).
    pub has_map: bool,
}

impl RowView {
    pub fn new(state: RowState) -> RowView {
        RowView {
            state,
            name: String::new(),
            race: tidewater_sim::setup::race::NEUTRAL,
            ready: false,
            say: String::new(),
            has_map: false,
        }
    }
}

/// The lobby as the host holds it and every joiner sees it. Row 0 is the host.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LobbyView {
    pub enhanced: bool,
    /// The map as [`GameConfig::map_name`] and [`GameConfig::map_hash`] say.
    pub map_name: String,
    pub map_hash: u64,
    /// The Scenario box's three lines.
    pub scenario: Vec<String>,
    /// The map's playable slots: the most rows that can be filled.
    pub map_players: u8,
    /// The options: resources, tileset, units, fog of war and Fixed Order (`fixed_starts`). `race` and `opponents`
    /// are the rows'.
    pub custom: tidewater_sim::setup::Custom,
    pub speed: u8,
    /// Classic mode's Cheat Codes box (multiplayer games run with cheats off whatever it says).
    pub cheats_allowed: bool,
    /// The network delay row (`net/lockstep.md`, "Which row").
    pub row: u8,
    /// The optional improvements that change the simulation, as the host set them (`docs/design/extras.md`); every
    /// machine starts the game with these, never with its own settings. All off unless the host turned some on.
    pub extras: tidewater_sim::Extras,
    /// The C+L options as the host set them, like [`GameConfig::client_extras`].
    pub client_extras: Vec<(String, u8)>,
    /// How many observers the host lets in (0 to [`MAX_OBSERVERS`], none by default): places to watch the game from,
    /// outside the rows.
    pub observers: u8,
    /// The names of the observers in the lobby, in the order they came.
    pub watchers: Vec<String>,
    pub rows: Vec<RowView>,
}

/// Writes one frame.
pub fn write_frame(w: &mut impl Write, msg: &Msg) -> io::Result<()> {
    let body = postcard::to_stdvec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if body.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    // One write, so a frame is one segment when it fits.
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    w.write_all(&out)?;
    w.flush()
}

/// Reads one frame. A frame that does not decode, or is longer than [`MAX_FRAME`], is an `InvalidData` error.
pub fn read_frame(r: &mut impl Read) -> io::Result<Msg> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    postcard::from_bytes(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// What a joiner's first frame is, read far enough to give a joiner of another protocol a reason.
#[derive(Debug)]
pub enum HelloRead {
    Hello {
        identity: Identity,
        name: String,
        observer: bool,
    },
    /// A Hello of another [`PROTOCOL`] (its layout may differ, so only the version is read).
    OtherProtocol(u16),
    /// Anything else: not a Tidewater joiner.
    NotHello,
}

/// Reads the first frame of a connection as a Hello.
pub fn read_hello(r: &mut impl Read) -> io::Result<HelloRead> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Ok(HelloRead::NotHello);
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    // Variant 0 (Hello), then the identity's first field, the protocol, as a varint.
    if body.first() == Some(&0)
        && let Ok((protocol, _)) = postcard::take_from_bytes::<u16>(&body[1..])
        && protocol != PROTOCOL
    {
        return Ok(HelloRead::OtherProtocol(protocol));
    }
    Ok(match postcard::from_bytes::<Msg>(&body) {
        Ok(Msg::Hello {
            identity,
            name,
            observer,
        }) => HelloRead::Hello {
            identity,
            name,
            observer,
        },
        _ => HelloRead::NotHello,
    })
}

/// The hash of an import manifest's archive entries: `(path, SHA-256)` of every `.mpq`, by path in lower case.
pub fn manifest_archives<'a>(entries: impl Iterator<Item = (&'a str, &'a str)>) -> u64 {
    let mut v: Vec<(String, &str)> = entries
        .filter(|(p, _)| p.to_ascii_lowercase().ends_with(".mpq"))
        .map(|(p, h)| (p.to_ascii_lowercase(), h))
        .collect();
    v.sort();
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for (p, sha) in v {
        h = fnv_extend(h, p.as_bytes());
        h = fnv_extend(h, &[0]);
        h = fnv_extend(h, sha.as_bytes());
        h = fnv_extend(h, &[0]);
    }
    // 0 means "no manifest": an install with no archives hashes to something else.
    h | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GameConfig {
        GameConfig {
            map_name: "Maps\\x.pud".into(),
            map_hash: 7,
            seed: 9,
            setup: Setup::default(),
            lobby: Lobby {
                humans: vec![(0, 0), (1, 1)],
                local: 0,
                computers: vec![],
                fixed_order: true,
            },
            speed: 8,
            row: 4,
            extras: Default::default(),
            client_extras: vec![("health-bars".into(), 3)],
            names: vec!["Player 1".into(), "Player 2".into()],
        }
    }

    #[test]
    fn a_frame_round_trips_every_message() {
        let msgs = [
            Msg::Hello {
                identity: Identity::ours(5),
                name: "x".into(),
                observer: false,
            },
            Msg::Hello {
                identity: Identity::ours(5),
                name: "x".into(),
                observer: true,
            },
            Msg::Refuse { reason: "no".into() },
            Msg::Welcome {
                slot: 1,
                config: Box::new(config()),
            },
            Msg::Ready,
            Msg::Start {
                names: vec!["a".into()],
            },
            Msg::Turn {
                slot: 1,
                seq: 4,
                cmd: Some(Command::Pause { player: 1 }),
            },
            Msg::Turn {
                slot: 0,
                seq: 5,
                cmd: None,
            },
            Msg::Sync {
                slot: 0,
                turn: 3,
                digest: u64::MAX,
            },
            Msg::Drop { slot: 1, at_seq: 8 },
            Msg::DropRequest { slot: 1 },
            Msg::Lobby(Box::new(LobbyView {
                enhanced: true,
                map_name: "Maps\\x.pud".into(),
                map_hash: 3,
                scenario: vec!["a".into()],
                map_players: 4,
                custom: tidewater_sim::setup::Custom::default(),
                speed: 4,
                cheats_allowed: false,
                row: 3,
                extras: tidewater_sim::Extras {
                    rally_points: true,
                    selection_limit: 12,
                    ..Default::default()
                },
                client_extras: vec![("widescreen".into(), 2)],
                observers: 2,
                watchers: vec!["Eve".into()],
                rows: vec![RowView::new(RowState::Human)],
            })),
            Msg::SetRace { race: 1 },
            Msg::SetReady { ready: true },
            Msg::Say { text: "hi".into() },
            Msg::Go {
                slot: 1,
                config: Box::new(config()),
            },
            Msg::HaveMap { hash: 9, have: true },
            Msg::MapRequest { hash: 9 },
            Msg::MapOffer { hash: 9, size: 40000 },
            Msg::MapChunk {
                hash: 9,
                offset: 16384,
                bytes: vec![1, 2, 3],
            },
            Msg::MapDenied {
                hash: 9,
                reason: "no".into(),
            },
            Msg::Chat {
                slot: 2,
                allies: true,
                text: "hold the ford".into(),
            },
            Msg::Ping { slot: 1, x: 40, y: 99 },
            Msg::Mesh { key: 0xfeed_beef },
            Msg::Listen { port: 50123 },
            Msg::Peer {
                slot: 2,
                addr: "10.0.0.7:50123".into(),
            },
            Msg::Rejoin {
                slot: 2,
                key: 9,
                turn: 40,
                have: vec![(0, 41), (2, 43)],
                drops: vec![(3, 30)],
            },
            Msg::Resume {
                host: 1,
                have: vec![(0, 40)],
            },
            Msg::Resend {
                slot: 0,
                seq: 40,
                cmd: Some(Command::Stop {
                    player: 0,
                    units: vec![],
                }),
            },
            Msg::ResendSync {
                slot: 0,
                turn: 39,
                digest: 77,
            },
            Msg::Lost { slot: 0, at_seq: 41 },
        ];
        let mut bytes = Vec::new();
        for m in &msgs {
            write_frame(&mut bytes, m).unwrap();
        }
        let mut r = bytes.as_slice();
        for m in &msgs {
            assert_eq!(&read_frame(&mut r).unwrap(), m);
        }
        assert!(read_frame(&mut r).is_err(), "nothing left");
    }

    #[test]
    fn hello_is_the_first_variant_so_a_newer_program_can_still_read_it() {
        // Variant 0 encodes as the byte 0; the protocol number follows.
        let bytes = postcard::to_stdvec(&Msg::Hello {
            identity: Identity::ours(1),
            name: String::new(),
            observer: false,
        })
        .unwrap();
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[1], PROTOCOL as u8);
    }

    #[test]
    fn a_frame_that_is_too_long_or_damaged_is_an_error_not_a_panic() {
        let mut huge = (MAX_FRAME as u32 + 1).to_le_bytes().to_vec();
        huge.extend_from_slice(&[0; 8]);
        assert_eq!(
            read_frame(&mut huge.as_slice()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let junk = [3, 0, 0, 0, 0xff, 0xff, 0xff];
        assert_eq!(
            read_frame(&mut junk.as_slice()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let cut = [9, 0, 0, 0, 1];
        assert_eq!(
            read_frame(&mut cut.as_slice()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn identities_that_differ_say_what_differs() {
        let a = Identity::ours(1);
        assert_eq!(a.mismatch(&a.clone()), None);
        let mut b = a.clone();
        b.data_hash = 2;
        assert!(a.mismatch(&b).unwrap().contains("game data"));
        let mut c = a.clone();
        c.sim_build = "other".into();
        assert!(a.mismatch(&c).unwrap().contains("builds"));
        let mut d = a.clone();
        d.protocol += 1;
        assert!(a.mismatch(&d).unwrap().contains("protocol"));
    }

    #[test]
    fn different_mods_are_refused_with_both_sides_mods_named() {
        let tag = |n: &str, v: &str| ModTag {
            name: n.into(),
            version: v.into(),
        };
        let host = Identity::ours(1).with_mods(77, vec![tag("big-axes", "1.2"), tag("sounds", "")]);
        assert_eq!(host.mismatch(&host.clone()), None);
        // Same tables, no mods on the joiner's side: the mods are what differs, and the refusal lists them.
        let why = host.mismatch(&Identity::ours(1)).expect("refused");
        assert!(why.contains("game data or mods"), "{why}");
        assert!(
            why.contains("big-axes 1.2, sounds") && why.contains("joiner mods: none"),
            "{why}"
        );
        // The same names with other content (another hash) are refused too.
        let other = Identity::ours(1).with_mods(78, host.mods.clone());
        assert!(host.mismatch(&other).is_some());
        // Mods or not, different tables are still different game data.
        assert!(
            Identity::ours(1)
                .mismatch(&Identity::ours(2))
                .unwrap()
                .contains("game data or mods")
        );
    }

    #[test]
    fn the_import_manifest_names_the_edition_and_the_archives_when_both_sides_have_one() {
        let ours = Identity::ours(1).with_manifest("Battle.net Edition", 7);
        let no_manifest = Identity::ours(1);
        assert_eq!(
            ours.mismatch(&no_manifest),
            None,
            "a side with no manifest is not judged by it"
        );
        assert_eq!(no_manifest.mismatch(&ours), None);
        let other_edition = Identity::ours(1).with_manifest("Expansion", 7);
        let why = ours.mismatch(&other_edition).unwrap();
        assert!(why.contains("editions") && why.contains("Expansion"), "{why}");
        let other_files = Identity::ours(1).with_manifest("Battle.net Edition", 8);
        assert!(ours.mismatch(&other_files).unwrap().contains("game files"));
        assert_eq!(ours.mismatch(&ours.clone()), None);
    }

    #[test]
    fn the_archive_hash_ignores_other_files_and_order_and_is_never_zero() {
        let a = manifest_archives([("War2Dat.mpq", "aa"), ("Maps/x.pud", "11"), ("patch.MPQ", "bb")].into_iter());
        let b = manifest_archives([("patch.mpq", "bb"), ("war2dat.mpq", "aa")].into_iter());
        assert_eq!(a, b);
        let c = manifest_archives([("war2dat.mpq", "ab"), ("patch.mpq", "bb")].into_iter());
        assert_ne!(a, c);
        assert_ne!(manifest_archives(std::iter::empty()), 0);
    }

    #[test]
    fn a_hello_of_another_protocol_is_recognised_by_its_version_alone() {
        // A frame whose body is a Hello of version 1 with a layout this program doesn't know.
        let mut body = vec![0u8, 1];
        body.extend_from_slice(b"whatever the old layout was");
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(&body);
        match read_hello(&mut frame.as_slice()).unwrap() {
            HelloRead::OtherProtocol(1) => {}
            other => panic!("{other:?}"),
        }
        let mut ours = Vec::new();
        write_frame(
            &mut ours,
            &Msg::Hello {
                identity: Identity::ours(3),
                name: "Ann".into(),
                observer: true,
            },
        )
        .unwrap();
        match read_hello(&mut ours.as_slice()).unwrap() {
            HelloRead::Hello {
                identity,
                name,
                observer,
            } => assert_eq!((identity, name.as_str(), observer), (Identity::ours(3), "Ann", true)),
            other => panic!("{other:?}"),
        }
        let mut other = Vec::new();
        write_frame(&mut other, &Msg::Ready).unwrap();
        assert!(matches!(
            read_hello(&mut other.as_slice()).unwrap(),
            HelloRead::NotHello
        ));
    }
}
