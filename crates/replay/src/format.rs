//! The `.twrep` file: one game's start and every command, with hash checkpoints.
//!
//! ```text
//! offset  size  field
//! 0       8     magic, the bytes "TWREPLAY"
//! 8       2     format version, little endian (7)
//! 10      8     body length in bytes, little endian
//! 18      n     body: a postcard-encoded [`Replay`]
//! 18+n    8     FNV-1a 64 of every byte before it, little endian
//! ```
//!
//! The body is serialised with `postcard` (a compact, versioned-by-us serde format; the dependency tracks its newest
//! release). The checksum makes a damaged or truncated file fail with a clear error instead of replaying garbage; it
//! is not a defence against a deliberately edited file (a replay is trusted input like a saved game). Any change to the
//! body's layout, to [`tidewater_sim::Command`] or to [`Header`] bumps [`VERSION`].
//!
//! A cycle is named by the world's tick counter as it stands *before* the cycle runs ([`tidewater_sim::World::tick`]):
//! the commands at cycle `n` are applied at the start of the step that takes the counter from `n` to `n + 1`. A
//! checkpoint at cycle `n` is the state hash *after* that counter reached `n`, so checkpoint 100 follows the step of
//! cycle 99.

use serde::{Deserialize, Serialize};
use tidewater_formats::identity::{DataIdentity, DataMismatch};
use tidewater_sim::setup::{Lobby, Setup};
use tidewater_sim::{Command, Extras};

/// The first bytes of every replay file.
pub const MAGIC: [u8; 8] = *b"TWREPLAY";
/// The format version this build writes and reads. Version 2: the header's extras (`RulesTag::extras`) have fields. Version 3: the extras gain shift-queued orders. Version 4: the diplomacy command exists (`game/diplomacy.md`). Version 5: the Hold workers and Release workers commands exist (`orders/orders.md`) and the speed command carries the new speed. Version 6: the select command names unit slots, empty ones included, instead of unit ids (round 107, `ui/selecting-and-commanding.md`, "What the synced selection holds"). Version 7: the header names the game data the game ran on ([`Header::data`]); version 6 files still load, with that identity unknown.
pub const VERSION: u16 = 7;
/// The oldest format version [`Replay::decode`] still reads.
pub const OLDEST: u16 = 6;
/// The file name extension.
pub const EXTENSION: &str = "twrep";
/// A checkpoint is recorded every this many cycles (and at the end of a finished game).
pub const CHECKPOINT_EVERY: u32 = 100;

const FIXED: usize = 8 + 2 + 8;

/// Why a replay could not be read or used.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReplayError {
    #[error("not a Tidewater replay (the file does not start with the replay marker)")]
    NotAReplay,
    #[error("replay format version {found} is not supported (this build reads versions {OLDEST} to {VERSION})")]
    Version { found: u16 },
    #[error("the replay file is cut short ({found} bytes, {wanted} expected)")]
    Truncated { found: usize, wanted: usize },
    #[error("the replay file is damaged (its checksum does not match)")]
    Checksum,
    #[error("the replay file is damaged ({0})")]
    Decode(String),
    #[error(
        "this replay was recorded on map {name}, but the map found there is a different file (hash {found:016x}, the replay wants {wanted:016x})"
    )]
    WrongMap { name: String, wanted: u64, found: u64 },
    #[error("this replay was recorded with different game files: {0}")]
    Data(DataMismatch),
    #[error(
        "the replay's rules ({recorded}) do not match the game it starts ({built}); is the data or the build different?"
    )]
    Rules { recorded: String, built: String },
}

/// The rules a game ran under, as the world reports them after its start (`tidewater_sim::Rules`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RulesTag {
    /// Enhanced rules (the default) rather than Classic, which exists only as the multiplayer opt-in.
    pub enhanced: bool,
    pub multiplayer: bool,
    pub cheats_disabled: bool,
    /// The optional improvements that touch the sim, as the game ran with them (`docs/design/extras.md`). Playback
    /// starts the world with these, never with the viewer's own settings. Four bytes while the set has its four
    /// fields (version 2 of the format).
    pub extras: Extras,
}

impl std::fmt::Display for RulesTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}{}{}",
            if self.enhanced { "Enhanced" } else { "Classic" },
            if self.multiplayer { ", multiplayer" } else { "" },
            if self.cheats_disabled { ", cheats disabled" } else { "" },
            if self.extras.is_default() { "" } else { ", with extras" }
        )
    }
}

/// Everything needed to start the game again.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Header {
    /// The simulation build that recorded the replay (`sim_build`). A different build may still replay it; the
    /// checkpoints say whether it did.
    pub sim_build: String,
    /// The map as the game was told to load it: a name inside the archives, or the file name of a map the user gave.
    pub map_name: String,
    /// FNV-1a 64 of the map file's bytes ([`crate::start::map_hash`]).
    pub map_hash: u64,
    /// The game's start seed (`core/timing-and-rng.md`, "Seeding").
    pub seed: u32,
    pub setup: Setup,
    /// The lobby of a multiplayer game; `None` for a single-player game.
    pub lobby: Option<Lobby>,
    pub rules: RulesTag,
    /// The cheat flags the client started the world with (the three kept across games; zero in a multiplayer game).
    pub start_cheats: u32,
    /// The slot the recording player controlled. Playback shows the game as this player saw it.
    pub local: u8,
    /// The game data the game ran on (`tidewater_formats::identity`): the tables' hash and the mods. `None` in a
    /// replay of format version 6 or a recording made without it: unknown, so playback does not compare.
    pub data: Option<DataIdentity>,
}

/// A header as format version 6 wrote it, before [`Header::data`].
#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct HeaderV6 {
    sim_build: String,
    map_name: String,
    map_hash: u64,
    seed: u32,
    setup: Setup,
    lobby: Option<Lobby>,
    rules: RulesTag,
    start_cheats: u32,
    local: u8,
}

/// A replay as format version 6 wrote it.
#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct ReplayV6 {
    header: HeaderV6,
    commands: Vec<CycleCommands>,
    checkpoints: Vec<Checkpoint>,
    cycles: u32,
    notes: Vec<String>,
}

impl From<ReplayV6> for Replay {
    fn from(r: ReplayV6) -> Replay {
        let h = r.header;
        Replay {
            header: Header {
                sim_build: h.sim_build,
                map_name: h.map_name,
                map_hash: h.map_hash,
                seed: h.seed,
                setup: h.setup,
                lobby: h.lobby,
                rules: h.rules,
                start_cheats: h.start_cheats,
                local: h.local,
                data: None,
            },
            commands: r.commands,
            checkpoints: r.checkpoints,
            cycles: r.cycles,
            notes: r.notes,
        }
    }
}

/// The commands that ran at one cycle, in the order they ran.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CycleCommands {
    pub cycle: u32,
    pub commands: Vec<Command>,
}

/// The state hash after the world's tick counter reached `cycle`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub cycle: u32,
    pub hash: u64,
}

/// A recorded game.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Replay {
    pub header: Header,
    /// Only the cycles that had commands, in ascending cycle order.
    pub commands: Vec<CycleCommands>,
    /// In ascending cycle order, at multiples of [`CHECKPOINT_EVERY`] and at the end of a finished game.
    pub checkpoints: Vec<Checkpoint>,
    /// How many cycles were played: the replay runs cycles `0..cycles`.
    pub cycles: u32,
    /// Things that happened in the recorded game that the command stream cannot reproduce (a change made outside the
    /// commands, `docs/design/multiplayer-determinism.md`, "Writes to the world outside the command stream"). A replay
    /// with notes may fail its checkpoints; playback says so first.
    pub notes: Vec<String>,
}

impl Replay {
    pub fn new(header: Header) -> Replay {
        Replay {
            header,
            commands: Vec::new(),
            checkpoints: Vec::new(),
            cycles: 0,
            notes: Vec::new(),
        }
    }

    /// Records that `commands` ran at `cycle`, which also counts the cycle as played. Cycles come in ascending order;
    /// an empty list records nothing but the count.
    pub fn record(&mut self, cycle: u32, commands: &[Command]) {
        self.cycles = self.cycles.max(cycle + 1);
        if commands.is_empty() {
            return;
        }
        match self.commands.last_mut() {
            Some(last) if last.cycle == cycle => last.commands.extend_from_slice(commands),
            _ => self.commands.push(CycleCommands {
                cycle,
                commands: commands.to_vec(),
            }),
        }
    }

    /// Records the state hash at `cycle` (replacing one already there).
    pub fn checkpoint(&mut self, cycle: u32, hash: u64) {
        match self.checkpoints.last_mut() {
            Some(last) if last.cycle == cycle => last.hash = hash,
            _ => self.checkpoints.push(Checkpoint { cycle, hash }),
        }
    }

    /// Adds a note once.
    pub fn note(&mut self, text: &str) {
        if !self.notes.iter().any(|n| n == text) {
            self.notes.push(text.to_string());
        }
    }

    /// The commands that run at `cycle`.
    pub fn commands_at(&self, cycle: u32) -> &[Command] {
        match self.commands.binary_search_by_key(&cycle, |c| c.cycle) {
            Ok(i) => &self.commands[i].commands,
            Err(_) => &[],
        }
    }

    /// The recorded hash at `cycle`, if there is a checkpoint there.
    pub fn checkpoint_at(&self, cycle: u32) -> Option<u64> {
        self.checkpoints
            .binary_search_by_key(&cycle, |c| c.cycle)
            .ok()
            .map(|i| self.checkpoints[i].hash)
    }

    /// The file's bytes.
    pub fn encode(&self) -> Vec<u8> {
        let body = postcard::to_stdvec(self).expect("a replay always serialises");
        let mut out = Vec::with_capacity(FIXED + body.len() + 8);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(body.len() as u64).to_le_bytes());
        out.extend_from_slice(&body);
        let sum = fnv64(&out);
        out.extend_from_slice(&sum.to_le_bytes());
        out
    }

    /// Reads a replay file, checking the marker, the version, the length and the checksum before anything else.
    pub fn decode(bytes: &[u8]) -> Result<Replay, ReplayError> {
        if bytes.len() < MAGIC.len() || bytes[..MAGIC.len()] != MAGIC {
            return Err(ReplayError::NotAReplay);
        }
        if bytes.len() < FIXED {
            return Err(ReplayError::Truncated {
                found: bytes.len(),
                wanted: FIXED,
            });
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if !(OLDEST..=VERSION).contains(&version) {
            return Err(ReplayError::Version { found: version });
        }
        let len = u64::from_le_bytes(bytes[10..18].try_into().expect("8 bytes"));
        let wanted = usize::try_from(len)
            .ok()
            .and_then(|n| n.checked_add(FIXED + 8))
            .ok_or_else(|| ReplayError::Decode("an impossible length".into()))?;
        if bytes.len() < wanted {
            return Err(ReplayError::Truncated {
                found: bytes.len(),
                wanted,
            });
        }
        if bytes.len() > wanted {
            return Err(ReplayError::Decode("bytes after the end".into()));
        }
        let (covered, sum) = bytes.split_at(wanted - 8);
        if fnv64(covered) != u64::from_le_bytes(sum.try_into().expect("8 bytes")) {
            return Err(ReplayError::Checksum);
        }
        let body = &covered[FIXED..];
        let decoded = if version == VERSION {
            postcard::from_bytes(body)
        } else {
            postcard::from_bytes::<ReplayV6>(body).map(Replay::from)
        };
        decoded.map_err(|e| ReplayError::Decode(e.to_string()))
    }
}

/// FNV-1a over bytes, 64 bits.
pub fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    })
}

/// The simulation build a replay is recorded by: this crate's version, which is the workspace's.
pub fn sim_build() -> String {
    format!("tidewater-sim {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            sim_build: sim_build(),
            map_name: "Maps\\x.pud".into(),
            map_hash: 7,
            seed: 12345,
            setup: Setup::default(),
            lobby: Some(Lobby {
                humans: vec![(0, 0), (1, 1)],
                local: 1,
                computers: vec![],
                fixed_order: true,
            }),
            rules: RulesTag {
                enhanced: true,
                multiplayer: true,
                cheats_disabled: false,
                extras: Extras::default(),
            },
            start_cheats: 0,
            local: 1,
            data: Some(DataIdentity {
                data_hash: 0xabcd,
                mods_hash: 0,
                mods: vec![],
            }),
        }
    }

    fn sample() -> Replay {
        let mut r = Replay::new(header());
        r.record(3, &[Command::Pause { player: 0 }]);
        r.record(3, &[Command::Surrender { player: 1 }]);
        r.record(9, &[]);
        r.checkpoint(100, 0xdead_beef);
        r.note("fog of war changed");
        r
    }

    #[test]
    fn a_replay_round_trips() {
        let r = sample();
        assert_eq!(r.cycles, 10);
        assert_eq!(r.commands_at(3).len(), 2, "same-cycle commands are kept in order");
        assert!(r.commands_at(4).is_empty());
        assert_eq!(r.checkpoint_at(100), Some(0xdead_beef));
        let back = Replay::decode(&r.encode()).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn a_replay_with_the_optional_improvements_on_round_trips() {
        let mut h = header();
        h.rules.extras = Extras {
            rally_points: true,
            production_queue: true,
            selection_limit: 18,
            extra_speeds: 2,
            order_queue: true,
        };
        let mut r = Replay::new(h);
        r.record(
            4,
            &[Command::Rally {
                player: 0,
                building: tidewater_sim::UnitId(3),
                target: None,
                tile: (7, 8),
            }],
        );
        let back = Replay::decode(&r.encode()).unwrap();
        assert_eq!(back, r);
        assert!(!back.header.rules.extras.is_default());
        assert!(back.header.rules.to_string().contains("extras"));
    }

    /// A diplomacy command (`game/diplomacy.md`) is recorded and read back like any other.
    #[test]
    fn a_replay_with_a_diplomacy_command_round_trips() {
        let mut r = Replay::new(header());
        let cmd = Command::Diplomacy {
            player: 1,
            alliances: [1, 1, 0, 3, 0, 0, 0, 0],
            vision: 0b1010,
            allied_victory: true,
        };
        r.record(5, std::slice::from_ref(&cmd));
        let back = Replay::decode(&r.encode()).unwrap();
        assert_eq!(back.commands_at(5), [cmd]);
    }

    /// Hold workers and Release workers (`orders/orders.md`, "Holding and releasing workers") and a speed command that
    /// carries its value are recorded and read back like any other.
    #[test]
    fn a_replay_with_hold_release_and_speed_commands_round_trips() {
        let mut r = Replay::new(header());
        let cmds = [
            Command::HoldWorkers { player: 0 },
            Command::ReleaseWorkers { player: 0 },
            Command::Speed { player: 0, speed: 6 },
        ];
        r.record(7, &cmds);
        let back = Replay::decode(&r.encode()).unwrap();
        assert_eq!(back.commands_at(7), cmds);
    }

    #[test]
    fn the_file_starts_with_the_marker_and_the_version() {
        let b = sample().encode();
        assert_eq!(&b[..8], b"TWREPLAY");
        assert_eq!(u16::from_le_bytes([b[8], b[9]]), VERSION);
    }

    #[test]
    fn damage_is_reported_not_replayed() {
        let good = sample().encode();
        assert_eq!(Replay::decode(b"hello"), Err(ReplayError::NotAReplay));
        assert!(matches!(
            Replay::decode(&good[..12]),
            Err(ReplayError::Truncated { .. })
        ));
        assert!(matches!(
            Replay::decode(&good[..good.len() - 3]),
            Err(ReplayError::Truncated { .. })
        ));
        let mut flipped = good.clone();
        flipped[30] ^= 0x40;
        assert_eq!(Replay::decode(&flipped), Err(ReplayError::Checksum));
        let mut other = good.clone();
        other[8] = 9;
        assert_eq!(Replay::decode(&other), Err(ReplayError::Version { found: 9 }));
        let mut long = good;
        long.push(0);
        assert!(matches!(Replay::decode(&long), Err(ReplayError::Decode(_))));
    }

    #[test]
    fn a_sound_checksum_over_nonsense_is_a_decode_error() {
        let mut b = Vec::new();
        b.extend_from_slice(&MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.extend_from_slice(&3u64.to_le_bytes());
        b.extend_from_slice(&[0xff, 0xff, 0xff]);
        let sum = fnv64(&b);
        b.extend_from_slice(&sum.to_le_bytes());
        assert!(matches!(Replay::decode(&b), Err(ReplayError::Decode(_))));
    }

    /// A file as format version 6 wrote it: the same body without the header's data identity.
    fn version_6_file(r: &Replay) -> Vec<u8> {
        let h = &r.header;
        let old = ReplayV6 {
            header: HeaderV6 {
                sim_build: h.sim_build.clone(),
                map_name: h.map_name.clone(),
                map_hash: h.map_hash,
                seed: h.seed,
                setup: h.setup,
                lobby: h.lobby.clone(),
                rules: h.rules,
                start_cheats: h.start_cheats,
                local: h.local,
            },
            commands: r.commands.clone(),
            checkpoints: r.checkpoints.clone(),
            cycles: r.cycles,
            notes: r.notes.clone(),
        };
        let body = postcard::to_stdvec(&old).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&6u16.to_le_bytes());
        out.extend_from_slice(&(body.len() as u64).to_le_bytes());
        out.extend_from_slice(&body);
        let sum = fnv64(&out);
        out.extend_from_slice(&sum.to_le_bytes());
        out
    }

    #[test]
    fn a_version_6_replay_still_loads_with_unknown_data() {
        let r = sample();
        let back = Replay::decode(&version_6_file(&r)).unwrap();
        assert_eq!(back.header.data, None);
        let mut expected = r;
        expected.header.data = None;
        assert_eq!(back, expected);
        // An unknown identity never refuses a replay.
        let now = DataIdentity {
            data_hash: 1,
            mods_hash: 2,
            mods: vec![],
        };
        assert_eq!(back.header.check_data(&now), Ok(()));
    }

    #[test]
    fn versions_outside_the_supported_range_are_refused() {
        let mut b = sample().encode();
        b[8] = 5;
        assert_eq!(Replay::decode(&b), Err(ReplayError::Version { found: 5 }));
    }

    #[test]
    fn a_replay_is_refused_on_other_game_data_and_says_which() {
        let h = header();
        let same = h.data.clone().unwrap();
        assert_eq!(h.check_data(&same), Ok(()));
        let tables = DataIdentity {
            data_hash: 1,
            ..same.clone()
        };
        let err = h.check_data(&tables).unwrap_err();
        assert!(err.to_string().contains("different game files"), "{err}");
        assert!(err.to_string().contains("data hash differs"), "{err}");
        let modded = DataIdentity {
            data_hash: 1,
            mods_hash: 77,
            mods: vec![tidewater_formats::identity::ModTag {
                name: "pack".into(),
                version: "2".into(),
            }],
        };
        let err = h.check_data(&modded).unwrap_err();
        assert!(err.to_string().contains("mod set differs"), "{err}");
        assert!(err.to_string().contains("pack 2"), "{err}");
    }
}
