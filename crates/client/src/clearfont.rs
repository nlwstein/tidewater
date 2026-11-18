//! The `font` option's clear font (`docs/design/extras.md`): plain sans-serif letters in place of the game's own text
//! fonts, for small screens where the game's shaded serif letters are hard to read.
//!
//! **What it replaces.** The two text fonts, `font10x` (the game screen, the small menu text) and `font12x` (the menu
//! screens), for the printable ASCII characters '!' to '~'. Every other character keeps the game's glyph, and the
//! title fonts (`font32`, `font50`) and the hit point digits (`font6`) are left alone.
//!
//! **How it fits.** A clear glyph is built in the game font's own form (`tidewater_formats::fnt`): the letter in shade
//! 1, the brightest face colour of every caller's shade map, and a one-pixel shadow down and to the right in shade 5.
//! It keeps the game font's line height and widest-glyph width, so line spacing and the menus' space width
//! (`menuscreen::label_gap`) are unchanged. The letters are one size, 9-pixel capitals with 2-pixel strokes, about
//! `font10x`'s; in `font12x`'s places (12-pixel capitals) they are smaller than the game's. They stand where the game
//! font's baseline is, so text keeps its place on the line.
//!
//! **Source.** The glyphs are Pixel Operator Bold by Jayvee Enaguas (CC0 1.0, public domain; `THIRD_PARTY_NOTICES.md`),
//! at its 16-pixel design size, the only size that keeps every pixel of it; `tools/gen_clear_font.py` writes
//! `clearfont/glyphs.rs` from the font file.

mod glyphs;

use tidewater_formats::fnt::{Font, Glyph};

use crate::core::Core;

/// The face's shade: shade 1 is the brightest colour in every shade map (`hud::TEXT`, `menuscreen::colours`).
const FACE: u8 = 1;
/// The shadow's shade, black in every shade map.
const SHADOW: u8 = 5;

/// The first and last characters the clear font draws.
const FIRST: u8 = b'!';
const LAST: u8 = b'~';

/// The row of a font's cell the clear letters stand on: row 9 for a line of 16 or fewer pixels (`font10x`, 14, whose
/// own baseline is there), row 10 for a taller one (`font12x`, 17, whose baseline is row 11: one row up keeps the
/// smaller letters' middle where the game's capitals' middle is).
fn baseline(height: u8) -> i32 {
    if height > 16 { 10 } else { 9 }
}

/// Puts the menus' and the game screen's fonts in line with the option, once a frame after the flow's change of
/// screen: a change on the Settings screen shows at once, and a game's interface starts in the chosen font. The game
/// screen's interface is drawn again in full when its font changes.
pub(crate) fn apply(c: &mut Core) {
    let on = c.extras.clear_font();
    c.front.set_clear_font(on);
    if let Some(hud) = c.hud.as_mut()
        && hud.art.set_clear_font(on)
    {
        hud.drawn = None;
    }
}

/// `original` with the clear glyphs in place of its printable ASCII ones.
pub fn clear(original: &Font) -> Font {
    let first = original.first.min(FIRST);
    let last = (original.first as usize + original.glyphs.len())
        .saturating_sub(1)
        .max(LAST as usize) as u8;
    let base = baseline(original.height);
    let glyphs = (first..=last)
        .map(|c| match c {
            FIRST..=LAST => Some(glyph(c, base)),
            _ => original.glyph(c).cloned(),
        })
        .collect();
    Font {
        first,
        max_width: original.max_width,
        height: original.height,
        glyphs,
    }
}

/// The clear glyph of `c` ('!' to '~') standing on row `base` of the cell.
fn glyph(c: u8, base: i32) -> Glyph {
    let &(_, width, top, rows) = &glyphs::GLYPHS[(c - FIRST) as usize];
    let (w, h) = (width as usize + 1, rows.len() + 1);
    let ink = |x: usize, y: usize| y < rows.len() && x < width as usize && rows[y] >> x & 1 == 1;
    let mut shades = vec![0; w * h];
    for y in 0..h {
        for x in 0..w {
            shades[y * w + x] = if ink(x, y) {
                FACE
            } else if x > 0 && y > 0 && ink(x - 1, y - 1) {
                SHADOW
            } else {
                0
            };
        }
    }
    Glyph {
        width: w as u8,
        height: h as u8,
        x: 0,
        y: (base + top as i32).max(0) as u8,
        shades,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for a game font: `height` tall, every character from ' ' to 'ÿ' a 3 × 3 glyph of shade 2 except the
    /// space, which has none.
    fn game_font(height: u8) -> Font {
        let g = Glyph {
            width: 3,
            height: 3,
            x: 1,
            y: 2,
            shades: vec![2; 9],
        };
        Font {
            first: b' ',
            max_width: 17,
            height,
            glyphs: (b' '..=0xff).map(|c| (c != b' ').then(|| g.clone())).collect(),
        }
    }

    #[test]
    fn the_table_covers_every_printable_character_in_order() {
        for (i, &(c, width, _, rows)) in glyphs::GLYPHS.iter().enumerate() {
            assert_eq!(c as u32, FIRST as u32 + i as u32);
            assert!(width > 0 && !rows.is_empty(), "{c:?} has ink");
            assert!(rows.iter().all(|r| r >> width == 0), "{c:?} stays within its width");
        }
    }

    #[test]
    fn printable_characters_are_replaced_and_the_rest_kept() {
        let game = game_font(14);
        let f = clear(&game);
        assert_eq!(
            (f.first, f.height, f.max_width),
            (b' ', 14, 17),
            "line height and widest glyph kept"
        );
        assert_eq!(f.glyphs.len(), game.glyphs.len());
        assert!(f.glyph(b' ').is_none(), "the space stays a gap");
        assert_ne!(f.glyph(b'A'), game.glyph(b'A'));
        assert_eq!(
            f.glyph(0xe9),
            game.glyph(0xe9),
            "an accented letter keeps the game's glyph"
        );
        assert_eq!(f.glyph(0x7f), game.glyph(0x7f));
    }

    #[test]
    fn a_glyph_is_a_flat_face_with_a_shadow_down_and_right() {
        let f = clear(&game_font(14));
        // '|' is a 2 × 9 bar from the cap line to the baseline.
        let g = f.glyph(b'|').unwrap();
        assert_eq!((g.width, g.height, g.x, g.y), (3, 10, 0, 1));
        let row = |y: usize| &g.shades[y * 3..y * 3 + 3];
        assert_eq!(row(0), [FACE, FACE, 0]);
        assert_eq!(row(4), [FACE, FACE, SHADOW]);
        assert_eq!(row(9), [0, SHADOW, SHADOW]);
        assert!(g.shades.iter().all(|&s| [0, FACE, SHADOW].contains(&s)));
    }

    #[test]
    fn letters_stand_on_the_game_fonts_baselines() {
        // 'H' has no descender: its last ink row is the baseline.
        for (height, base) in [(14, 9), (17, 10)] {
            let g = clear(&game_font(height)).glyph(b'H').unwrap().clone();
            assert_eq!(g.y as i32 + g.height as i32 - 2, base, "line height {height}");
        }
    }

    #[test]
    fn every_glyph_and_its_shadow_fit_the_line() {
        for height in [14, 17] {
            let f = clear(&game_font(height));
            for c in FIRST..=LAST {
                let g = f.glyph(c).unwrap();
                assert!(
                    g.y as u32 + g.height as u32 <= height as u32,
                    "{:?} in a line of {height}",
                    c as char
                );
            }
        }
    }

    #[test]
    fn the_advance_is_the_ink_the_shadow_and_a_pixel() {
        // The advance is the ink, the shadow and one pixel: 'i' (2 wide) 4, 'm' (7 wide) 9.
        let f = clear(&game_font(14));
        assert_eq!(f.advance(b'i'), 4);
        assert_eq!(f.advance(b'm'), 9);
    }
}
