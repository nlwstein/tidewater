//! The unit types the palette offers, their names and groups, and the placement rules the editor checks.
//!
//! Type numbers and names are the public PUD specification's unit list (Daniel Lemberg, "PUD file format", 1997).
//! Footprints and movement classes come from the unit table (`tidewater_formats::udta`). The placement rules are a
//! simplified design of our own over the tileset's movement flags (`tidewater_sim::terrain::bits`), close to the
//! original editor's documented ones (land units on land, ships on water, buildings on buildable land) but not a
//! copy of its exact rules.

use tidewater_formats::udta::{UnitStats, flags};
use tidewater_sim::terrain::bits;

/// A palette section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Land,
    Sea,
    Air,
    Buildings,
    Heroes,
    Neutral,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::Land,
        Group::Sea,
        Group::Air,
        Group::Buildings,
        Group::Heroes,
        Group::Neutral,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::Land => "Land units",
            Group::Sea => "Ships",
            Group::Air => "Air units",
            Group::Buildings => "Buildings",
            Group::Heroes => "Heroes",
            Group::Neutral => "Neutral",
        }
    }
}

/// Which race a type belongs to: the palette shows the selected player's race and the neutral types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Race {
    Human,
    Orc,
    Any,
}

/// (type, name, race, group) for every type the palette offers.
#[rustfmt::skip]
pub const TYPES: &[(u8, &str, Race, Group)] = &[
    (0x00, "Footman", Race::Human, Group::Land),
    (0x01, "Grunt", Race::Orc, Group::Land),
    (0x02, "Peasant", Race::Human, Group::Land),
    (0x03, "Peon", Race::Orc, Group::Land),
    (0x04, "Ballista", Race::Human, Group::Land),
    (0x05, "Catapult", Race::Orc, Group::Land),
    (0x06, "Knight", Race::Human, Group::Land),
    (0x07, "Ogre", Race::Orc, Group::Land),
    (0x08, "Archer", Race::Human, Group::Land),
    (0x09, "Axethrower", Race::Orc, Group::Land),
    (0x0a, "Mage", Race::Human, Group::Land),
    (0x0b, "Death Knight", Race::Orc, Group::Land),
    (0x0c, "Paladin", Race::Human, Group::Land),
    (0x0d, "Ogre-Mage", Race::Orc, Group::Land),
    (0x0e, "Dwarven Demolition Squad", Race::Human, Group::Land),
    (0x0f, "Goblin Sappers", Race::Orc, Group::Land),
    (0x12, "Ranger", Race::Human, Group::Land),
    (0x13, "Berserker", Race::Orc, Group::Land),
    (0x1a, "Oil Tanker", Race::Human, Group::Sea),
    (0x1b, "Oil Tanker", Race::Orc, Group::Sea),
    (0x1c, "Transport", Race::Human, Group::Sea),
    (0x1d, "Transport", Race::Orc, Group::Sea),
    (0x1e, "Elven Destroyer", Race::Human, Group::Sea),
    (0x1f, "Troll Destroyer", Race::Orc, Group::Sea),
    (0x20, "Battleship", Race::Human, Group::Sea),
    (0x21, "Juggernaught", Race::Orc, Group::Sea),
    (0x26, "Gnomish Submarine", Race::Human, Group::Sea),
    (0x27, "Giant Turtle", Race::Orc, Group::Sea),
    (0x28, "Gnomish Flying Machine", Race::Human, Group::Air),
    (0x29, "Goblin Zeppelin", Race::Orc, Group::Air),
    (0x2a, "Gryphon Rider", Race::Human, Group::Air),
    (0x2b, "Dragon", Race::Orc, Group::Air),
    (0x3a, "Farm", Race::Human, Group::Buildings),
    (0x3b, "Pig Farm", Race::Orc, Group::Buildings),
    (0x3c, "Barracks", Race::Human, Group::Buildings),
    (0x3d, "Barracks", Race::Orc, Group::Buildings),
    (0x3e, "Church", Race::Human, Group::Buildings),
    (0x3f, "Altar of Storms", Race::Orc, Group::Buildings),
    (0x40, "Scout Tower", Race::Human, Group::Buildings),
    (0x41, "Watch Tower", Race::Orc, Group::Buildings),
    (0x42, "Stables", Race::Human, Group::Buildings),
    (0x43, "Ogre Mound", Race::Orc, Group::Buildings),
    (0x44, "Gnomish Inventor", Race::Human, Group::Buildings),
    (0x45, "Goblin Alchemist", Race::Orc, Group::Buildings),
    (0x46, "Gryphon Aviary", Race::Human, Group::Buildings),
    (0x47, "Dragon Roost", Race::Orc, Group::Buildings),
    (0x48, "Shipyard", Race::Human, Group::Buildings),
    (0x49, "Shipyard", Race::Orc, Group::Buildings),
    (0x4a, "Town Hall", Race::Human, Group::Buildings),
    (0x4b, "Great Hall", Race::Orc, Group::Buildings),
    (0x4c, "Elven Lumber Mill", Race::Human, Group::Buildings),
    (0x4d, "Troll Lumber Mill", Race::Orc, Group::Buildings),
    (0x4e, "Foundry", Race::Human, Group::Buildings),
    (0x4f, "Foundry", Race::Orc, Group::Buildings),
    (0x50, "Mage Tower", Race::Human, Group::Buildings),
    (0x51, "Temple of the Damned", Race::Orc, Group::Buildings),
    (0x52, "Blacksmith", Race::Human, Group::Buildings),
    (0x53, "Blacksmith", Race::Orc, Group::Buildings),
    (0x54, "Refinery", Race::Human, Group::Buildings),
    (0x55, "Refinery", Race::Orc, Group::Buildings),
    (0x56, "Oil Platform", Race::Human, Group::Buildings),
    (0x57, "Oil Platform", Race::Orc, Group::Buildings),
    (0x58, "Keep", Race::Human, Group::Buildings),
    (0x59, "Stronghold", Race::Orc, Group::Buildings),
    (0x5a, "Castle", Race::Human, Group::Buildings),
    (0x5b, "Fortress", Race::Orc, Group::Buildings),
    (0x60, "Guard Tower", Race::Human, Group::Buildings),
    (0x61, "Guard Tower", Race::Orc, Group::Buildings),
    (0x62, "Cannon Tower", Race::Human, Group::Buildings),
    (0x63, "Cannon Tower", Race::Orc, Group::Buildings),
    (0x14, "Alleria", Race::Human, Group::Heroes),
    (0x16, "Kurdan and Sky'ree", Race::Human, Group::Heroes),
    (0x18, "Khadgar", Race::Human, Group::Heroes),
    (0x2c, "Turalyon", Race::Human, Group::Heroes),
    (0x2e, "Danath", Race::Human, Group::Heroes),
    (0x32, "Lothar", Race::Human, Group::Heroes),
    (0x34, "Uther Lightbringer", Race::Human, Group::Heroes),
    (0x15, "Teron Gorefiend", Race::Orc, Group::Heroes),
    (0x17, "Dentarg", Race::Orc, Group::Heroes),
    (0x19, "Grom Hellscream", Race::Orc, Group::Heroes),
    (0x23, "Deathwing", Race::Orc, Group::Heroes),
    (0x2f, "Korgath Bladefist", Race::Orc, Group::Heroes),
    (0x31, "Cho'gall", Race::Orc, Group::Heroes),
    (0x33, "Gul'dan", Race::Orc, Group::Heroes),
    (0x35, "Zuljin", Race::Orc, Group::Heroes),
    (0x5e, "Start location", Race::Human, Group::Neutral),
    (0x5f, "Start location", Race::Orc, Group::Neutral),
    (GOLD_MINE, "Gold Mine", Race::Any, Group::Neutral),
    (OIL_PATCH, "Oil Patch", Race::Any, Group::Neutral),
    (0x39, "Critter", Race::Any, Group::Neutral),
    (0x64, "Circle of Power", Race::Any, Group::Neutral),
    (0x65, "Dark Portal", Race::Any, Group::Neutral),
    (0x66, "Runestone", Race::Any, Group::Neutral),
    (0x37, "Skeleton", Race::Any, Group::Neutral),
    (0x38, "Daemon", Race::Any, Group::Neutral),
];

pub const GOLD_MINE: u8 = 0x5c;
pub const OIL_PATCH: u8 = 0x5d;
/// The human and orc start locations.
pub const STARTS: [u8; 2] = [0x5e, 0x5f];

/// A type's name, or its number in hex for types the list doesn't name.
pub fn name(kind: u8) -> String {
    TYPES
        .iter()
        .find(|t| t.0 == kind)
        .map_or_else(|| format!("type {kind:#04x}"), |t| t.1.to_string())
}

/// Whether a type is a resource: its `data` word is the amount / 2500.
pub fn is_resource(kind: u8) -> bool {
    kind == GOLD_MINE || kind == OIL_PATCH
}

/// Whether a type belongs to the neutral player by default.
pub fn is_neutral(kind: u8) -> bool {
    TYPES.iter().any(|t| t.0 == kind && t.2 == Race::Any)
}

/// A type's footprint in tiles; start locations are one tile.
pub fn footprint(stats: &[UnitStats], kind: u8) -> (i32, i32) {
    if STARTS.contains(&kind) {
        return (1, 1);
    }
    stats
        .get(kind as usize)
        .map_or((1, 1), |s| (s.size.0.max(1) as i32, s.size.1.max(1) as i32))
}

/// The layer a unit occupies: two units on one layer can't overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Ground,
    Air,
    /// Start locations: one per player, and they overlap anything.
    Marker,
}

pub fn layer(stats: &[UnitStats], kind: u8) -> Layer {
    if STARTS.contains(&kind) {
        return Layer::Marker;
    }
    match stats.get(kind as usize) {
        Some(s) if s.flags & flags::AIR != 0 || s.movement_class == 1 => Layer::Air,
        _ => Layer::Ground,
    }
}

/// Whether a type may stand on a tile with these movement flags (for every tile of its footprint): `water` counts the
/// footprint's water tiles, for shore buildings which need some.
fn tile_ok(s: Option<&UnitStats>, kind: u8, f: u16) -> bool {
    let water = f & bits::WATER != 0;
    let blocked = f & (bits::OBSTACLE | bits::HUMAN_WALL | bits::COMPUTER_WALL) != 0;
    // Start locations, and types the table lacks, stand on open land.
    let Some(s) = s.filter(|_| !STARTS.contains(&kind)) else {
        return !water && !blocked && f & bits::LAND != 0;
    };
    if s.flags & flags::AIR != 0 || s.movement_class == 1 {
        return true;
    }
    if kind == OIL_PATCH || s.flags & flags::SEA != 0 || s.movement_class == 2 {
        return water && !blocked;
    }
    if s.flags & flags::OIL_PATCH != 0 || matches!(kind, 0x56 | 0x57) {
        return water && !blocked;
    }
    if s.flags & flags::SHORE_BUILDING != 0 {
        return !blocked && (water || f & bits::NO_BUILD == 0 || f & bits::SHORE != 0);
    }
    if s.flags & flags::BUILDING != 0 {
        return !water && !blocked && f & bits::NO_BUILD == 0 && f & bits::LAND != 0;
    }
    !water && !blocked && f & bits::LAND != 0
}

/// Whether a unit of `kind` fits at tile (x, y) on terrain whose movement flags `flags_at` gives (`None` off the map).
pub fn terrain_allows(
    stats: &[UnitStats],
    kind: u8,
    (x, y): (i32, i32),
    flags_at: impl Fn(i32, i32) -> Option<u16>,
) -> bool {
    let (w, h) = footprint(stats, kind);
    let s = stats.get(kind as usize);
    let mut water = 0;
    for ty in y..y + h {
        for tx in x..x + w {
            let Some(f) = flags_at(tx, ty) else { return false };
            if !tile_ok(s, kind, f) {
                return false;
            }
            water += i32::from(f & bits::WATER != 0);
        }
    }
    // A shipyard or refinery stands partly on water and partly on land.
    let shore = s.is_some_and(|s| s.flags & flags::SHORE_BUILDING != 0);
    !shore || (water > 0 && water < w * h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> Vec<UnitStats> {
        let mut v = vec![UnitStats::default(); 110];
        v[0].size = (1, 1);
        v[0].flags = flags::LAND;
        v[0x1e].size = (2, 2);
        v[0x1e].flags = flags::SEA;
        v[0x1e].movement_class = 2;
        v[0x3a].size = (2, 2);
        v[0x3a].flags = flags::BUILDING;
        v[0x28].flags = flags::AIR;
        v[0x28].movement_class = 1;
        v
    }

    #[test]
    fn names_and_groups() {
        assert_eq!(name(0x5c), "Gold Mine");
        assert_eq!(name(0x70), "type 0x70");
        assert!(is_neutral(0x5d) && !is_neutral(0x00));
        assert_eq!(footprint(&stats(), 0x5e), (1, 1));
        assert_eq!(layer(&stats(), 0x28), Layer::Air);
    }

    #[test]
    fn terrain_rules() {
        let s = stats();
        // Columns 0 and 1 land, 2 and 3 water, row 3 forest.
        let at = |x: i32, y: i32| -> Option<u16> {
            if !(0..4).contains(&x) || !(0..4).contains(&y) {
                None
            } else if y == 3 {
                Some(0x81)
            } else if x >= 2 {
                Some(0x40)
            } else {
                Some(0x01)
            }
        };
        assert!(terrain_allows(&s, 0x00, (0, 0), at));
        assert!(!terrain_allows(&s, 0x00, (2, 0), at), "footman on water");
        assert!(!terrain_allows(&s, 0x00, (0, 3), at), "footman in the forest");
        assert!(terrain_allows(&s, 0x1e, (2, 0), at));
        assert!(!terrain_allows(&s, 0x1e, (1, 0), at), "ship half on land");
        assert!(terrain_allows(&s, 0x3a, (0, 0), at));
        assert!(!terrain_allows(&s, 0x3a, (0, 2), at), "farm on the forest row");
        assert!(!terrain_allows(&s, 0x3a, (3, 0), at), "farm off the map");
        assert!(terrain_allows(&s, 0x28, (2, 3), at), "flyer anywhere");
        assert!(terrain_allows(&s, 0x5e, (1, 1), at) && !terrain_allows(&s, 0x5e, (2, 1), at));
    }
}
