//! A recorded game replays to the same hashes (set `TIDEWATER_INSTALL` to the game's installation; without it the
//! tests do nothing, like the other tests that need game files).
//!
//! The game is a two-human skirmish on the installation's smallest map with two player slots, run for 1,500 cycles
//! with a script that gathers, trains, builds and moves. The recording is made the way the client makes one: the
//! commands of each cycle go in before the step and a checkpoint every 100 cycles after it.

use std::path::PathBuf;

use tidewater_formats::install::Install;
use tidewater_formats::pud::Pud;
use tidewater_replay::format::CHECKPOINT_EVERY;
use tidewater_replay::run::{first_desync, verify};
use tidewater_replay::seek::{Progress, Seeker, Vision};
use tidewater_replay::start::{build_world, choose_start, map_hash, new_world, origin_world, rules_tag};
use tidewater_replay::{Header, Replay, ReplayError};
use tidewater_sim::economy::kind;
use tidewater_sim::setup::{Lobby, Setup};
use tidewater_sim::site::SiteCheck;
use tidewater_sim::{Command, UnitId, World};

const CYCLES: u32 = 1500;

struct Game {
    install: Install,
    map: Vec<u8>,
    slots: [u8; 2],
}

fn game() -> Option<Game> {
    let root = PathBuf::from(std::env::var_os("TIDEWATER_INSTALL")?);
    let mut best: Option<(usize, Vec<u8>, [u8; 2])> = None;
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("Maps"))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pud")))
        .collect();
    files.sort();
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        let Ok(pud) = Pud::parse(&bytes) else { continue };
        let slots: Vec<u8> = pud
            .owners()
            .iter()
            .enumerate()
            .take(8)
            .filter(|&(_, &c)| c == 4 || c == 5)
            .map(|(i, _)| i as u8)
            .collect();
        let size = pud.width as usize * pud.height as usize;
        if slots.len() >= 2 && best.as_ref().is_none_or(|b| size < b.0) {
            best = Some((size, bytes, [slots[0], slots[1]]));
        }
    }
    let (_, map, slots) = best?;
    Some(Game {
        install: Install::open(&root).unwrap(),
        map,
        slots,
    })
}

fn lobby(slots: [u8; 2], local: u8) -> Lobby {
    Lobby {
        humans: vec![(slots[0], 0), (slots[1], 1)],
        local,
        computers: vec![],
        fixed_order: true,
    }
}

/// The commands for cycle `t`, worked out from the world.
fn script(w: &World, t: u32, players: [u8; 2]) -> Vec<Command> {
    let mut out = Vec::new();
    for &p in &players {
        let mine: Vec<&tidewater_sim::Unit> = w.units.iter().filter(|u| u.owner == p && !u.dying).collect();
        let workers: Vec<UnitId> = mine.iter().filter(|u| !u.building).map(|u| u.id).collect();
        let Some(hall) = mine.iter().find(|u| kind::is_hall(u.kind)) else {
            continue;
        };
        match t {
            1 => {
                let dist = |a: (i32, i32), b: (i32, i32)| (a.0 - b.0).abs().max((a.1 - b.1).abs());
                let sites: Vec<(i32, i32)> = w.units.iter().filter(|u| u.building).map(|u| u.tile).collect();
                for &id in &workers {
                    let me = w.unit(id).unwrap();
                    if let Some(tile) = sites
                        .iter()
                        .copied()
                        .filter(|&s| w.can_harvest(me, s))
                        .min_by_key(|&s| dist(me.tile, s))
                    {
                        out.push(Command::Harvest {
                            player: p,
                            units: vec![id],
                            target: None,
                            tile,
                        });
                    }
                }
            }
            2 | 400 | 900 => out.push(Command::Train {
                player: p,
                building: hall.id,
                kind: if hall.kind == kind::TOWN_HALL { 0x02 } else { 0x03 },
            }),
            300 | 1200 => {
                let farm = if hall.kind == kind::TOWN_HALL {
                    kind::FARM
                } else {
                    kind::PIG_FARM
                };
                if let Some(&b) = workers.first() {
                    let at = (-6..=6)
                        .flat_map(|dy| (-6..=6).map(move |dx| (hall.tile.0 + dx, hall.tile.1 + dy)))
                        .find(|&s| w.check_site(farm, s, b) == SiteCheck::Allowed);
                    if let Some(tile) = at {
                        out.push(Command::Build {
                            player: p,
                            unit: b,
                            kind: farm,
                            tile,
                        });
                    }
                }
            }
            700 => out.push(Command::Move {
                player: p,
                units: workers.iter().copied().take(2).collect(),
                to: (hall.tile.0 + 3, hall.tile.1 + 3),
            }),
            760 => out.push(Command::Stop {
                player: p,
                units: workers.iter().copied().take(1).collect(),
            }),
            _ => {}
        }
    }
    out
}

/// Plays the scripted game and records it the way the client does.
fn record(g: &mut Game) -> (Replay, u64) {
    let pud = Pud::parse(&g.map).unwrap();
    let lobby = lobby(g.slots, g.slots[0]);
    let setup = Setup::default();
    let start = choose_start(&pud, &setup, 12345, Some(&lobby)).unwrap();
    let (mut world, _) = build_world(&mut g.install, &pud, &start).unwrap();
    world.start_up();
    let header = Header::for_game(
        "test.pud".into(),
        map_hash(&g.map),
        12345,
        setup,
        Some(lobby),
        &world,
        0,
        start.local,
    );
    assert_eq!(header.rules, rules_tag(&world));
    let mut replay = Replay::new(header);
    for _ in 0..CYCLES {
        let tick = world.tick;
        let cmds = script(&world, tick, g.slots);
        replay.record(tick, &cmds);
        world.step(&cmds);
        if world.tick % CHECKPOINT_EVERY == 0 {
            replay.checkpoint(world.tick, world.state_hash());
        }
    }
    (replay, world.state_hash())
}

#[test]
fn a_recorded_game_plays_back_to_the_same_hashes() {
    let Some(mut g) = game() else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let (replay, final_hash) = record(&mut g);
    assert!(
        replay.commands.len() > 5,
        "the script gave {} turns",
        replay.commands.len()
    );
    assert_eq!(replay.checkpoints.len(), (CYCLES / CHECKPOINT_EVERY) as usize);
    // Through the file and back, as the client writes and reads it.
    let back = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(back, replay);
    let mut world = new_world(&mut g.install, &g.map, &back.header).unwrap();
    let report = verify(&mut world, &back).expect("the replay verifies");
    assert_eq!(report.cycles, CYCLES);
    assert_eq!(report.checked, (CYCLES / CHECKPOINT_EVERY) as usize);
    assert_eq!(report.final_hash, final_hash);
}

#[test]
fn a_replay_from_the_other_player_s_seat_has_the_same_hashes() {
    let Some(mut g) = game() else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let (replay, _) = record(&mut g);
    let mut other = replay.clone();
    other.header = replay.header.with_local(g.slots[1]).unwrap();
    assert_eq!(other.header.local, g.slots[1]);
    let mut a = new_world(&mut g.install, &g.map, &replay.header).unwrap();
    let mut b = new_world(&mut g.install, &g.map, &other.header).unwrap();
    assert_eq!(a.vision.local, g.slots[0]);
    assert_eq!(b.vision.local, g.slots[1]);
    assert_eq!(first_desync(&mut a, &replay, &mut b, &other), None);
    // A single-player replay has no other seat.
    let mut solo = replay.header.clone();
    solo.lobby = None;
    assert!(solo.with_local(1).is_err());
}

#[test]
fn a_replay_with_a_changed_command_stops_at_the_first_checkpoint_after_it() {
    let Some(mut g) = game() else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let (replay, _) = record(&mut g);
    // Drop the move order at cycle 700: the first checkpoint at or after it that differs is 800.
    let mut bad = replay.clone();
    bad.commands.retain(|c| c.cycle != 700);
    let mut world = new_world(&mut g.install, &g.map, &bad.header).unwrap();
    let m = verify(&mut world, &bad).expect_err("a changed command must not verify");
    assert_eq!(m.cycle, 800, "{m}");
    assert!(m.to_string().contains("cycle 800"));
    // desync-diff names the cycle and the field.
    let mut a = new_world(&mut g.install, &g.map, &replay.header).unwrap();
    let mut b = new_world(&mut g.install, &g.map, &bad.header).unwrap();
    let d = first_desync(&mut a, &replay, &mut b, &bad).expect("a difference");
    assert!(d.cycle > 700 && d.cycle <= 800, "cycle {}", d.cycle);
    let diff = d.difference.expect("a named piece");
    assert!(!diff.part.is_empty() && !diff.left.is_empty(), "{diff}");
}

#[test]
fn a_wrong_map_is_refused() {
    let Some(mut g) = game() else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let (replay, _) = record(&mut g);
    let mut wrong = g.map.clone();
    let last = wrong.len() - 1;
    wrong[last] ^= 1;
    let err = new_world(&mut g.install, &wrong, &replay.header).expect_err("refused");
    assert!(matches!(
        err.downcast_ref::<ReplayError>(),
        Some(ReplayError::WrongMap { .. })
    ));
    assert!(err.to_string().contains("different file"), "{err}");
}

#[test]
fn a_replay_checks_the_rules_it_was_recorded_under() {
    let Some(mut g) = game() else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let (replay, _) = record(&mut g);
    let mut header = replay.header.clone();
    header.rules.enhanced = false;
    let err = new_world(&mut g.install, &g.map, &header).expect_err("refused");
    assert!(matches!(
        err.downcast_ref::<ReplayError>(),
        Some(ReplayError::Rules { .. })
    ));
}

/// The state hash after every cycle of a straight playthrough: index `n` is the hash with the counter at `n`.
fn straight_hashes(g: &mut Game, replay: &Replay) -> Vec<u64> {
    let mut world = new_world(&mut g.install, &g.map, &replay.header).unwrap();
    let mut hashes = vec![world.state_hash()];
    for _ in 0..replay.cycles {
        world.step(replay.commands_at(world.tick));
        hashes.push(world.state_hash());
    }
    hashes
}

/// Seeks to `target` and plays the jump out, with the recording's own checkpoints compared on the way. Returns the
/// number of `work` calls it took.
fn jump(seeker: &mut Seeker, world: &mut World, replay: &Replay, target: u32) -> u32 {
    seeker.seek(world, target);
    let mut calls = 0;
    loop {
        calls += 1;
        let done = seeker
            .work(world, replay, 250, |tick, hash| {
                assert_eq!(Some(hash), replay.checkpoint_at(tick), "checkpoint {tick} on the way");
                Ok(())
            })
            .expect("no mismatch");
        if done != Progress::Running {
            assert!(matches!(done, Progress::Arrived | Progress::Idle));
            break;
        }
    }
    assert_eq!(world.tick, target.min(replay.cycles));
    calls
}

#[test]
fn seeking_back_and_forth_gives_the_straight_playthrough_s_hash_at_every_stop() {
    let Some(mut g) = game() else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let (replay, _) = record(&mut g);
    let want = straight_hashes(&mut g, &replay);
    let origin = origin_world(&mut g.install, &g.map, &replay.header).unwrap();
    let mut seeker = Seeker::new(origin, &replay);
    // Play it through once, as the client does, so the copies are made.
    let mut world = new_world(&mut g.install, &g.map, &replay.header).unwrap();
    while world.tick < replay.cycles {
        world.step(replay.commands_at(world.tick));
        seeker.note(&world);
    }
    assert_eq!(seeker.keyframes().ticks().collect::<Vec<_>>(), [500, 1000, 1500]);
    assert_eq!(world.state_hash(), want[1500]);
    // Back and forth: before the first copy (the origin), on a copy, between copies, ahead and back, the very end,
    // the very start and past the end.
    for target in [1100, 200, 1400, 650, 500, 1000, 999, 0, 1500, 1, 1499, 760, 1200, 2000] {
        let calls = jump(&mut seeker, &mut world, &replay, target);
        let at = target.min(replay.cycles);
        assert_eq!(world.state_hash(), want[at as usize], "after seeking to {target}");
        assert!(calls <= 5, "a jump to {target} took {calls} calls of 250 cycles");
    }
    // The world that a jump leaves is one the replay plays on from: a straight run of 100 more cycles agrees too.
    jump(&mut seeker, &mut world, &replay, 700);
    for _ in 0..100 {
        world.step(replay.commands_at(world.tick));
        assert_eq!(world.state_hash(), want[world.tick as usize]);
    }
}

#[test]
fn another_player_s_view_and_everyone_s_leave_the_hash_alone() {
    let Some(mut g) = game() else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let (replay, _) = record(&mut g);
    let want = straight_hashes(&mut g, &replay);
    let origin = origin_world(&mut g.install, &g.map, &replay.header).unwrap();
    let mut seeker = Seeker::new(origin, &replay);
    let mut world = new_world(&mut g.install, &g.map, &replay.header).unwrap();
    assert_eq!(seeker.vision(), Vision::Player(g.slots[0]));
    assert!(seeker.players().contains(&g.slots[1]));
    assert!(
        seeker.has_fog(g.slots[0]) && seeker.has_fog(g.slots[1]),
        "both are humans"
    );
    assert!(!seeker.has_fog(7), "a slot nobody plays has none");
    jump(&mut seeker, &mut world, &replay, 900);
    let mine = world.vision.local;
    let explored_of = |w: &World| w.vision.flags(tidewater_sim::vision::Map::Explored).to_vec();
    let first = explored_of(&world);

    // The other player: the game is played again from the start with that player's maps, to the same cycle.
    seeker.set_vision(&mut world, Vision::Player(g.slots[1]));
    assert_eq!(world.tick, 0, "the new view starts over");
    let mut calls = 0;
    while seeker.work(&mut world, &replay, 250, |_, _| Ok(())).unwrap() == Progress::Running {
        calls += 1;
    }
    assert_eq!(world.tick, 900);
    assert!(calls >= 3, "the jump is spread over frames");
    assert_eq!(world.vision.local, g.slots[1]);
    assert_eq!(seeker.seat(), g.slots[1]);
    assert_ne!(world.vision.local, mine);
    assert_eq!(
        world.state_hash(),
        want[900],
        "the other seat's world has the same hash"
    );
    assert_ne!(explored_of(&world), first, "and its own explored map");

    // Back and forth in that view still matches, and so does everyone's.
    for target in [300, 1300, 650] {
        jump(&mut seeker, &mut world, &replay, target);
        assert_eq!(world.state_hash(), want[target as usize]);
    }
    seeker.set_vision(&mut world, Vision::Everyone);
    assert!(world.observing && seeker.vision() == Vision::Everyone);
    for target in [1000, 100, 1500] {
        jump(&mut seeker, &mut world, &replay, target);
        assert!(world.observing, "a restored world shows everyone too");
        assert_eq!(world.state_hash(), want[target as usize]);
    }
    // Everyone's is only the flag: switching it on or off keeps the world and its tick.
    seeker.set_vision(&mut world, Vision::Player(g.slots[1]));
    assert!(!world.observing);
    assert_eq!(world.tick, 1500);
}

#[test]
fn a_recorded_replay_with_a_wrong_command_stops_a_seek_at_the_checkpoint() {
    let Some(mut g) = game() else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let (replay, _) = record(&mut g);
    let mut bad = replay.clone();
    bad.commands.retain(|c| c.cycle != 700);
    let origin = origin_world(&mut g.install, &g.map, &bad.header).unwrap();
    let mut seeker = Seeker::new(origin, &bad);
    let mut world = new_world(&mut g.install, &g.map, &bad.header).unwrap();
    seeker.seek(&mut world, 1200);
    let m = loop {
        match seeker.work(&mut world, &bad, 250, |tick, hash| match bad.checkpoint_at(tick) {
            Some(r) if r != hash => Err(tidewater_replay::run::Mismatch {
                cycle: tick,
                recorded: r,
                got: hash,
            }),
            _ => Ok(()),
        }) {
            Ok(Progress::Running) => {}
            Ok(p) => panic!("arrived ({p:?}) on a replay that does not match"),
            Err(m) => break m,
        }
    };
    assert_eq!(m.cycle, 800);
    assert_eq!(seeker.target(), None, "a mismatch ends the jump");
}
