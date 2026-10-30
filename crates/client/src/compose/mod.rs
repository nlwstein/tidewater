//! The screen's composition, without an engine: pictures placed on the screen (640 × 480, or wider in a game with the
//! `widescreen` option, [`crate::widescreen`]), one over another (`docs/design/client-core.md`). The core lists them in
//! a [`DrawList`] each frame; [`cpu`] composes the list into one [`Frame`] exactly, and a shell may instead draw the
//! list on its GPU (the SDL shell's `gpu`), where the pictures are textures it uploads only when they change.
//!
//! Each picture is sampled at the centre of each screen pixel it covers (nearest texel), a fully transparent texel
//! leaves the pixel as it was, and a tinted picture's colours are multiplied by the tint in linear light, as a GPU
//! sampling an sRGB texture into an sRGB target does.

pub mod cpu;

/// The original screen's size in pixels, and a [`Frame`]'s default; a widescreen game's frame is wider.
pub const W: usize = 640;
pub const H: usize = 480;

/// A screen rectangle in pixels: left, top, right and bottom (exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clip {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Clip {
    /// The whole frame, whatever its size ([`Clip::on_frame`] cuts it to the frame drawn on).
    pub const SCREEN: Clip = Clip {
        x0: 0,
        y0: 0,
        x1: i32::MAX,
        y1: i32::MAX,
    };

    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Clip {
            x0: x,
            y0: y,
            x1: x + w,
            y1: y + h,
        }
    }

    /// The part on a frame `width` × `height` pixels.
    pub fn on_frame(self, width: usize, height: usize) -> Self {
        Clip {
            x0: self.x0.clamp(0, width as i32),
            y0: self.y0.clamp(0, height as i32),
            x1: self.x1.clamp(0, width as i32),
            y1: self.y1.clamp(0, height as i32),
        }
    }
}

/// A picture placed on the screen.
#[derive(Clone, Copy, Debug)]
pub struct Placed<'a> {
    /// RGBA, row by row (sRGB, straight alpha).
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// The screen position of the picture's top-left corner, in pixels (it may fall between pixels).
    pub origin: (f64, f64),
    /// Screen pixels per picture pixel.
    pub scale: (f64, f64),
    /// Only this part of the screen is drawn on.
    pub clip: Clip,
    /// A grey tint (sRGB, 0 to 1): 1 leaves the colours as they are.
    pub tint: f32,
}

/// A picture as a [`DrawList`] reads it: RGBA, row by row (sRGB, straight alpha).
#[derive(Clone, Copy, Debug)]
pub struct Pic<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// An owned picture: the pointer a [`DrawList`] carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pic(&self) -> Pic<'_> {
        Pic {
            width: self.width,
            height: self.height,
            rgba: &self.rgba,
        }
    }
}

/// What an [`Item`] draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// One of the core's pictures (`Core::picture`); `revision` changes whenever the picture does.
    Picture { id: u64, revision: u64 },
    /// The list's [`DrawList::pointer`].
    Pointer,
    /// The list's [`DrawList::mark`], one picture pixel per screen pixel. Its colours are mixed with what is under them
    /// by their alpha in sRGB values, not linear light, as a GPU's blending does, so both compositions give the same
    /// pixels.
    Mark,
}

/// A picture placed on the screen, as [`Placed`] places it.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub source: Source,
    pub origin: (f64, f64),
    pub scale: (f64, f64),
    pub clip: Clip,
    pub tint: f32,
}

/// One frame's screen: `width` × `height` pixels, black, with the items drawn over it in order.
#[derive(Clone, Debug, Default)]
pub struct DrawList {
    pub width: usize,
    pub height: usize,
    pub items: Vec<Item>,
    pub pointer: Option<Image>,
    /// A mark over everything (the touch pulse), the last item.
    pub mark: Option<Image>,
}

/// One whole screen, RGBA (opaque): `width` × `height` pixels, 640 × 480 unless a game has a wider screen.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

impl Default for Frame {
    fn default() -> Self {
        Self::black()
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({:016x})", self.hash())
    }
}

/// sRGB to linear light (the sRGB standard's curve).
pub fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light to sRGB.
pub fn to_srgb(l: f32) -> f32 {
    if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

impl Frame {
    pub fn black() -> Self {
        Self::black_sized(W, H)
    }

    /// An all-black frame `width` × `height` pixels.
    pub fn black_sized(width: usize, height: usize) -> Self {
        let mut rgba = vec![0; width * height * 4];
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Frame { rgba, width, height }
    }

    /// Fills a rectangle with a colour.
    #[cfg(test)]
    pub fn fill(&mut self, clip: Clip, rgb: [u8; 3]) {
        let c = clip.on_frame(self.width, self.height);
        for y in c.y0..c.y1 {
            for x in c.x0..c.x1 {
                let i = (y as usize * self.width + x as usize) * 4;
                self.rgba[i..i + 3].copy_from_slice(&rgb);
                self.rgba[i + 3] = 255;
            }
        }
    }

    /// The frame's fingerprint (FNV-1a over the pixels' colours).
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for px in self.rgba.chunks_exact(4) {
            let v = u32::from_le_bytes([px[0], px[1], px[2], 0]);
            h ^= v as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4] + Copy) -> Vec<u8> {
        (0..h).flat_map(|y| (0..w).flat_map(move |x| f(x, y))).collect()
    }

    fn at(f: &Frame, x: usize, y: usize) -> [u8; 4] {
        let i = (y * W + x) * 4;
        f.rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn a_picture_lands_on_its_pixels_and_transparent_texels_leave_the_frame() {
        let rgba = pic(2, 2, |x, y| {
            if x == 1 && y == 1 {
                [0, 0, 0, 0]
            } else {
                [10, 20, 30, 255]
            }
        });
        let mut f = Frame::black();
        f.fill(Clip::SCREEN, [1, 2, 3]);
        f.draw(&Placed {
            rgba: &rgba,
            width: 2,
            height: 2,
            origin: (5.0, 7.0),
            scale: (1.0, 1.0),
            clip: Clip::SCREEN,
            tint: 1.0,
        });
        assert_eq!(at(&f, 5, 7), [10, 20, 30, 255]);
        assert_eq!(at(&f, 6, 8), [1, 2, 3, 255]);
        assert_eq!(at(&f, 4, 7), [1, 2, 3, 255]);
        assert_eq!(at(&f, 7, 7), [1, 2, 3, 255]);
    }

    #[test]
    fn scaling_repeats_texels_and_the_clip_cuts() {
        let rgba = pic(2, 1, |x, _| [x as u8 * 100, 0, 0, 255]);
        let mut f = Frame::black();
        f.draw(&Placed {
            rgba: &rgba,
            width: 2,
            height: 1,
            origin: (0.0, 0.0),
            scale: (2.0, 2.0),
            clip: Clip::new(0, 0, 3, 480),
            tint: 1.0,
        });
        assert_eq!(at(&f, 1, 1)[0], 0);
        assert_eq!(at(&f, 2, 1)[0], 100);
        assert_eq!(at(&f, 3, 1)[0], 0, "clipped");
        assert_eq!(at(&f, 2, 2), [0, 0, 0, 255], "below the picture");
    }

    #[test]
    fn a_tint_multiplies_in_linear_light() {
        let rgba = pic(1, 1, |_, _| [255, 128, 0, 255]);
        let mut f = Frame::black();
        f.draw(&Placed {
            rgba: &rgba,
            width: 1,
            height: 1,
            origin: (0.0, 0.0),
            scale: (1.0, 1.0),
            clip: Clip::SCREEN,
            tint: 0.5,
        });
        // 0.5 sRGB is about 0.214 linear: full white comes out as 0.5 sRGB again, and 128 (0.216 linear) as 0.046
        // linear, 61 in sRGB; a multiplication in sRGB would have given 64.
        assert_eq!(at(&f, 0, 0), [128, 61, 0, 255]);
    }

    #[test]
    fn a_wider_frame_keeps_its_own_row_length_and_clips_to_its_own_size() {
        let rgba = pic(2, 1, |x, _| [x as u8 * 100 + 50, 0, 0, 255]);
        let mut f = Frame::black_sized(864, 480);
        assert_eq!((f.width, f.height, f.rgba.len()), (864, 480, 864 * 480 * 4));
        // The whole-frame clip reaches the far right and bottom, past where a 640 × 480 frame ends.
        f.draw(&Placed {
            rgba: &rgba,
            width: 2,
            height: 1,
            origin: (862.0, 479.0),
            scale: (1.0, 1.0),
            clip: Clip::SCREEN,
            tint: 1.0,
        });
        let at = |f: &Frame, x: usize, y: usize| f.rgba[(y * 864 + x) * 4..(y * 864 + x) * 4 + 4].to_vec();
        assert_eq!(at(&f, 862, 479), [50, 0, 0, 255]);
        assert_eq!(at(&f, 863, 479), [150, 0, 0, 255]);
        assert_eq!(at(&f, 861, 479), [0, 0, 0, 255]);
        // A picture that hangs over the edge is cut, not wrapped onto the next row.
        f.draw(&Placed {
            rgba: &rgba,
            width: 2,
            height: 1,
            origin: (863.0, 10.0),
            scale: (1.0, 1.0),
            clip: Clip::SCREEN,
            tint: 1.0,
        });
        assert_eq!(at(&f, 863, 10), [50, 0, 0, 255]);
        assert_eq!(at(&f, 0, 11), [0, 0, 0, 255]);
        assert_eq!(at(&f, 0, 10), [0, 0, 0, 255], "nothing wrapped to the next row's start");
    }

    #[test]
    fn the_hash_sees_every_pixel() {
        let a = Frame::black();
        let mut b = Frame::black();
        b.fill(Clip::new(639, 479, 1, 1), [0, 0, 1]);
        assert_ne!(a.hash(), b.hash());
    }
}
