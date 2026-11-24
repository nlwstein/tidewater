//! The map being edited: what the editor changes (tiles, units, players, description) over the file's sections.
//!
//! Loading keeps every section of the file ([`PudFile`]); saving rewrites only the sections whose content changed and
//! leaves the rest as they were, so a map opened and saved without edits is the same file byte for byte. When the
//! tiles changed, the movement map is regenerated and the region map rebuilt (`derived`); otherwise both are kept as
//! the file had them.
//!
//! Section layouts are the public PUD specification's (see `tidewater_formats::pud`).

use anyhow::{Context, Result, bail};
use tidewater_formats::pud::{Era, PlacedUnit, Pud};
use tidewater_formats::pudfile::{PudFile, from_words, words};
use tidewater_formats::tileset::Tileset;

use crate::derived;

/// Owner codes (`OWNR`) the editor offers, from the public PUD specification.
pub mod owner {
    pub const PASSIVE_COMPUTER: u8 = 2;
    pub const NOBODY: u8 = 3;
    pub const COMPUTER: u8 = 4;
    pub const PERSON: u8 = 5;
    pub const RESCUE_PASSIVE: u8 = 6;
    pub const RESCUE_ACTIVE: u8 = 7;
}

/// The neutral player that owns gold mines, oil patches, critters and the like.
pub const NEUTRAL: u8 = 15;

/// Everything the editor changes: one undo step stores a copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// Tile numbers, row-major, size × size.
    pub tiles: Vec<u16>,
    pub units: Vec<PlacedUnit>,
    /// The description, at most 31 bytes.
    pub description: String,
    pub owners: [u8; 16],
    /// 0 human, 1 orc, 2 neutral.
    pub sides: [u8; 16],
    pub gold: [u16; 16],
    pub lumber: [u16; 16],
    pub oil: [u16; 16],
    /// The computer script per player (`AIPL`), when the map has the section.
    pub ai: Option<[u8; 16]>,
}

/// An open map.
#[derive(Clone, Debug)]
pub struct Map {
    file: PudFile,
    /// The state as loaded, to tell which sections changed.
    original: State,
    pub size: usize,
    pub era: Era,
    pub state: State,
}

fn array<T: Copy + Default, const N: usize>(v: &[T]) -> [T; N] {
    std::array::from_fn(|i| v.get(i).copied().unwrap_or_default())
}

/// The description as it is stored: the text, cut to 31 bytes, then zeros up to 32.
fn desc_bytes(s: &str) -> [u8; 32] {
    let mut d = [0u8; 32];
    let b = s.as_bytes();
    let n = b.len().min(31);
    d[..n].copy_from_slice(&b[..n]);
    d
}

fn unit_bytes(units: &[PlacedUnit]) -> Vec<u8> {
    let mut v = Vec::with_capacity(units.len() * 8);
    for u in units {
        v.extend(u.x.to_le_bytes());
        v.extend(u.y.to_le_bytes());
        v.extend([u.kind, u.owner]);
        v.extend(u.data.to_le_bytes());
    }
    v
}

impl Map {
    /// Reads a map file. It must load as the game loads it (`Pud::parse`).
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let pud = Pud::parse(data).context("reading the map")?;
        let file = PudFile::split(data);
        let state = State {
            tiles: pud.tiles.clone(),
            units: pud.units.clone(),
            description: pud.description(),
            owners: array(pud.owners()),
            sides: array(pud.sides()),
            gold: array(&from_words(pud.section(b"SGLD").unwrap_or_default())),
            lumber: array(&from_words(pud.section(b"SLBR").unwrap_or_default())),
            oil: array(&from_words(pud.section(b"SOIL").unwrap_or_default())),
            ai: pud.section(b"AIPL").map(array),
        };
        Ok(Self {
            file,
            original: state.clone(),
            size: pud.width as usize,
            era: pud.era,
            state,
        })
    }

    /// A new map of `size` × `size` tiles of `tile`, with no units. Players 1 and 2 are a person and a computer.
    pub fn new(size: u16, era: Era, tile: u16) -> Result<Self> {
        if !matches!(size, 32 | 64 | 96 | 128) {
            bail!("map size {size} is not 32, 64, 96 or 128");
        }
        let n = size as usize * size as usize;
        let era_word = match era {
            Era::Forest => 0u16,
            Era::Winter => 1,
            Era::Wasteland => 2,
            Era::Swamp => 3,
        };
        let mut f = PudFile::default();
        // The id tag (TYPE bytes 12 to 15) only has to differ between maps; a fixed one is fine for a new map.
        f.set(b"TYPE", b"WAR2 MAP\0\0\x0a\xff\x00\x00\x00\x00".to_vec());
        f.set(b"VER ", words(&[0x13]));
        f.set(b"DESC", desc_bytes("New map").to_vec());
        let mut owners = [owner::NOBODY; 16];
        owners[0] = owner::PERSON;
        owners[1] = owner::COMPUTER;
        f.set(b"OWNR", owners.to_vec());
        // Swamp needs the expansion's ERAX; ERA  then holds the forest so the original's older readers still work.
        f.set(b"ERA ", words(&[if era == Era::Swamp { 0 } else { era_word }]));
        f.set(b"ERAX", words(&[era_word]));
        f.set(b"DIM ", words(&[size, size]));
        let mut sides = [0u8; 16];
        for (i, s) in sides.iter_mut().enumerate() {
            *s = if i >= 8 { 2 } else { (i % 2) as u8 };
        }
        f.set(b"SIDE", sides.to_vec());
        f.set(b"SGLD", words(&[2000; 16]));
        f.set(b"SLBR", words(&[1000; 16]));
        f.set(b"SOIL", words(&[1000; 16]));
        f.set(b"MTXM", words(&vec![tile; n]));
        f.set(b"SQM ", words(&vec![0; n]));
        f.set(b"OILM", vec![0; n]);
        f.set(b"REGM", words(&vec![0; n]));
        f.set(b"UNIT", Vec::new());
        let mut map = Self::from_bytes(&f.to_bytes())?;
        // The movement and region maps above are placeholders: mark the terrain as changed so saving derives them.
        map.original.tiles.clear();
        Ok(map)
    }

    /// Whether anything differs from the map as loaded or last saved.
    pub fn modified(&self) -> bool {
        self.state != self.original
    }

    /// The map file: the loaded sections with the changed ones rewritten. `ts` is the map's tileset, for the derived
    /// sections.
    pub fn to_bytes(&self, ts: &Tileset) -> Vec<u8> {
        self.file_with(ts, &self.original).to_bytes()
    }

    /// Marks the current state as saved.
    pub fn saved(&mut self, ts: &Tileset) {
        self.file = self.file_with(ts, &self.original);
        self.original = self.state.clone();
    }

    /// The file with every section that differs from `base` rewritten.
    fn file_with(&self, ts: &Tileset, base: &State) -> PudFile {
        let s = &self.state;
        let mut f = self.file.clone();
        if s.description != base.description {
            f.set(b"DESC", desc_bytes(&s.description).to_vec());
        }
        if s.owners != base.owners {
            f.set(b"OWNR", s.owners.to_vec());
        }
        if s.sides != base.sides {
            f.set(b"SIDE", s.sides.to_vec());
        }
        if s.gold != base.gold {
            f.set(b"SGLD", words(&s.gold));
        }
        if s.lumber != base.lumber {
            f.set(b"SLBR", words(&s.lumber));
        }
        if s.oil != base.oil {
            f.set(b"SOIL", words(&s.oil));
        }
        if s.ai != base.ai {
            match s.ai {
                Some(ai) => f.set(b"AIPL", ai.to_vec()),
                None => f.remove(b"AIPL"),
            }
        }
        if s.tiles != base.tiles {
            f.set(b"MTXM", words(&s.tiles));
            f.set(b"SQM ", words(&derived::movement(ts, &s.tiles)));
            f.set(b"REGM", words(&derived::regions(ts, &s.tiles, self.size, self.size)));
        }
        if s.units != base.units {
            f.set(b"UNIT", unit_bytes(&s.units));
        }
        f
    }

    /// Every editable section written again from the state, as if all of it had changed except the derived movement
    /// and region maps. For the round-trip test: it shows the encoders give the file's own bytes.
    pub fn reencoded(&self) -> Vec<u8> {
        let mut f = self.file.clone();
        let s = &self.state;
        f.set(b"DESC", desc_bytes(&s.description).to_vec());
        f.set(b"OWNR", s.owners.to_vec());
        f.set(b"SIDE", s.sides.to_vec());
        f.set(b"SGLD", words(&s.gold));
        f.set(b"SLBR", words(&s.lumber));
        f.set(b"SOIL", words(&s.oil));
        if let Some(ai) = s.ai {
            f.set(b"AIPL", ai.to_vec());
        }
        f.set(b"MTXM", words(&s.tiles));
        f.set(b"UNIT", unit_bytes(&s.units));
        f.to_bytes()
    }

    /// The stored section, for checks.
    pub fn section(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.file.get(tag)
    }

    pub fn tile(&self, x: i32, y: i32) -> Option<u16> {
        (x >= 0 && y >= 0 && (x as usize) < self.size && (y as usize) < self.size)
            .then(|| self.state.tiles[y as usize * self.size + x as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidewater_formats::pud::build;

    #[test]
    fn unchanged_map_saves_byte_for_byte() {
        let mut m = build::Map::default();
        m.units.push((3, 4, 0x5c, 15, 40));
        let bytes = m.bytes();
        let map = Map::from_bytes(&bytes).unwrap();
        let ts = crate::terrain::tests::synthetic();
        assert!(!map.modified());
        assert_eq!(map.to_bytes(&ts), bytes);
        assert_eq!(map.reencoded(), bytes);
    }

    #[test]
    fn edits_rewrite_only_their_sections() {
        let bytes = build::Map::default().bytes();
        let mut map = Map::from_bytes(&bytes).unwrap();
        let ts = crate::terrain::tests::synthetic();
        map.state.description = "A much longer description than fits".into();
        map.state.gold[0] = 5000;
        map.state.units.push(PlacedUnit {
            x: 1,
            y: 2,
            kind: 0x5e,
            owner: 0,
            data: 1,
        });
        let out = Map::from_bytes(&map.to_bytes(&ts)).unwrap();
        assert_eq!(out.state.description, "A much longer description than ");
        assert_eq!(out.state.gold[0], 5000);
        assert_eq!(out.state.units.len(), 1);
        // The tiles didn't change, so the movement map is the file's own.
        assert_eq!(out.section(b"SQM "), map.section(b"SQM "));
    }

    #[test]
    fn changed_tiles_derive_movement_and_regions() {
        let bytes = build::Map::default().bytes();
        let mut map = Map::from_bytes(&bytes).unwrap();
        let ts = crate::terrain::tests::synthetic();
        map.state.tiles[0] = 0x10;
        let out = Map::from_bytes(&map.to_bytes(&ts)).unwrap();
        assert_eq!(out.state.tiles[0], 0x10);
        let pud = Pud::parse(&map.to_bytes(&ts)).unwrap();
        assert_eq!(pud.movement().unwrap(), derived::movement(&ts, &out.state.tiles));
        assert!(pud.regions().is_some());
    }

    #[test]
    fn new_maps_load() {
        let ts = crate::terrain::tests::synthetic();
        for era in [Era::Forest, Era::Swamp] {
            let m = Map::new(64, era, 0x50).unwrap();
            let pud = Pud::parse(&m.to_bytes(&ts)).unwrap();
            assert_eq!((pud.width, pud.era, pud.tiles[100]), (64, era, 0x50));
            assert_eq!(pud.owners()[..2], [owner::PERSON, owner::COMPUTER]);
        }
        assert!(Map::new(50, Era::Forest, 0x50).is_err());
    }
}
