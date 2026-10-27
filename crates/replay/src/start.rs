//! The world a game starts as: the one builder the client and the tools share, so a replay starts from exactly the
//! world the recorded game did.
//!
//! The steps are the client's (`scene::load`, `game::start_world`): choose the players from the map, build the world
//! from the map and the installation's tables, give it the computer-player scripts, run the two start-up cycles
//! (`core/timing-and-rng.md`, scheduler rule 1) and start the cheat flags the game began with.

use std::sync::Arc;

use anyhow::{Context, Result};
use tidewater_formats::identity::DataIdentity;
use tidewater_formats::install::Install;
use tidewater_formats::pud::Pud;
use tidewater_formats::tileset::Tileset;
use tidewater_formats::udta::UnitTable;
use tidewater_sim::World;
use tidewater_sim::cheats::Flags;
use tidewater_sim::setup::{Lobby, Setup, Start};

use crate::format::{Header, ReplayError, RulesTag, fnv64, sim_build};

/// The hash of a map file's bytes, as the header keeps it (FNV-1a 64).
pub fn map_hash(bytes: &[u8]) -> u64 {
    fnv64(bytes)
}

/// The rules a world runs under, as a header keeps them.
pub fn rules_tag(world: &World) -> RulesTag {
    RulesTag {
        enhanced: world.rules.enhanced,
        multiplayer: world.rules.multiplayer,
        cheats_disabled: world.rules.cheats_disabled,
        extras: world.rules.extras,
    }
}

/// The name a header keeps for a map: the file name of a map the user gave by its own path, else the name as given
/// (a name inside the archives). A path never goes into a replay.
pub fn recorded_map_name(map: &str, is_user_file: bool) -> String {
    if is_user_file {
        std::path::Path::new(map)
            .file_name()
            .map_or_else(|| map.to_string(), |n| n.to_string_lossy().into_owned())
    } else {
        map.to_string()
    }
}

impl Header {
    /// The header of a game that is starting: `map_hash` is [`map_hash`] of the map file as read, `world` the world
    /// the start built (before any command), `start_cheats` the flags the client started it with.
    #[allow(clippy::too_many_arguments)]
    pub fn for_game(
        map_name: String,
        map_hash: u64,
        seed: u32,
        setup: Setup,
        lobby: Option<Lobby>,
        world: &World,
        start_cheats: u32,
        local: u8,
    ) -> Header {
        Header {
            sim_build: sim_build(),
            map_name,
            map_hash,
            seed,
            setup,
            lobby,
            rules: rules_tag(world),
            start_cheats,
            local,
            data: None,
        }
    }

    /// This header with the game data the game ran on.
    pub fn with_data(mut self, data: DataIdentity) -> Header {
        self.data = Some(data);
        self
    }

    /// [`with_data`](Self::with_data) for an identity that may be unknown.
    pub fn with_data_opt(mut self, data: Option<DataIdentity>) -> Header {
        self.data = data;
        self
    }

    /// Refuses to play a replay on game data other than the data it was recorded on, naming what differs (the mods or
    /// the tables). A replay that records no identity (format version 6) passes: its checkpoints will say.
    pub fn check_data(&self, now: &DataIdentity) -> Result<(), ReplayError> {
        match self.data.as_ref().and_then(|d| d.mismatch(now)) {
            Some(m) => Err(ReplayError::Data(m)),
            None => Ok(()),
        }
    }

    /// Refuses a map that is not the one the game was recorded on.
    pub fn check_map(&self, map_bytes: &[u8]) -> Result<(), ReplayError> {
        let found = map_hash(map_bytes);
        if found == self.map_hash {
            Ok(())
        } else {
            Err(ReplayError::WrongMap {
                name: self.map_name.clone(),
                wanted: self.map_hash,
                found,
            })
        }
    }

    /// The same game with another human as the local player (`twtool desync-diff --local`). Only a game with a lobby
    /// has humans to choose from.
    pub fn with_local(&self, slot: u8) -> Result<Header> {
        let lobby = self
            .lobby
            .as_ref()
            .context("this replay is a single-player game; it has no other player to view it as")?;
        anyhow::ensure!(
            lobby.humans.iter().any(|&(s, _)| s == slot),
            "slot {slot} is not a human in this replay (the humans are in slots {:?})",
            lobby.humans.iter().map(|h| h.0).collect::<Vec<_>>()
        );
        let mut h = self.clone();
        h.lobby.as_mut().expect("checked above").local = slot;
        h.local = slot;
        Ok(h)
    }
}

/// The players and choices of a game: a multiplayer start when there is a lobby, else a single-player one.
pub fn choose_start(pud: &Pud, setup: &Setup, seed: u32, lobby: Option<&Lobby>) -> Result<Start> {
    Ok(match lobby {
        Some(l) => Start::multiplayer(pud, setup, seed, l)?,
        None => Start::choose(pud, setup, seed)?,
    })
}

/// Builds the world for `start` on `pud` from the installation's tables, with the computer players' scripts, and
/// returns the era's tileset it was built with (the client draws the terrain with it). The two start-up cycles have
/// not run yet.
pub fn build_world(install: &mut Install, pud: &Pud, start: &Start) -> Result<(World, Tileset)> {
    let ts = install.tileset(start.era)?;
    let stats = match pud.section(b"UDTA").map(UnitTable::from_udta).transpose()?.flatten() {
        Some(t) => t,
        None => UnitTable::from_table(&install.read("rez\\unitdata.dat")?)?,
    };
    let scripts = tidewater_formats::script::Scripts::parse(install.read("rez\\script.bin")?)?;
    let upgrades = tidewater_formats::ugrd::UpgradeTable::from_table(&install.read("rez\\upgrades.dat")?)?;
    let mut world = World::from_pud(pud, &ts, stats, upgrades, Arc::new(scripts), start)?;
    let ai = tidewater_formats::aibin::AiFile::parse(install.read("rez\\ai.bin")?)?;
    world.start_ai(Arc::new(ai));
    Ok((world, ts))
}

/// The world a replay starts from, ready for its first recorded cycle: checks the map against the header, builds the
/// world, runs the start-up cycles, starts the cheat flags and checks that the rules came out as recorded. A replay
/// recorded on other game data is refused first, naming the difference ([`Header::check_data`]).
pub fn new_world(install: &mut Install, map_bytes: &[u8], header: &Header) -> Result<World> {
    header.check_data(&DataIdentity::of(install)?)?;
    new_world_unchecked(install, map_bytes, header)
}

/// [`new_world`] without the game data comparison (`twtool replay --force`): the map and the rules are still checked.
pub fn new_world_unchecked(install: &mut Install, map_bytes: &[u8], header: &Header) -> Result<World> {
    header.check_map(map_bytes)?;
    let pud = Pud::parse(map_bytes)?;
    let start = choose_start(&pud, &header.setup, header.seed, header.lobby.as_ref())?;
    let (mut world, _) = build_world(install, &pud, &start)?;
    // The options the game ran with, from the header: never the viewer's own settings.
    world.rules.extras = header.rules.extras;
    world.start_up();
    if header.start_cheats != 0 {
        world.start_cheats(Flags(header.start_cheats));
    }
    let built = rules_tag(&world);
    if built != header.rules {
        return Err(ReplayError::Rules {
            recorded: header.rules.to_string(),
            built: built.to_string(),
        }
        .into());
    }
    Ok(world)
}
