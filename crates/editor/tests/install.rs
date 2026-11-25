//! Checks against a real installation (set `TIDEWATER_INSTALL`; without it each test prints that it was skipped):
//! every shipped map saves back byte for byte, the derived sections match the shipped ones as far as they can,
//! painting keeps a valid tiling on real tilesets, and an edited map starts and runs in the simulation.

use std::path::PathBuf;

use tidewater_editor::catalog;
use tidewater_editor::derived;
use tidewater_editor::doc::{Map, owner};
use tidewater_editor::terrain::{self, Rules, Stroke};
use tidewater_formats::install::Install;
use tidewater_formats::pud::{Era, PlacedUnit, Pud};
use tidewater_formats::udta::UnitTable;
use tidewater_sim::setup::{Setup, Start};

fn root() -> Option<PathBuf> {
    let r = std::env::var_os("TIDEWATER_INSTALL").map(PathBuf::from);
    if r.is_none() {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
    }
    r
}

/// Every `.pud` under `Maps`, sorted.
fn shipped_maps(root: &std::path::Path) -> Vec<PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pud")) {
                out.push(p);
            }
        }
    }
    let mut v = Vec::new();
    walk(&root.join("Maps"), &mut v);
    v.sort();
    v
}

#[test]
fn shipped_maps_round_trip() {
    let Some(root) = root() else { return };
    let mut inst = Install::open(&root).unwrap();
    let tilesets: Vec<_> = [Era::Forest, Era::Winter, Era::Wasteland, Era::Swamp]
        .map(|e| inst.tileset(e).unwrap())
        .into();
    let maps = shipped_maps(&root);
    assert!(!maps.is_empty());
    let (mut movement, mut regions_exact, mut regions_partition) = (0, 0, 0);
    for path in &maps {
        let bytes = std::fs::read(path).unwrap();
        let map = Map::from_bytes(&bytes).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let ts = &tilesets[map.era as usize];
        // Unchanged: the same file. Every editable section written again from the decoded state: still the same.
        assert!(map.to_bytes(ts) == bytes, "{} changed on saving", path.display());
        assert!(map.reencoded() == bytes, "{} changed on re-encoding", path.display());
        // The derived sections, compared with the shipped ones.
        let pud = Pud::parse(&bytes).unwrap();
        movement += usize::from(pud.movement().unwrap() == derived::movement(ts, &map.state.tiles));
        let rebuilt = derived::regions(ts, &map.state.tiles, map.size, map.size);
        let stored = pud.regions().unwrap();
        regions_exact += usize::from(rebuilt == stored);
        regions_partition += usize::from(derived::same_partition(&rebuilt, &stored));
        // Every shipped map is a valid tiling by the painter's own check.
        assert!(
            terrain::invalid_tiles(ts, &map.state.tiles, map.size, map.size).is_empty(),
            "{} has tiles the painter reads as invalid",
            path.display()
        );
    }
    eprintln!(
        "{} maps: all byte-identical on saving; movement map rebuilt exactly on {movement}; region map exactly on \
         {regions_exact}, same areas on {regions_partition}",
        maps.len()
    );
    assert_eq!(
        movement,
        maps.len(),
        "the movement map is the group flags on every shipped map"
    );
}

#[test]
fn painting_real_maps_keeps_a_valid_tiling() {
    let Some(root) = root() else { return };
    let mut inst = Install::open(&root).unwrap();
    let maps = shipped_maps(&root);
    let mut seed = 99u32;
    let mut rand = |m: u32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (seed >> 16) % m
    };
    // A few maps of each kind, many strokes each.
    for path in maps.iter().step_by(maps.len() / 12 + 1) {
        let mut map = Map::from_bytes(&std::fs::read(path).unwrap()).unwrap();
        let ts = inst.tileset(map.era).unwrap();
        let rules = Rules::new(&ts);
        assert_eq!(rules.solid, [1, 2, 3, 4, 5, 6, 7, 8], "{}", path.display());
        let n = map.size;
        for _ in 0..150 {
            let code = rules.solid[rand(8) as usize];
            let at = (rand(n as u32) as i32, rand(n as u32) as i32);
            let size = [1, 3, 5][rand(3) as usize];
            terrain::paint(&rules, &ts, &mut map.state.tiles, n, n, Stroke { at, size, code });
            let bad = terrain::invalid_tiles(&ts, &map.state.tiles, n, n);
            assert!(bad.is_empty(), "{}: code {code} at {at:?} left {bad:?}", path.display());
        }
        // The edited map saves and loads.
        let out = Pud::parse(&map.to_bytes(&ts)).unwrap();
        assert_eq!(out.tiles, map.state.tiles);
    }
}

/// The smallest shipped map with two playable slots.
fn small_map(root: &std::path::Path) -> (PathBuf, Vec<u8>) {
    shipped_maps(root)
        .into_iter()
        .map(|p| {
            let b = std::fs::read(&p).unwrap();
            (p, b)
        })
        .filter(|(_, b)| {
            Pud::parse(b).is_ok_and(|p| p.owners().iter().take(8).filter(|&&c| c == 4 || c == 5).count() >= 2)
        })
        .min_by_key(|(_, b)| Pud::parse(b).unwrap().width)
        .unwrap()
}

/// Starts `bytes` as a custom game with `setup`, runs it `cycles` cycles and returns the world.
fn run(inst: &mut Install, bytes: &[u8], setup: &str, cycles: u32) -> tidewater_sim::World {
    let pud = Pud::parse(bytes).unwrap();
    let setup = Setup::parse(setup).unwrap();
    let start = Start::choose(&pud, &setup, 1).unwrap();
    let (mut w, _) = tidewater_replay::start::build_world(inst, &pud, &start).unwrap();
    for _ in 0..cycles {
        w.step(&[]);
    }
    w
}

#[test]
fn edited_map_runs_in_the_simulation() {
    let Some(root) = root() else { return };
    let mut inst = Install::open(&root).unwrap();
    let (path, bytes) = small_map(&root);
    let mut map = Map::from_bytes(&bytes).unwrap();
    let ts = inst.tileset(map.era).unwrap();
    let rules = Rules::new(&ts);
    let stats = UnitTable::from_table(&inst.read("rez\\unitdata.dat").unwrap())
        .unwrap()
        .0;
    let n = map.size;
    // A lake and a wood in the middle.
    terrain::paint(
        &rules,
        &ts,
        &mut map.state.tiles,
        n,
        n,
        Stroke {
            at: (n as i32 / 2, n as i32 / 2),
            size: 5,
            code: 1,
        },
    );
    terrain::paint(
        &rules,
        &ts,
        &mut map.state.tiles,
        n,
        n,
        Stroke {
            at: (n as i32 / 4, n as i32 / 2),
            size: 3,
            code: 7,
        },
    );
    // A footman for the first playable player, on the first tile that takes one, and a gold mine somewhere free.
    let player = map
        .state
        .owners
        .iter()
        .position(|&c| c == owner::PERSON || c == owner::COMPUTER)
        .unwrap() as u8;
    let free = |m: &Map, kind: u8| {
        (0..n as i32 * n as i32)
            .map(|i| (i % n as i32, i / n as i32))
            .find(|&(x, y)| {
                let (w, h) = catalog::footprint(&stats, kind);
                catalog::terrain_allows(&stats, kind, (x, y), |tx, ty| m.tile(tx, ty).map(|t| ts.flags_for(t)))
                    && !m.state.units.iter().any(|u| {
                        let (uw, uh) = catalog::footprint(&stats, u.kind);
                        x < u.x as i32 + uw && (u.x as i32) < x + w && y < u.y as i32 + uh && (u.y as i32) < y + h
                    })
            })
            .unwrap()
    };
    let (fx, fy) = free(&map, 0x00);
    map.state.units.push(PlacedUnit {
        x: fx as u16,
        y: fy as u16,
        kind: 0x00,
        owner: player,
        data: 1,
    });
    let (gx, gy) = free(&map, catalog::GOLD_MINE);
    map.state.units.push(PlacedUnit {
        x: gx as u16,
        y: gy as u16,
        kind: catalog::GOLD_MINE,
        owner: 15,
        data: 20,
    });
    map.state.description = "Edited".into();
    let saved = map.to_bytes(&ts);
    let pud = Pud::parse(&saved).unwrap();
    assert_eq!(pud.description(), "Edited");
    assert!(terrain::invalid_tiles(&ts, &pud.tiles, n, n).is_empty());
    let w = run(&mut inst, &saved, "custom", 600);
    let alive = |kind: u8, tile: (i32, i32)| w.units.iter().any(|u| u.kind == kind && u.tile == tile && u.alive());
    assert!(alive(catalog::GOLD_MINE, (gx, gy)), "{}: the gold mine", path.display());
    let mine = w
        .units
        .iter()
        .find(|u| u.kind == catalog::GOLD_MINE && u.tile == (gx, gy))
        .unwrap();
    // The simulation counts a mine's gold in trips of 100.
    assert_eq!(mine.resources, 20 * 2500 / 100);
    // The footman (a grunt if the game gave the slot the orc race) stands where it was placed, or near it.
    let soldier = w
        .units
        .iter()
        .find(|u| matches!(u.kind, 0x00 | 0x01) && u.owner == player);
    let soldier = soldier.unwrap_or_else(|| {
        panic!(
            "no footman for player {player}: {:?}",
            w.units
                .iter()
                .filter(|u| u.owner == player)
                .map(|u| (u.kind, u.tile))
                .collect::<Vec<_>>()
        )
    });
    assert!((soldier.tile.0 - fx).abs() <= 2 && (soldier.tile.1 - fy).abs() <= 2);
}

#[test]
fn new_map_runs_in_the_simulation() {
    let Some(root) = root() else { return };
    let mut inst = Install::open(&root).unwrap();
    let ts = inst.tileset(Era::Winter).unwrap();
    let rules = Rules::new(&ts);
    let mut map = Map::new(32, Era::Winter, 0x50).unwrap();
    let n = map.size;
    // Coast and water down the middle.
    for y in (0..n as i32).step_by(3) {
        terrain::paint(
            &rules,
            &ts,
            &mut map.state.tiles,
            n,
            n,
            Stroke {
                at: (14, y),
                size: 3,
                code: 2,
            },
        );
    }
    for (i, (x, y)) in [(4u16, 4u16), (26, 26)].into_iter().enumerate() {
        map.state.units.push(PlacedUnit {
            x,
            y,
            kind: catalog::STARTS[i],
            owner: i as u8,
            data: 0,
        });
        map.state.units.push(PlacedUnit {
            x: x - 3,
            y: y - 3,
            kind: catalog::GOLD_MINE,
            owner: 15,
            data: 40,
        });
    }
    let saved = map.to_bytes(&ts);
    assert!(terrain::invalid_tiles(&ts, &Pud::parse(&saved).unwrap().tiles, n, n).is_empty());
    // With only start locations on the map, "one worker only" gives each player a worker there.
    let w = run(&mut inst, &saved, "custom:units=one-worker", 300);
    for p in 0..2u8 {
        assert!(
            w.units.iter().any(|u| u.owner == p && u.alive()),
            "player {p} has units"
        );
    }
    assert_eq!(w.units.iter().filter(|u| u.kind == catalog::GOLD_MINE).count(), 2);
}

/// Research aid (run with `--ignored --nocapture`): which region class the shipped maps give each tile group, next
/// to the class the rebuild gives it.
#[test]
#[ignore]
fn region_classes_by_group() {
    use std::collections::BTreeMap;
    let Some(root) = root() else { return };
    let mut inst = Install::open(&root).unwrap();
    let mut seen: BTreeMap<(u8, u16), BTreeMap<&str, usize>> = BTreeMap::new();
    let class = |v: u16| match v {
        0xFFFA => "unassigned",
        0xFFFB => "wall",
        0xFFFD => "rock",
        0xFFFE => "forest",
        v if v >= 0x4000 => "land",
        _ => "water",
    };
    for path in shipped_maps(&root) {
        let bytes = std::fs::read(&path).unwrap();
        let pud = Pud::parse(&bytes).unwrap();
        let ts = inst.tileset(pud.era).unwrap();
        let rebuilt = derived::regions(&ts, &pud.tiles, pud.width as usize, pud.width as usize);
        for ((&t, &v), &r) in pud.tiles.iter().zip(&pud.regions().unwrap()).zip(&rebuilt) {
            let key = format!("{}>{}", class(v), class(r));
            let e = seen.entry((pud.era as u8, t >> 4)).or_default();
            *e.entry(if class(v) == class(r) {
                "same"
            } else {
                Box::leak(key.into_boxed_str())
            })
            .or_default() += 1;
        }
    }
    for ((era, g), m) in seen {
        if m.len() > 1 || !m.contains_key("same") {
            eprintln!("era {era} group {:#05x}: {m:?}", g << 4);
        }
    }
}
