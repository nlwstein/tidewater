//! The sections a map derives from its tiles: the movement map (`SQM `) and the region map (`REGM`).
//!
//! **Movement map.** Every shipped map's movement words equal its tiles' tileset group flags (checked over all
//! shipped maps), so the editor writes the flags.
//!
//! **Region map.** The region values are those of `docs/specs/movement/occupancy.md` ("How passability changes"):
//! water areas below 0x4000, land areas from 0x4000, and the fixed values for walls, rock and forest. The numbering
//! of the areas is not specified yet (a spec request is open), so this is our own rebuild from what the shipped maps
//! show: a tile's class comes from its group flags and corner codes; walkable land, shore (the water-to-coast
//! boundary groups, flag 0x02, which the shipped maps number as land) and water are cut into 8-connected areas of
//! one class, and each class numbers its areas in scan order. The partition is what matters to the simulation (it
//! compares region values for equality); the original game may need the exact numbers. When the terrain is
//! untouched the editor keeps a map's stored region map instead (`doc`).

use std::collections::VecDeque;

use tidewater_formats::tileset::Tileset;
use tidewater_sim::terrain::{bits, region};

/// The movement words for a map's tiles: the tileset's group flags.
pub fn movement(ts: &Tileset, tiles: &[u16]) -> Vec<u16> {
    tiles.iter().map(|&t| ts.flags_for(t)).collect()
}

/// A tile's region class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Water,
    Land,
    Shore,
    /// A fixed region value (wall, rock, forest).
    Fixed(u16),
}

fn class(ts: &Tileset, tile: u16) -> Class {
    let Some(g) = ts.groups.get((tile >> 4) as usize) else {
        return Class::Fixed(region::UNASSIGNED);
    };
    let any = |code: u16| g.terrain.iter().any(|&w| w & 0xff == code || w >> 8 == code);
    let f = g.flags;
    if f & bits::SHORE != 0 {
        Class::Shore
    } else if f & bits::WATER != 0 {
        Class::Water
    } else if f & (bits::HUMAN_WALL | bits::COMPUTER_WALL) != 0 {
        Class::Fixed(region::WALL)
    } else if f & bits::OBSTACLE != 0 && any(7) {
        Class::Fixed(region::FOREST)
    } else if f & bits::OBSTACLE != 0 {
        Class::Fixed(region::ROCK)
    } else if f & bits::LAND != 0 {
        Class::Land
    } else {
        Class::Fixed(region::UNASSIGNED)
    }
}

/// The region map for a map's tiles.
pub fn regions(ts: &Tileset, tiles: &[u16], width: usize, height: usize) -> Vec<u16> {
    let classes: Vec<Class> = tiles.iter().map(|&t| class(ts, t)).collect();
    let mut out = vec![u16::MAX; tiles.len()];
    let (mut water, mut land) = (0u16, region::LAND_AREA);
    for start in 0..tiles.len() {
        if out[start] != u16::MAX {
            continue;
        }
        let c = classes[start];
        let id = match c {
            Class::Fixed(v) => {
                out[start] = v;
                continue;
            }
            Class::Water => {
                water = water.wrapping_add(1);
                water - 1
            }
            Class::Land | Class::Shore => {
                land = (land + 1).min(region::UNASSIGNED - 1);
                land - 1
            }
        };
        // Flood the area: 8-connected tiles of the same class.
        let mut q = VecDeque::from([start]);
        out[start] = id;
        while let Some(n) = q.pop_front() {
            let (x, y) = ((n % width) as i32, (n / width) as i32);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    let m = ny as usize * width + nx as usize;
                    if out[m] == u16::MAX && classes[m] == c {
                        out[m] = id;
                        q.push_back(m);
                    }
                }
            }
        }
    }
    out
}

/// Whether two region maps cut the map into the same areas (equal fixed values, and a one-to-one match between the
/// area numbers).
pub fn same_partition(a: &[u16], b: &[u16]) -> bool {
    use std::collections::HashMap;
    let area = |v: u16| v < region::UNASSIGNED;
    let (mut ab, mut ba) = (HashMap::new(), HashMap::new());
    a.len() == b.len()
        && a.iter().zip(b).all(|(&x, &y)| {
            if !area(x) || !area(y) {
                return x == y;
            }
            *ab.entry(x).or_insert(y) == y && *ba.entry(y).or_insert(x) == x
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidewater_formats::tileset::{Palette, TileGroup};

    fn ts() -> Tileset {
        let g = |flags: u16, code: u16| TileGroup {
            megatiles: [0; 16],
            terrain: [code * 0x101; 4],
            flags,
        };
        Tileset {
            palette: Palette([[0; 3]; 256]),
            // 0 land, 1 water, 2 forest, 3 rock, 4 shore.
            groups: vec![g(0x01, 6), g(0x40, 2), g(0x81, 7), g(0x81, 8), g(0x82, 4)],
            megatiles: Vec::new(),
            minitiles: Vec::new(),
            changes: None,
        }
    }

    #[test]
    fn areas_are_eight_connected_and_numbered_in_scan_order() {
        // Two water areas split by a land column; land joined only diagonally is one area.
        #[rustfmt::skip]
        let tiles: Vec<u16> = [
            1, 0, 1, 2,
            1, 0, 1, 3,
            0, 2, 1, 1,
            2, 0, 2, 4,
        ].iter().map(|&g| g << 4).collect();
        let r = regions(&ts(), &tiles, 4, 4);
        #[rustfmt::skip]
        assert_eq!(r, [
            0, 0x4000, 1, region::FOREST,
            0, 0x4000, 1, region::ROCK,
            0x4000, region::FOREST, 1, 1,
            region::FOREST, 0x4000, region::FOREST, 0x4001,
        ]);
        assert_eq!(movement(&ts(), &tiles)[3], 0x81);
    }

    #[test]
    fn partitions_compare_up_to_renumbering() {
        assert!(same_partition(&[0, 0, 5, 0xFFFE], &[3, 3, 1, 0xFFFE]));
        assert!(!same_partition(&[0, 0, 5], &[3, 1, 1]));
        assert!(!same_partition(&[0, 0xFFFD], &[0, 0xFFFE]));
    }
}
