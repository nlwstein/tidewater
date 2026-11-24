//! Terrain painting: tiles as Wang tiles over a grid of corner terrain codes.
//!
//! Established by inspecting the tilesets' `.cv4` group records (`tidewater_formats::tileset`), not from the original
//! editor: each group's four terrain words are its **left, top, right and bottom edges**, and each word holds the
//! terrain code at the edge's two ends, the low byte at the top (or left) end and the high byte at the bottom (or
//! right) end. Codes: 1 and 2 dark and light water, 3 and 4 dark and light coast, 5 and 6 dark and light ground, 7
//! forest, 8 rock, 9 and 10 human and orc wall. On every shipped map the right edge of a tile equals the left edge of
//! its right neighbour and the bottom edge the top edge of the tile below, so a map is a grid of **corner codes**
//! and every tile is the group whose four corners match.
//!
//! A group whose corners hold one code is that terrain's solid group; one with two codes is a boundary between
//! them, and the boundary groups that exist say which codes may meet ("adjacent" codes). They form a tree (in the
//! forest tileset: dark water - light water - light coast - light ground - dark ground, with dark coast, rock and
//! forest hanging off light coast, light coast and light ground), so between any two codes there is one chain of
//! transitions. Walls are lines, not corners, and are not painted here: their groups read as light ground.
//!
//! A brush stroke sets corners to one code. The repair pass then walks outward from every changed corner: a
//! neighbouring corner (any of the 8 around it, since diagonal corners share a tile) whose code can't meet the changed
//! one moves one step along the chain toward it, and is checked in turn. Painted corners never move. Last, every tile
//! touching a changed corner takes the group its corners name: the tile keeps its number when the group is the same,
//! and otherwise takes one of the group's variants, chosen by its position so painting is repeatable.

use std::collections::{HashMap, VecDeque};

use tidewater_formats::tileset::{TileGroup, Tileset};

/// The highest terrain code the tilesets use.
pub const MAX_CODE: u8 = 15;
/// Ground the walls stand on: wall groups read as this code.
const WALL_GROUND: u8 = 6;

/// A tile's corners: top left, top right, bottom left, bottom right.
pub type Corners = [u8; 4];

/// What the tileset allows: the group for each corner pattern, and how codes connect.
#[derive(Clone, Debug)]
pub struct Rules {
    /// Group index by corner pattern.
    groups: HashMap<Corners, usize>,
    /// Each group's tile variants (tile numbers whose megatile is defined).
    variants: Vec<Vec<u16>>,
    /// `next[a][b]`: the code one step from `a` toward `b`; `a` when they are equal, 0 when unreachable.
    next: [[u8; 16]; 16],
    /// The codes that have a solid group, in code order.
    pub solid: Vec<u8>,
}

/// Reads a group's corners, with wall codes taken as the ground they stand on. `None` for an empty group or one
/// whose edges disagree.
pub fn group_corners(g: &TileGroup) -> Option<Corners> {
    let norm = |c: u16| match c as u8 {
        9 | 10 => WALL_GROUND,
        c => c,
    };
    let lo = |w: u16| norm(w & 0xff);
    let hi = |w: u16| norm(w >> 8);
    let [left, top, right, bottom] = g.terrain;
    let c = [lo(left), hi(top), hi(left), hi(right)];
    let consistent = lo(top) == c[0] && lo(right) == c[1] && lo(bottom) == c[2] && hi(bottom) == c[3];
    (consistent && c.iter().all(|&k| (1..=MAX_CODE).contains(&k))).then_some(c)
}

impl Rules {
    pub fn new(ts: &Tileset) -> Self {
        let mut groups = HashMap::new();
        let mut adjacent = [[false; 16]; 16];
        let mut variants = Vec::with_capacity(ts.groups.len());
        let mut solid = Vec::new();
        for (i, g) in ts.groups.iter().enumerate() {
            let tiles: Vec<u16> = (0..16u16)
                .filter(|&v| v == 0 || g.megatiles[v as usize] != 0)
                .map(|v| ((i as u16) << 4) | v)
                .collect();
            variants.push(tiles);
            // Wall groups read as ground but must never be chosen for it.
            let wall = g
                .terrain
                .iter()
                .any(|w| matches!(w & 0xff, 9 | 10) || matches!(w >> 8, 9 | 10));
            let Some(c) = group_corners(g) else { continue };
            if wall && c.iter().all(|&k| k == WALL_GROUND) {
                continue;
            }
            let mut codes = c.to_vec();
            codes.sort_unstable();
            codes.dedup();
            match codes[..] {
                [a] => solid.push(a),
                [a, b] => {
                    adjacent[a as usize][b as usize] = true;
                    adjacent[b as usize][a as usize] = true;
                }
                _ => continue,
            }
            groups.entry(c).or_insert(i);
        }
        solid.sort_unstable();
        solid.dedup();
        // Breadth-first from every target gives the first step from each code toward it.
        let mut next = [[0u8; 16]; 16];
        for b in 0..16usize {
            next[b][b] = b as u8;
            let mut q = VecDeque::from([b]);
            let mut seen = [false; 16];
            seen[b] = true;
            while let Some(x) = q.pop_front() {
                for a in 0..16usize {
                    if adjacent[a][x] && !seen[a] {
                        seen[a] = true;
                        next[a][b] = x as u8;
                        q.push_back(a);
                    }
                }
            }
        }
        Self {
            groups,
            variants,
            next,
            solid,
        }
    }

    /// Whether codes `a` and `b` may share a tile.
    pub fn can_meet(&self, a: u8, b: u8) -> bool {
        a == b || self.next[a as usize][b as usize] == b
    }

    /// The group whose corners are `c`.
    pub fn group(&self, c: Corners) -> Option<usize> {
        self.groups.get(&c).copied()
    }

    /// The tile numbers that draw a group.
    pub fn variants(&self, group: usize) -> &[u16] {
        self.variants.get(group).map_or(&[], |v| v.as_slice())
    }
}

/// A map's corner codes, `(width + 1) × (height + 1)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CornerMap {
    pub width: usize,
    pub height: usize,
    pub codes: Vec<u8>,
}

impl CornerMap {
    /// The corners of a map's tiles. A tile the rules can't read leaves its corners to its neighbours (light ground
    /// when none has them).
    pub fn from_tiles(ts: &Tileset, tiles: &[u16], width: usize, height: usize) -> Self {
        let mut codes = vec![WALL_GROUND; (width + 1) * (height + 1)];
        for y in 0..height {
            for x in 0..width {
                let Some(c) = ts
                    .groups
                    .get((tiles[y * width + x] >> 4) as usize)
                    .and_then(group_corners)
                else {
                    continue;
                };
                let w = width + 1;
                codes[y * w + x] = c[0];
                codes[y * w + x + 1] = c[1];
                codes[(y + 1) * w + x] = c[2];
                codes[(y + 1) * w + x + 1] = c[3];
            }
        }
        Self { width, height, codes }
    }

    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.codes[y * (self.width + 1) + x]
    }

    /// The corners of tile (x, y).
    pub fn tile(&self, x: usize, y: usize) -> Corners {
        [
            self.at(x, y),
            self.at(x + 1, y),
            self.at(x, y + 1),
            self.at(x + 1, y + 1),
        ]
    }
}

/// One brush stroke: `code` on the corners of the `size` × `size` tiles whose top left is `at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub at: (i32, i32),
    pub size: i32,
    pub code: u8,
}

/// Paints a stroke (clipped to the map), repairs the corners around it and redraws the tiles that changed. Returns
/// how many tiles changed. `tiles` is the map's tile numbers, row-major.
pub fn paint(rules: &Rules, ts: &Tileset, tiles: &mut [u16], width: usize, height: usize, stroke: Stroke) -> usize {
    let Stroke { at: (x, y), size, code } = stroke;
    let mut corners = CornerMap::from_tiles(ts, tiles, width, height);
    let w = width as i32 + 1;
    let h = height as i32 + 1;
    let mut fixed = vec![false; corners.codes.len()];
    let mut changed = vec![false; corners.codes.len()];
    let mut queue = VecDeque::new();
    for cy in y.max(0)..(y + size + 1).min(h) {
        for cx in x.max(0)..(x + size + 1).min(w) {
            let n = (cy * w + cx) as usize;
            fixed[n] = true;
            if corners.codes[n] != code {
                corners.codes[n] = code;
                changed[n] = true;
            }
            queue.push_back((cx, cy));
        }
    }
    while let Some((cx, cy)) = queue.pop_front() {
        let here = corners.codes[(cy * w + cx) as usize];
        for (dx, dy) in NEIGHBOURS {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let n = (ny * w + nx) as usize;
            let there = corners.codes[n];
            if fixed[n] || rules.can_meet(here, there) {
                continue;
            }
            let step = rules.next[here as usize][there as usize];
            if step == 0 {
                // No chain between the codes: the neighbour takes the painted code itself.
                corners.codes[n] = here;
            } else {
                corners.codes[n] = step;
            }
            changed[n] = true;
            queue.push_back((nx, ny));
        }
    }
    let mut count = 0;
    for ty in 0..height {
        for tx in 0..width {
            let w = width + 1;
            let touched = [ty * w + tx, ty * w + tx + 1, (ty + 1) * w + tx, (ty + 1) * w + tx + 1]
                .iter()
                .any(|&n| changed[n]);
            if touched && resolve(rules, &corners, tiles, width, tx, ty) {
                count += 1;
            }
        }
    }
    count
}

/// The 8 corners around a corner.
const NEIGHBOURS: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

/// Gives tile (x, y) the group its corners name, keeping its number when the group already matches. Returns whether
/// the tile changed.
fn resolve(rules: &Rules, corners: &CornerMap, tiles: &mut [u16], width: usize, x: usize, y: usize) -> bool {
    let Some(g) = rules.group(corners.tile(x, y)) else {
        return false;
    };
    let old = tiles[y * width + x];
    if (old >> 4) as usize == g {
        return false;
    }
    let v = rules.variants(g);
    if v.is_empty() {
        return false;
    }
    // A fixed scramble of the position picks the variant, so the same stroke always gives the same tiles.
    let h = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
    tiles[y * width + x] = v[(h as usize) % v.len()];
    true
}

/// Checks a map's tiling: every tile's group exists with readable corners, and its edges equal its right and lower
/// neighbours' (wall groups are skipped, since walls join as lines). Returns the positions that fail.
pub fn invalid_tiles(ts: &Tileset, tiles: &[u16], width: usize, height: usize) -> Vec<(usize, usize)> {
    let corners = |t: u16| ts.groups.get((t >> 4) as usize).and_then(group_corners);
    let mut bad = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let Some(c) = corners(tiles[y * width + x]) else {
                bad.push((x, y));
                continue;
            };
            let right = (x + 1 < width).then(|| corners(tiles[y * width + x + 1]));
            let below = (y + 1 < height).then(|| corners(tiles[(y + 1) * width + x]));
            let ok_right = match right {
                Some(Some(r)) => r[0] == c[1] && r[2] == c[3],
                Some(None) => false,
                None => true,
            };
            let ok_below = match below {
                Some(Some(b)) => b[0] == c[2] && b[1] == c[3],
                Some(None) => false,
                None => true,
            };
            if !(ok_right && ok_below) {
                bad.push((x, y));
            }
        }
    }
    bad
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tidewater_formats::tileset::{Palette, TileGroup};

    /// A synthetic tileset shaped like the shipped ones: solid groups for codes 1 to 8 at groups 1 to 8, and the 14
    /// two-code patterns for each adjacent pair, in a chain 1-2-4-6-5 with 3 and 8 off 4 and 7 off 6.
    pub fn synthetic() -> Tileset {
        let pairs = [(1u8, 2u8), (2, 4), (3, 4), (4, 8), (4, 6), (5, 6), (6, 7)];
        let mut groups = vec![group([0; 4])];
        for k in 1..=8u8 {
            groups.push(group([k; 4]));
        }
        while groups.len() < 16 {
            groups.push(group([0; 4]));
        }
        for (a, b) in pairs {
            for mask in 1..15u8 {
                let c = std::array::from_fn(|i| if mask >> i & 1 != 0 { b } else { a });
                groups.push(group(c));
            }
            groups.push(group([0; 4]));
            groups.push(group([0; 4]));
        }
        Tileset {
            palette: Palette([[0; 3]; 256]),
            groups,
            megatiles: vec![[0; 16]; 2],
            minitiles: vec![0; 64],
            changes: None,
        }
    }

    /// A group with these corners (top left, top right, bottom left, bottom right), as the `.cv4` words hold them.
    fn group(c: Corners) -> TileGroup {
        let w = |lo: u8, hi: u8| u16::from(lo) | (u16::from(hi) << 8);
        let mut megatiles = [0u16; 16];
        megatiles[0] = 1;
        megatiles[1] = 1;
        TileGroup {
            megatiles,
            terrain: [w(c[0], c[2]), w(c[0], c[1]), w(c[1], c[3]), w(c[2], c[3])],
            flags: 0,
        }
    }

    #[test]
    fn reads_group_corners() {
        // Group 0x110 of the forest tileset: light water with dark water at the top right.
        let g = TileGroup {
            megatiles: [0; 16],
            terrain: [0x0202, 0x0102, 0x0201, 0x0202],
            flags: 0,
        };
        assert_eq!(group_corners(&g), Some([2, 1, 2, 2]));
        // Walls read as ground.
        let wall = TileGroup {
            terrain: [0x0606, 0x0606, 0x0606, 0x0909],
            ..g
        };
        assert_eq!(group_corners(&wall), Some([6; 4]));
    }

    #[test]
    fn chains_come_from_the_boundary_groups() {
        let r = Rules::new(&synthetic());
        assert_eq!(r.solid, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(r.can_meet(6, 7) && r.can_meet(4, 2) && !r.can_meet(7, 2));
        // Forest to deep water: ground, coast, water.
        assert_eq!(r.next[7][1], 6);
        assert_eq!(r.next[6][1], 4);
        assert_eq!(r.next[4][1], 2);
    }

    #[test]
    fn painting_water_in_forest_adds_the_rings() {
        let ts = synthetic();
        let r = Rules::new(&ts);
        let (w, h) = (16, 16);
        let mut tiles = vec![7 << 4; w * h];
        let n = paint(
            &r,
            &ts,
            &mut tiles,
            w,
            h,
            Stroke {
                at: (7, 7),
                size: 1,
                code: 1,
            },
        );
        assert!(n > 9);
        assert!(invalid_tiles(&ts, &tiles, w, h).is_empty());
        let c = CornerMap::from_tiles(&ts, &tiles, w, h);
        // Painted corners hold deep water, and the rings step out: light water, coast, ground, forest.
        assert_eq!(c.tile(7, 7), [1; 4]);
        assert_eq!((c.at(6, 7), c.at(5, 7), c.at(4, 7), c.at(3, 7)), (2, 4, 6, 7));
    }

    #[test]
    fn random_strokes_keep_a_valid_tiling() {
        let ts = synthetic();
        let r = Rules::new(&ts);
        let (w, h) = (24, 24);
        let mut tiles = vec![6 << 4; w * h];
        let mut seed = 12345u32;
        let mut rand = |m: u32| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 16) % m
        };
        for _ in 0..300 {
            let code = r.solid[rand(r.solid.len() as u32) as usize];
            let (x, y) = (rand(w as u32 + 2) as i32 - 1, rand(h as u32 + 2) as i32 - 1);
            paint(
                &r,
                &ts,
                &mut tiles,
                w,
                h,
                Stroke {
                    at: (x, y),
                    size: [1, 3, 5][rand(3) as usize],
                    code,
                },
            );
            assert!(
                invalid_tiles(&ts, &tiles, w, h).is_empty(),
                "invalid after painting {code} at {x},{y}"
            );
        }
    }

    #[test]
    fn untouched_tiles_keep_their_variant() {
        let ts = synthetic();
        let r = Rules::new(&ts);
        let (w, h) = (16, 16);
        let mut tiles = vec![(6 << 4) | 1; w * h];
        paint(
            &r,
            &ts,
            &mut tiles,
            w,
            h,
            Stroke {
                at: (2, 2),
                size: 1,
                code: 7,
            },
        );
        // Far from the stroke nothing changed, variant 1 included.
        assert_eq!(tiles[15 * w + 15], (6 << 4) | 1);
        // Painting the same code again changes nothing.
        let before = tiles.clone();
        assert_eq!(
            paint(
                &r,
                &ts,
                &mut tiles,
                w,
                h,
                Stroke {
                    at: (2, 2),
                    size: 1,
                    code: 7
                }
            ),
            0
        );
        assert_eq!(tiles, before);
    }
}
