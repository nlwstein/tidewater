//! The CPU's composition of a [`DrawList`] into one [`Frame`], exactly as the module above describes: the frame every
//! scripted run, test and screenshot sees, and what a shell without a GPU path shows.

use super::{DrawList, Frame, Pic, Placed, Source, to_linear, to_srgb};

/// Composes `list` into `frame` (its buffer kept); `picture` finds a [`Source::Picture`]'s picture.
pub fn render<'a>(list: &'a DrawList, picture: impl Fn(u64) -> Option<Pic<'a>>, frame: &mut Frame) {
    frame.clear_black(list.width, list.height);
    for item in &list.items {
        let pic = match item.source {
            Source::Picture { id, .. } => picture(id),
            Source::Pointer => list.pointer.as_ref().map(|p| p.pic()),
            Source::Mark => {
                if let Some(m) = &list.mark {
                    frame.mix(m.pic(), (item.origin.0.floor() as i32, item.origin.1.floor() as i32));
                }
                continue;
            }
        };
        let Some(pic) = pic else { continue };
        frame.draw(&Placed {
            rgba: pic.rgba,
            width: pic.width,
            height: pic.height,
            origin: item.origin,
            scale: item.scale,
            clip: item.clip,
            tint: item.tint,
        });
    }
}

/// A tint's multiplication of an 8-bit sRGB channel, in linear light.
fn tinted(table: &[u8; 256], c: u8) -> u8 {
    table[c as usize]
}

/// Each 8-bit sRGB value multiplied by `tint` in linear light, as a tinted picture is drawn.
pub fn tint_table(tint: f32) -> [u8; 256] {
    let k = to_linear(tint.clamp(0.0, 1.0));
    std::array::from_fn(|c| {
        (to_srgb(to_linear(c as f32 / 255.0) * k) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    })
}

/// `to_linear` of each 8-bit sRGB value, as [`put`]'s blending uses it.
fn linear_table() -> &'static [f32; 256] {
    static TABLE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|c| to_linear(c as f32 / 255.0)))
}

/// One texel `s` over one frame pixel `d` (both RGBA): a transparent texel leaves it, an opaque one replaces its
/// colour, any other is blended in linear light. `table`: the tint.
#[inline(always)]
fn put(d: &mut [u8], s: &[u8], table: Option<&[u8; 256]>) {
    let a = s[3];
    if a == 0 {
        return;
    }
    let mut rgb = [s[0], s[1], s[2]];
    if let Some(t) = table {
        rgb = rgb.map(|c| tinted(t, c));
    }
    if a == 255 {
        d[..3].copy_from_slice(&rgb);
        return;
    }
    let lin = linear_table();
    let a = a as f32 / 255.0;
    for k in 0..3 {
        let l = lin[rgb[k] as usize] * a + lin[d[k] as usize] * (1.0 - a);
        d[k] = (to_srgb(l) * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

impl Frame {
    /// Makes the frame all black at `width` × `height`, keeping its buffer.
    pub fn clear_black(&mut self, width: usize, height: usize) {
        self.rgba.clear();
        self.rgba.resize(width * height * 4, 0);
        for px in self.rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        (self.width, self.height) = (width, height);
    }

    /// Mixes `pic`, its top-left corner at pixel `at`, over the frame by its alpha in sRGB values ([`Source::Mark`]):
    /// each channel becomes (s·a + d·(255 − a)) / 255, rounded.
    pub fn mix(&mut self, pic: Pic, at: (i32, i32)) {
        let (w, h) = (pic.width as i32, pic.height as i32);
        let (x0, x1) = (at.0.max(0), (at.0 + w).min(self.width as i32));
        let (y0, y1) = (at.1.max(0), (at.1 + h).min(self.height as i32));
        for y in y0..y1 {
            for x in x0..x1 {
                let s = ((y - at.1) * w + x - at.0) as usize * 4;
                let s = &pic.rgba[s..s + 4];
                let a = s[3] as u32;
                if a == 0 {
                    continue;
                }
                let d = (y as usize * self.width + x as usize) * 4;
                for (d, &s) in self.rgba[d..d + 3].iter_mut().zip(&s[..3]) {
                    *d = ((s as u32 * a + *d as u32 * (255 - a) + 127) / 255) as u8;
                }
            }
        }
    }

    /// Draws a placed picture over the frame.
    pub fn draw(&mut self, p: &Placed) {
        if p.width == 0 || p.height == 0 || p.scale.0 <= 0.0 || p.scale.1 <= 0.0 {
            return;
        }
        let table = (p.tint != 1.0).then(|| tint_table(p.tint));
        let table = table.as_ref();
        let c = p.clip.on_frame(self.width, self.height);
        if c.x0 >= c.x1 || c.y0 >= c.y1 {
            return;
        }
        let width = self.width;
        // The texel under a pixel's centre, if the picture covers it.
        let texel = |px: i32, origin: f64, scale: f64, len: u32| {
            let t = ((px as f64 + 0.5 - origin) / scale).floor();
            (t >= 0.0 && t < len as f64).then_some(t as usize)
        };
        // One picture pixel per screen pixel across: a row's texels are one run, `x + shift` (the centre's floor
        // moves with the whole pixel), cut to the picture.
        let run = (p.scale.0 == 1.0).then(|| {
            let shift = (0.5 - p.origin.0).floor() as i64;
            let x0 = (c.x0 as i64).max(-shift);
            let x1 = (c.x1 as i64).min(p.width as i64 - shift);
            (x0, x1, shift)
        });
        let cols: Vec<u32> = match run {
            Some(_) => Vec::new(),
            None => (c.x0..c.x1)
                .map(|x| texel(x, p.origin.0, p.scale.0, p.width).map_or(u32::MAX, |t| t as u32))
                .collect(),
        };
        let pitch = p.width as usize * 4;
        for y in c.y0..c.y1 {
            let Some(ty) = texel(y, p.origin.1, p.scale.1, p.height) else {
                continue;
            };
            let row = &p.rgba[ty * pitch..(ty + 1) * pitch];
            let out = &mut self.rgba[y as usize * width * 4..(y as usize + 1) * width * 4];
            match run {
                Some((x0, x1, shift)) => {
                    if x0 >= x1 {
                        return;
                    }
                    let src = &row[((x0 + shift) * 4) as usize..((x1 + shift) * 4) as usize];
                    let dst = &mut out[(x0 * 4) as usize..(x1 * 4) as usize];
                    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                        put(d, s, table);
                    }
                }
                None => {
                    let dst = &mut out[c.x0 as usize * 4..c.x1 as usize * 4];
                    for (d, &tx) in dst.chunks_exact_mut(4).zip(&cols) {
                        if tx != u32::MAX {
                            put(d, &row[tx as usize * 4..tx as usize * 4 + 4], table);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mark mixes by its alpha in sRGB values, rounded, leaves what its clear pixels cover, and is cut at the frame's
    /// edges.
    #[test]
    fn a_mark_mixes_in_srgb_values_and_is_cut_at_the_edges() {
        let mut f = Frame::default();
        f.clear_black(4, 1);
        f.rgba[4..8].copy_from_slice(&[200, 100, 0, 255]);
        let rgba = [255, 255, 255, 128, 255, 255, 255, 128, 9, 9, 9, 0];
        let mark = Pic {
            width: 3,
            height: 1,
            rgba: &rgba,
        };
        // Cut on the left: pixel 0 takes texel 1 (255·128/255 rounded = 128), pixel 1 the clear texel 2.
        f.mix(mark, (-1, 0));
        assert_eq!(&f.rgba[..8], &[128, 128, 128, 255, 200, 100, 0, 255]);
        // Over (200, 100, 0): (255·128 + 200·127) / 255 = 228.1, (255·128 + 100·127) / 255 = 178.3, and 128.498.
        f.mix(mark, (1, 0));
        assert_eq!(&f.rgba[4..12], &[228, 178, 128, 255, 128, 128, 128, 255]);
        assert_eq!(&f.rgba[12..], &[0, 0, 0, 255], "under its clear texel");
    }
}
