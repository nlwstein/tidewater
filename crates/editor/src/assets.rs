//! The installation's art for one era, as the editor draws it: the tileset's megatiles in one atlas texture, each
//! megatile's average colour for the minimap, and the unit sprites in player colours.
//!
//! Sprites are the sheets `tidewater_formats::sprites` names. A sprite is drawn centred on its unit's footprint, its
//! cell's centre on the footprint's centre, as the game draws it (`tidewater_sim`'s unit centre). Team colours: the
//! sheets draw them with palette indices 208 to 211, which a player's colour moves to 208 + 4 × player for players
//! 0 to 6 and to 12 for player 7 and the neutral player (the client's `view::team_base`).

use std::collections::HashMap;

use anyhow::{Context, Result};
use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use tidewater_formats::grp::Grp;
use tidewater_formats::install::Install;
use tidewater_formats::pud::Era;
use tidewater_formats::sprites::unit_sheet_candidates;
use tidewater_formats::tileset::{TILE, Tileset};
use tidewater_formats::udta::{UnitStats, UnitTable};

use crate::terrain::Rules;

/// Megatiles per atlas row.
pub const ATLAS_COLUMNS: usize = 32;
const RAMP: u8 = 208;

/// The first palette index of a player's team colours.
pub fn team_base(owner: u8) -> u8 {
    match owner {
        0..=6 => RAMP + 4 * owner,
        7 | 15 => 12,
        _ => RAMP,
    }
}

/// A sprite ready to draw: its texture and the cell size it was cut from.
pub struct Sprite {
    pub texture: TextureHandle,
    /// The frame's offset in its cell, and its size, in map pixels.
    pub offset: (f32, f32),
    pub size: (f32, f32),
    pub cell: (f32, f32),
}

pub struct Assets {
    pub era: Era,
    pub tileset: Tileset,
    pub rules: Rules,
    pub stats: Vec<UnitStats>,
    pub atlas: TextureHandle,
    /// Average colour of each megatile.
    pub colours: Vec<Color32>,
    sheets: HashMap<u8, Grp>,
    sprites: HashMap<(u8, u8), Option<Sprite>>,
}

/// The atlas position (column, row) of a megatile.
pub fn atlas_cell(m: usize) -> (usize, usize) {
    (m % ATLAS_COLUMNS, m / ATLAS_COLUMNS)
}

impl Assets {
    /// Loads an era's art. `udta` is the map's own unit data section, when it has one (it can change footprints).
    pub fn load(ctx: &egui::Context, inst: &mut Install, era: Era, udta: Option<&[u8]>) -> Result<Self> {
        let tileset = inst
            .tileset(era)
            .with_context(|| format!("reading the {era:?} tileset"))?;
        let rules = Rules::new(&tileset);
        let stats = match udta.map(UnitTable::from_udta).transpose()?.flatten() {
            Some(t) => t.0,
            None => UnitTable::from_table(&inst.read("rez\\unitdata.dat")?)?.0,
        };
        let (atlas_img, colours) = atlas(&tileset);
        let atlas = ctx.load_texture(
            "tileset",
            atlas_img,
            TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                ..Default::default()
            },
        );
        let sheets = (0..tidewater_formats::udta::UNIT_TYPES as u8)
            .filter_map(|k| {
                unit_sheet_candidates(k, era).into_iter().find_map(|name| {
                    let g = Grp::parse(&inst.read(&name).ok()?).ok()?;
                    (!g.frames.is_empty()).then_some((k, g))
                })
            })
            .collect();
        Ok(Self {
            era,
            tileset,
            rules,
            stats,
            atlas,
            colours,
            sheets,
            sprites: HashMap::new(),
        })
    }

    /// The sprite of `kind` in `owner`'s colours: a building's first frame, a unit facing south. `None` for types
    /// without a sheet.
    pub fn sprite(&mut self, ctx: &egui::Context, kind: u8, owner: u8) -> Option<&Sprite> {
        let (sheets, pal) = (&self.sheets, &self.tileset.palette);
        let stats = &self.stats;
        self.sprites
            .entry((kind, owner))
            .or_insert_with(|| {
                let g = sheets.get(&kind)?;
                let building = stats
                    .get(kind as usize)
                    .is_some_and(|s| s.flags & tidewater_formats::udta::flags::BUILDING != 0);
                let f = if !building && g.frames.len() >= 5 { 4 } else { 0 };
                let fr = &g.frames[f];
                let base = team_base(owner);
                let mut img = ColorImage::filled(
                    [fr.width.max(1) as usize, fr.height.max(1) as usize],
                    Color32::TRANSPARENT,
                );
                for (i, &v) in fr.pixels.iter().enumerate() {
                    if v == 0 {
                        continue;
                    }
                    let v = if (RAMP..RAMP + 4).contains(&v) {
                        base + (v - RAMP)
                    } else {
                        v
                    };
                    let [r, gg, b] = pal.0[v as usize];
                    img.pixels[i] = Color32::from_rgb(r, gg, b);
                }
                let texture = ctx.load_texture(format!("unit-{kind}-{owner}"), img, TextureOptions::NEAREST);
                Some(Sprite {
                    texture,
                    offset: (fr.x as f32, fr.y as f32),
                    size: (fr.width as f32, fr.height as f32),
                    cell: (g.width as f32, g.height as f32),
                })
            })
            .as_ref()
    }

    /// The megatile a tile number draws (0 when the tileset doesn't define it).
    pub fn megatile(&self, tile: u16) -> usize {
        self.tileset.megatile_for(tile).unwrap_or(0)
    }

    /// The atlas size in pixels.
    pub fn atlas_size(&self) -> (f32, f32) {
        let s = self.atlas.size();
        (s[0] as f32, s[1] as f32)
    }
}

/// Draws every megatile into one image, [`ATLAS_COLUMNS`] to a row, and averages each one's colour.
fn atlas(ts: &Tileset) -> (ColorImage, Vec<Color32>) {
    let n = ts.megatiles.len().max(1);
    let rows = n.div_ceil(ATLAS_COLUMNS);
    let (w, h) = (ATLAS_COLUMNS * TILE, rows * TILE);
    let mut img = ColorImage::filled([w, h], Color32::BLACK);
    let mut colours = Vec::with_capacity(n);
    let mut px = vec![0u8; TILE * TILE];
    for m in 0..ts.megatiles.len() {
        ts.draw_megatile(m, &mut px, TILE);
        let (cx, cy) = atlas_cell(m);
        let mut sum = [0u32; 3];
        for y in 0..TILE {
            for x in 0..TILE {
                let [r, g, b] = ts.palette.0[px[y * TILE + x] as usize];
                img.pixels[(cy * TILE + y) * w + cx * TILE + x] = Color32::from_rgb(r, g, b);
                sum[0] += u32::from(r);
                sum[1] += u32::from(g);
                sum[2] += u32::from(b);
            }
        }
        let k = (TILE * TILE) as u32;
        colours.push(Color32::from_rgb(
            (sum[0] / k) as u8,
            (sum[1] / k) as u8,
            (sum[2] / k) as u8,
        ));
    }
    (img, colours)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_colours() {
        assert_eq!([0, 1, 6, 7, 15].map(team_base), [208, 212, 232, 12, 12]);
        assert_eq!(atlas_cell(33), (1, 1));
    }
}
