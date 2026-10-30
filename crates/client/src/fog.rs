//! Fog of war as the map view draws it (`docs/specs/ui/map-drawing.md`, "Fog"; the maps themselves are the sim's,
//! `vision/fog.md`, "The fog maps"; the minimap is in `crate::minimap`). No SDL types here.
//!
//! Each tile in view gets the picture for its visible-map code v and explored-map code e: black, a black
//! checkerboard on the even pixels, or the fog shapes of the tileset. Shape c (1 to 15) is bytes 1024·c to
//! 1024·c + 1023 of the tileset's `.vr4` file, read as one 32 × 32 block as the spec says.

use crate::palette::Entries;
use crate::view::Canvas;

/// The fog colour.
pub const BLACK: u8 = 0;
/// The code of a wholly fogged tile, and of a tile with no fog.
pub const ALL: u8 = 16;
pub const NONE: u8 = 0;

/// The fog shapes 0 to 15 (0 is never drawn).
#[derive(Clone, Debug)]
pub struct Shapes(Vec<[u8; 1024]>, Entries);

impl Shapes {
    /// The shapes from a tileset's `.vr4` bytes; missing bytes read as 0 (transparent).
    pub fn from_vr4(vr4: &[u8]) -> Self {
        let shapes: Vec<[u8; 1024]> = (0..16)
            .map(|c| {
                let mut s = [0u8; 1024];
                for (i, b) in s.iter_mut().enumerate() {
                    *b = vr4.get(1024 * c + i).copied().unwrap_or(0);
                }
                s
            })
            .collect();
        // The colours the fog can draw: black and the shapes' (shape 0 is never drawn, see `at`).
        let mut colours = Entries::of_pixels(shapes[1..].as_flattened());
        colours.insert(BLACK);
        Shapes(shapes, colours)
    }

    /// Shape c's non-zero pixel at (x, y) in the tile; `None` where it is transparent or the code has no shape.
    fn at(&self, c: u8, x: usize, y: usize) -> Option<u8> {
        (1..=15)
            .contains(&c)
            .then(|| self.0[c as usize][y * 32 + x])
            .filter(|&p| p != 0)
    }
}

/// What one pixel of a tile gets ("Checkerboard"); `None` leaves it as it is. Pixels where (x + y) is even are the
/// "even pixels".
pub fn tile_pixel(v: u8, e: u8, shapes: &Shapes, x: usize, y: usize) -> Option<u8> {
    let even = (x + y).is_multiple_of(2);
    let shape = |c: u8| shapes.at(c, x, y);
    match (v, e) {
        (NONE, _) => None,
        (ALL, ALL) => Some(BLACK),
        (ALL, NONE) => even.then_some(BLACK),
        (ALL, e) => {
            if even {
                Some(BLACK)
            } else {
                shape(e)
            }
        }
        (v, NONE) => even.then(|| shape(v)).flatten(),
        (v, e) if e == v => shape(e),
        // S(v) on the even pixels, then all of S(e) over it (none for e = 16).
        (v, e) => shape(e).or_else(|| even.then(|| shape(v)).flatten()),
    }
}

/// Draws the fog over the tiles the canvas covers. `codes` holds (v, e) per map tile, row by row, `width` tiles wide.
pub fn draw(canvas: &mut Canvas, width: usize, codes: &[(u8, u8)], shapes: &Shapes) {
    if width == 0 {
        return;
    }
    let height = codes.len() / width;
    // Marked once for all the fog may draw, not pixel by pixel.
    canvas.mark_used(&shapes.1);
    let (tx0, ty0) = (canvas.left.div_euclid(32).max(0), canvas.top.div_euclid(32).max(0));
    let tx1 = ((canvas.left + canvas.width + 31) / 32).min(width as i32);
    let ty1 = ((canvas.top + canvas.height + 31) / 32).min(height as i32);
    for ty in ty0..ty1 {
        for tx in tx0..tx1 {
            let (v, e) = codes[ty as usize * width + tx as usize];
            if v == NONE {
                continue;
            }
            // The tile's pixels inside the canvas, a row at a time.
            let (left, top) = (tx * 32 - canvas.left, ty * 32 - canvas.top);
            let (x0, x1) = ((-left).max(0), (canvas.width - left).min(32));
            let (y0, y1) = ((-top).max(0), (canvas.height - top).min(32));
            if x0 >= x1 {
                continue;
            }
            for y in y0..y1 {
                let at = ((top + y) * canvas.width + left + x0) as usize;
                let row = &mut canvas.px[at..at + (x1 - x0) as usize];
                if (v, e) == (ALL, ALL) {
                    row.fill(BLACK as u16);
                    continue;
                }
                for (x, p) in (x0 as usize..).zip(row) {
                    if let Some(c) = tile_pixel(v, e, shapes, x, y as usize) {
                        *p = c as u16;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::CLEAR;

    /// Shape c holds 100 + c on its top half (rows 0 to 15) and 0 below.
    fn shapes() -> Shapes {
        let mut vr4 = vec![0u8; 16 * 1024];
        for c in 1..16 {
            for i in 0..512 {
                vr4[1024 * c + i] = 100 + c as u8;
            }
        }
        Shapes::from_vr4(&vr4)
    }

    fn tile(v: u8, e: u8) -> Canvas {
        let mut c = Canvas::new(0, 0, 32, 32);
        draw(&mut c, 1, &[(v, e)], &shapes());
        c
    }

    #[test]
    fn explored_out_of_sight_is_a_black_checkerboard() {
        // v = 16, e = 0: black on even pixels, the terrain shows on odd pixels.
        let c = tile(16, 0);
        assert_eq!((c.at(0, 0), c.at(1, 0), c.at(1, 1)), (Some(0), Some(CLEAR), Some(0)));
    }

    #[test]
    fn unexplored_is_all_black() {
        assert!(tile(16, 16).px.iter().all(|&p| p == 0));
    }

    #[test]
    fn a_shape_on_the_even_pixels_only() {
        // v = 2 (top half fogged), e = 0: shape 2's non-zero pixels on the even pixels only.
        let c = tile(2, 0);
        assert_eq!((c.at(0, 0), c.at(1, 0)), (Some(102), Some(CLEAR)));
        assert_eq!(c.at(0, 20), Some(CLEAR), "zero pixels are transparent");
    }

    #[test]
    fn the_other_rows() {
        assert!(tile(0, 7).px.iter().all(|&p| p == CLEAR), "v = 0: nothing");
        let c = tile(16, 3);
        assert_eq!((c.at(0, 0), c.at(1, 0), c.at(1, 20)), (Some(0), Some(103), Some(CLEAR)));
        let c = tile(4, 4);
        assert_eq!((c.at(0, 0), c.at(1, 0)), (Some(104), Some(104)));
        let c = tile(2, 4);
        assert_eq!((c.at(0, 0), c.at(1, 0)), (Some(104), Some(104)), "S(e) over S(v)");
        let c = tile(2, 16);
        assert_eq!((c.at(0, 0), c.at(1, 0)), (Some(102), Some(CLEAR)));
    }

    #[test]
    fn only_tiles_in_view_are_drawn() {
        let mut c = Canvas::new(40, 0, 10, 10);
        draw(&mut c, 2, &[(16, 16), (16, 16)], &shapes());
        assert!(c.px.iter().all(|&p| p == 0));
    }
}
