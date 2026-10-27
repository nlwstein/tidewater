//! The lobby of a network game, on the command line (`--host`, `--join`, `--lobby`), and the handshake that starts the
//! game (`docs/design/network.md`, "The lobby").
//!
//! `ui/game-setup-lobby.md` describes the lobby screens; those are a later phase (M4). Until then the host gives the lobby
//! on its command line and the joiners are shown it as the host chose it and accept it by joining. The choices the
//! lobby has, as far as this phase has them:
//!
//! | Choice | Command line |
//! |---|---|
//! | Map | the positional map, as for a single-player game |
//! | Rules (Enhanced, or Classic, which is opt-in) and the game's options, fog of war among them | `--setup custom:fog=off,classic` |
//! | Humans, their races, Computer rows, Fixed Order, game speed, network delay, seed | `--lobby humans=2,races=human/orc,computers=1,fixed,speed=8,row=3,seed=N` |
//! | Places for observers, who watch with no fog and give no commands (off unless the host asks) | `--lobby observers=1`; an observer joins with `--join ADDR --observe` |
//! | Optional improvements that change the simulation (`docs/design/extras.md`) | the host's `--extra-NAME` flags and settings file, e.g. `--extra-rally-points` |
//! | The client-only options that give an edge (C+L), set for every player | the host's own `--extra-NAME` flags and settings file, or `--lobby extras=health-bars=always/widescreen=16:9` to choose them for the game |
//!
//! The seed is the host's choice, sent to every machine (the seed exchange): with none given it is the host's clock.

use std::net::TcpListener;

use anyhow::{Context, Result, bail};
use tidewater_formats::install::Install;
use tidewater_sim::setup::{Lobby, Setup, race};

use super::lockstep::DIRECT_IP_ROW;
use super::proto::{DEFAULT_PORT, GameConfig, Identity, MAX_OBSERVERS, data_hash, manifest_archives};
use super::tcp::{self, Wire};
use crate::storage::Store;

/// A game ready to start: the handshake is done and every machine is connected.
pub struct NetStart {
    /// The game, with `lobby.local` this machine's slot.
    pub config: GameConfig,
    /// This machine's slot.
    pub me: u8,
    /// The connection to the other machines.
    pub wire: Box<dyn Wire>,
    /// Every slot's name, indexed by slot.
    pub names: Vec<String>,
}

/// The lobby choices of `--lobby`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbySpec {
    /// Human players, the host first (2 to 8).
    pub humans: usize,
    /// Each human's race, in slot order; missing ones are the map's.
    pub races: Vec<u8>,
    /// Computer rows after the humans.
    pub computers: usize,
    pub fixed_order: bool,
    /// Places for observers, 0 to [`MAX_OBSERVERS`] (the host's choice, none by default): the game starts when every
    /// player and every observer is in. An observer watches with no fog and gives no commands.
    pub observers: usize,
    /// The game speed, 0 to 8.
    pub speed: u8,
    /// The network delay row (`lockstep::ROW_CYCLES`).
    pub row: u8,
    /// The host's choice of seed.
    pub seed: Option<u32>,
    /// The optional improvements that change the simulation, the host's own (`--extra-NAME` flags and settings): the
    /// game starts with these on every machine. Not a `--lobby` choice; the host's command line sets them.
    pub extras: tidewater_sim::Extras,
    /// The C+L options `--lobby extras=NAME=VALUE/NAME=VALUE` names: they are put over the host's own values for the
    /// game's. The host's own settings stand for the others (`host`).
    pub lobby_extras: Vec<(crate::extras::Id, u8)>,
    /// The C+L options every machine plays with (`ClientExtras::lobby_values`): the host's own settings with
    /// [`lobby_extras`](Self::lobby_extras) over them. Set by the host's command-line start, like [`extras`](Self::extras).
    pub client_extras: Vec<(String, u8)>,
}

impl Default for LobbySpec {
    fn default() -> Self {
        LobbySpec {
            humans: 2,
            races: Vec::new(),
            computers: 0,
            fixed_order: false,
            observers: 0,
            speed: tidewater_sim::timing::ENHANCED_MS.len() as u8 - 1,
            // The spec's gap-fill for Direct IP: row 3, 8 cycles to a turn at speed 8 (`net/lockstep.md`, "Which
            // row"). The traced Direct Connect game ran row 4 (12 cycles); `row=4` picks it.
            row: DIRECT_IP_ROW,
            seed: None,
            extras: tidewater_sim::Extras::default(),
            lobby_extras: Vec::new(),
            client_extras: Vec::new(),
        }
    }
}

impl LobbySpec {
    /// Reads `humans=2,races=human/orc,computers=1,fixed,observers=1,speed=8,row=3,seed=N,extras=health-bars=always`;
    /// every part is optional. `extras` takes `NAME=VALUE` pairs separated by `/`, C+L options only.
    pub fn parse(s: &str) -> Result<LobbySpec, String> {
        let mut spec = LobbySpec::default();
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            let number = |max: u64| -> Result<u64, String> {
                let n: u64 = value
                    .parse()
                    .map_err(|_| format!("{key} needs a number, not {value:?}"))?;
                if n > max {
                    return Err(format!("{key} is at most {max}"));
                }
                Ok(n)
            };
            match key {
                "humans" => {
                    spec.humans = number(8)? as usize;
                    if spec.humans < 2 {
                        return Err("humans is at least 2 (a game with one human is a single-player game)".into());
                    }
                }
                "races" => {
                    spec.races = value
                        .split(['/', ':'])
                        .map(|r| match r.trim() {
                            "human" => Ok(race::HUMAN),
                            "orc" => Ok(race::ORC),
                            "random" | "map" => Ok(race::NEUTRAL),
                            other => Err(format!("unknown race {other:?} (human, orc or map)")),
                        })
                        .collect::<Result<_, _>>()?;
                }
                "computers" => spec.computers = number(7)? as usize,
                "fixed" => spec.fixed_order = true,
                "observers" => spec.observers = number(MAX_OBSERVERS as u64)? as usize,
                "speed" => spec.speed = number(8)? as u8,
                "row" => spec.row = number(15)? as u8,
                "seed" => spec.seed = Some(number(u64::from(u32::MAX))? as u32),
                "extras" => {
                    for pair in value.split('/').map(str::trim).filter(|p| !p.is_empty()) {
                        let (name, v) = pair
                            .split_once('=')
                            .ok_or_else(|| format!("extras needs NAME=VALUE pairs, not {pair:?}"))?;
                        let spec_row = crate::extras::lobby_specs()
                            .find(|s| s.name == name.trim())
                            .ok_or_else(|| {
                                let names: Vec<&str> = crate::extras::lobby_specs().map(|s| s.name).collect();
                                format!(
                                    "{name:?} is not an option the lobby sets for everyone ({})",
                                    names.join(", ")
                                )
                            })?;
                        let value = spec_row.parse(v).ok_or_else(|| {
                            format!("{v:?} is not a value of {} ({})", spec_row.name, spec_row.values())
                        })?;
                        spec.lobby_extras.retain(|(id, _)| *id != spec_row.id);
                        spec.lobby_extras.push((spec_row.id, value));
                    }
                }
                other => {
                    return Err(format!(
                        "unknown lobby choice {other:?} (humans, races, computers, fixed, observers, speed, row, seed, extras)"
                    ));
                }
            }
        }
        if spec.humans + spec.computers > 8 {
            return Err("humans and computers together are at most 8".into());
        }
        if spec.races.len() > spec.humans {
            return Err("more races than humans".into());
        }
        Ok(spec)
    }

    /// The game this lobby makes on a map (`map_name`, whose file hashes to `map_hash`), for the host's slot 0.
    pub fn config(&self, map_name: String, map_hash: u64, setup: Setup, seed: u32, host_name: &str) -> GameConfig {
        let humans = (0..self.humans)
            .map(|i| (i as u8, self.races.get(i).copied().unwrap_or(race::NEUTRAL)))
            .collect();
        let computers = (self.humans..self.humans + self.computers).map(|i| i as u8).collect();
        let mut names: Vec<String> = (0..self.humans).map(|i| format!("Player {}", i + 1)).collect();
        if let Some(n) = clean(host_name) {
            names[0] = n;
        }
        GameConfig {
            map_name,
            map_hash,
            seed: self.seed.unwrap_or(seed),
            setup,
            lobby: Lobby {
                humans,
                local: 0,
                computers,
                fixed_order: self.fixed_order,
            },
            speed: self.speed,
            row: self.row,
            extras: self.extras,
            client_extras: self.client_extras.clone(),
            names,
        }
    }

    /// The C+L options this game runs with: the host's own `own` with the lobby choices over them.
    pub fn lobby_values(&self, own: &crate::extras::ClientExtras) -> Vec<(String, u8)> {
        let mut e = *own;
        for &(id, v) in &self.lobby_extras {
            e.set(id, v);
        }
        e.lobby_values()
    }
}

fn clean(name: &str) -> Option<String> {
    let n: String = name.trim().chars().filter(|c| !c.is_control()).take(24).collect();
    (!n.is_empty()).then_some(n)
}

/// The bytes of `map` as a game would read them: the user's own file, else the installation's.
pub(crate) fn map_bytes(store: &Store, install: &mut Install, map: &str) -> Result<(Vec<u8>, bool)> {
    match store.user_map(map).with_context(|| format!("reading {map}"))? {
        Some(d) => Ok((d, true)),
        None => Ok((install.read(map).with_context(|| format!("the map {map}"))?, false)),
    }
}

/// This program's identity over the installation's data tables, and over the import manifest next to them when the
/// installation was imported (`manifest.json`: its edition and archive hashes).
pub fn identity(install: &mut Install, store: &Store) -> Result<Identity> {
    let hash = data_hash(|n| install.read(n))?;
    let id = Identity::ours(hash).with_mods(install.mods_hash(), install.mod_tags());
    let manifest = store
        .install(tidewater_import::manifest::FILE_NAME)
        .ok()
        .flatten()
        .and_then(|mut f| {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut bytes).ok()?;
            tidewater_import::Manifest::parse(&bytes)
        });
    Ok(match manifest {
        Some(m) => {
            let archives = manifest_archives(m.files.iter().map(|f| (f.path.as_str(), f.sha256.as_str())));
            id.with_manifest(&m.edition, archives)
        }
        None => id,
    })
}

/// The game, in words, for both sides' terminals (the lobby the command line stands in for).
pub fn describe(c: &GameConfig) -> String {
    use tidewater_sim::setup::Game;
    let rules = if c.setup.enhanced {
        "Enhanced rules"
    } else {
        "Classic rules (opt-in; Enhanced is the default)"
    };
    let fog = match c.setup.game {
        Game::Custom(custom) if !custom.fog_of_war => ", fog of war off",
        _ => "",
    };
    let speed = tidewater_sim::timing::SPEED_NAMES[usize::from(c.speed).min(8)];
    let humans: Vec<String> = c
        .lobby
        .humans
        .iter()
        .map(|&(slot, r)| {
            let race = match r {
                race::HUMAN => "Human",
                race::ORC => "Orc",
                _ => "map's race",
            };
            format!("{} ({race})", c.names.get(slot as usize).map_or("?", String::as_str))
        })
        .collect();
    let mut extras = c.extras.describe();
    extras.extend(crate::extras::describe_lobby(&c.client_extras));
    let extras = if extras.is_empty() {
        String::new()
    } else {
        format!(", with {}", extras.join(", "))
    };
    format!(
        "{}: {rules}{fog}{extras}, speed {speed}, {} human{}{}: {}; seed {}; turn length {} cycles",
        c.map_name,
        c.lobby.humans.len(),
        if c.lobby.humans.len() == 1 { "" } else { "s" },
        if c.lobby.computers.is_empty() {
            String::new()
        } else {
            format!(", {} computer", c.lobby.computers.len())
        },
        humans.join(", "),
        c.seed,
        super::lockstep::turn_length(c.row, c.setup.enhanced, c.speed),
    )
}

/// Hosts a game on `port` (the default when `None`): opens the port, then waits for every joiner of the lobby. Blocks
/// until the game starts; `say` hears what happens. `wall` is the clock for an unset seed.
#[allow(clippy::too_many_arguments)]
pub fn host(
    port: Option<u16>,
    spec: &LobbySpec,
    map: &str,
    setup: Setup,
    install: &mut Install,
    store: &Store,
    name: &str,
    wall: u64,
    mut say: impl FnMut(&str),
) -> Result<NetStart> {
    let (bytes, user_map) = map_bytes(store, install, map)?;
    let map_name = tidewater_replay::start::recorded_map_name(map, user_map);
    let map_hash = tidewater_replay::start::map_hash(&bytes);
    let identity = identity(install, store)?;
    let config = spec.config(map_name, map_hash, setup, wall as u32, name);
    let listener: TcpListener = tcp::listen(port.unwrap_or(DEFAULT_PORT))?;
    let bound = listener.local_addr().map_or(0, |a| a.port());
    say(&format!("hosting on port {bound}: {}", describe(&config)));
    let joiners: Vec<u8> = config.lobby.humans.iter().skip(1).map(|h| h.0).collect();
    say(&format!(
        "waiting for {} player{} (join with `--join HOST:{bound}`; the port must be open on this machine's firewall)",
        joiners.len(),
        if joiners.len() == 1 { "" } else { "s" }
    ));
    if spec.observers > 0 {
        say(&format!(
            "and {} observer{} (join with `--join HOST:{bound} --observe`)",
            spec.observers,
            if spec.observers == 1 { "" } else { "s" }
        ));
    }
    let wire = tcp::host_game_with(&listener, config.clone(), &identity, &joiners, spec.observers, &mut say)?;
    // The names the joiners gave came in during the handshake.
    let names = wire.names.clone();
    let mut config = config;
    config.names.clone_from(&names);
    Ok(NetStart {
        config,
        me: 0,
        wire: Box::new(wire),
        names,
    })
}

/// Joins the game at `addr`. Blocks until the host starts it. A map this machine doesn't have, with the same contents,
/// is refused before the game starts.
pub fn join(
    addr: &str,
    install: &mut Install,
    store: &Store,
    name: &str,
    observe: bool,
    mut say: impl FnMut(&str),
) -> Result<NetStart> {
    let identity = identity(install, store)?;
    let mut seen = None;
    let joined = {
        let install = &mut *install;
        let seen = &mut seen;
        tcp::join_as(
            addr,
            identity,
            name,
            observe,
            |c| {
                *seen = Some(describe(c));
                let (bytes, _) = map_bytes(store, install, &c.map_name)
                    .map_err(|e| format!("this machine can't read the map {}: {e:#}", c.map_name))?;
                if tidewater_replay::start::map_hash(&bytes) != c.map_hash {
                    return Err(format!(
                        "this machine's {} is a different map from the host's",
                        c.map_name
                    ));
                }
                Ok(())
            },
            &mut say,
        )?
    };
    if let Some(d) = seen {
        say(&format!("the host's game, {d}"));
    }
    if joined.config.lobby.humans.len() != joined.names.len().min(joined.config.lobby.humans.len()) {
        bail!("the host's game doesn't make sense");
    }
    let mut config = joined.config;
    config.names = joined.names.clone();
    Ok(NetStart {
        me: joined.slot,
        config,
        wire: Box::new(joined.wire),
        names: joined.names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lobby_is_read_from_the_command_line() {
        let l = LobbySpec::parse("humans=3,races=human/orc,computers=2,fixed,speed=6,row=3,seed=9").unwrap();
        assert_eq!(l.humans, 3);
        assert_eq!(l.races, [race::HUMAN, race::ORC]);
        assert_eq!(
            (l.computers, l.fixed_order, l.speed, l.row, l.seed),
            (2, true, 6, 3, Some(9))
        );
        assert_eq!(LobbySpec::parse("").unwrap(), LobbySpec::default());
        let d = LobbySpec::default();
        assert_eq!((d.humans, d.speed, d.row), (2, 8, 3));
    }

    #[test]
    fn observers_are_off_unless_the_host_asks_for_them() {
        assert_eq!(LobbySpec::default().observers, 0);
        assert_eq!(LobbySpec::parse("observers=2").unwrap().observers, 2);
        assert!(LobbySpec::parse("observers=5").is_err());
        assert!(LobbySpec::parse("observers=x").is_err());
    }

    #[test]
    fn a_lobby_that_cannot_be_is_refused_with_a_reason() {
        for bad in [
            "humans=1",
            "humans=9",
            "speed=9",
            "row=16",
            "races=elf",
            "wat=1",
            "humans=2,races=human/orc/human",
            "humans=8,computers=1",
            "seed=x",
        ] {
            assert!(LobbySpec::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_lobby_makes_the_game_with_the_hosts_choices() {
        let spec = LobbySpec::parse("humans=2,races=orc/human,computers=1,seed=77").unwrap();
        let c = spec.config("Maps\\x.pud".into(), 5, Setup::default(), 1000, "  Host\n");
        assert_eq!(c.seed, 77, "the host's seed wins over its clock");
        assert_eq!(c.lobby.humans, [(0, race::ORC), (1, race::HUMAN)]);
        assert_eq!(c.lobby.computers, [2]);
        assert_eq!(c.names, ["Host", "Player 2"]);
        let c = LobbySpec::default().config("m".into(), 5, Setup::default(), 1000, "");
        assert_eq!(c.seed, 1000, "no seed given: the clock");
        assert_eq!(c.names[0], "Player 1");
        assert_eq!(c.lobby.humans[0].1, race::NEUTRAL);
    }

    #[test]
    fn the_lobby_choice_extras_sets_the_c_plus_l_options_over_the_hosts_own() {
        use crate::extras::{ClientExtras, Id};
        let l = LobbySpec::parse("humans=2,extras=health-bars=always/widescreen=16:9").unwrap();
        assert_eq!(l.lobby_extras, [(Id::HealthBars, 3), (Id::Widescreen, 2)]);
        // The host's own smart attack-move stays; its own health bars lose to the lobby's.
        let mut own = ClientExtras::default();
        own.set(Id::SmartAttackMove, 1);
        own.set(Id::HealthBars, 1);
        own.set(Id::IdleWorker, 1);
        let values = l.lobby_values(&own);
        assert_eq!(
            values,
            [
                ("health-bars".to_string(), 3),
                ("smart-attack-move".to_string(), 1),
                ("widescreen".to_string(), 2)
            ]
        );
        let mut l = l;
        l.client_extras = values.clone();
        let c = l.config("Maps\\x.pud".into(), 5, Setup::default(), 1, "");
        assert_eq!(c.client_extras, values, "the game carries the host's choice");
        assert!(describe(&c).contains("health-bars always"), "{}", describe(&c));
        // Without it, the host's own settings are the game's.
        assert_eq!(LobbySpec::default().lobby_values(&own).len(), 2);
        // Only options the lobby sets for everyone, with values they take.
        for bad in [
            "extras=idle-worker=on",
            "extras=rally-points=on",
            "extras=health-bars",
            "extras=health-bars=loud",
            "extras=nonsense=1",
        ] {
            assert!(LobbySpec::parse(bad).is_err(), "{bad}");
        }
        let why = LobbySpec::parse("extras=idle-worker=on").unwrap_err();
        assert!(why.contains("health-bars") && why.contains("widescreen"), "{why}");
    }

    #[test]
    fn the_description_labels_classic_rules_as_opt_in() {
        let mut setup = Setup::default();
        let c = LobbySpec::default().config("m.pud".into(), 5, setup, 1, "");
        assert!(describe(&c).contains("Enhanced rules"));
        assert!(describe(&c).contains("turn length 8 cycles"), "{}", describe(&c));
        setup.enhanced = false;
        let c = LobbySpec::default().config("m.pud".into(), 5, setup, 1, "");
        assert!(describe(&c).contains("Classic rules (opt-in"));
    }
}
