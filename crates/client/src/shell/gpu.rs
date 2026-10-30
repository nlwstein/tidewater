//! The GPU path of the screen's composition (`tidewater_client::compose`): the core's draw list drawn with the
//! renderer's textures instead of composed on the CPU. Each picture is a texture uploaded only when its revision
//! changes, and then only the parts that changed when the change says which. A picture too big for a texture of its
//! own (the terrain, the whole map) is held only for the part on the screen, uploaded again whole when that part
//! moves. The textures of pictures no longer drawn are let go after a while.
//!
//! The GPU samples the nearest texel at each pixel's centre as the CPU path does, and a tinted picture is uploaded
//! with the CPU's tint already on its colours. One thing differs: a half-transparent texel is blended in sRGB rather
//! than linear light.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use tidewater_client::compose::{Item, Pic, Source, cpu};
use tidewater_client::core::Core;

use crate::sdl::{Renderer, TextureId};

/// The biggest picture held whole in one texture, in pixels (a little more than a widescreen screen); a bigger one
/// (the terrain) holds only its part on the screen, as uploading all of it again whenever it changes (the colour
/// cycling's water) costs too much where texture memory is slow to write (the PS Vita: about 16 MB at 180 MB/s).
const WHOLE_MAX: u32 = 1 << 20;
/// Frames a texture is kept after its picture was last drawn.
const KEEP_FRAMES: u64 = 120;

/// A picture, its revision and the parts its last change drew.
type Picture<'a> = (Pic<'a>, u64, Option<(u64, &'a [(u32, u32, u32, u32)])>);

/// The part of `rect` inside `within` (both x, y, width, height), if any.
fn clip_part(rect: (u32, u32, u32, u32), within: (u32, u32, u32, u32)) -> Option<(u32, u32, u32, u32)> {
    let (x0, y0) = (rect.0.max(within.0), rect.1.max(within.1));
    let (x1, y1) = (
        (rect.0 + rect.2).min(within.0 + within.2),
        (rect.1 + rect.3).min(within.1 + within.3),
    );
    (x0 < x1 && y0 < y1).then_some((x0, y0, x1 - x0, y1 - y0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Picture(u64),
    Pointer,
}

struct Entry {
    texture: TextureId,
    /// The texture's size.
    size: (u32, u32),
    /// The picture's revision and the part of it (x, y, width, height in texels) the texture holds.
    revision: u64,
    part: (u32, u32, u32, u32),
    /// The tint its colours were uploaded with (its bits).
    tint: u32,
    used: u64,
}

#[derive(Default)]
pub struct Gpu {
    entries: HashMap<Key, Entry>,
    frame: u64,
}

impl Gpu {
    /// Draws the core's draw list into the renderer's screen ([`Renderer::begin_layers`]); `false` if the renderer
    /// can't, and the caller shows the CPU's frame instead.
    pub fn draw(&mut self, r: &mut Renderer, core: &Core) -> bool {
        if !r.begin_layers() {
            return false;
        }
        self.frame += 1;
        let list = &core.draw_list;
        let screen = (list.width as i32, list.height as i32);
        for item in &list.items {
            let (key, pic, revision, changed) = match item.source {
                Source::Picture { id, revision } => {
                    let Some((pic, _)) = core.picture(id) else { continue };
                    (Key::Picture(id), pic, revision, core.picture_changed(id))
                }
                Source::Pointer => {
                    let Some(p) = &list.pointer else { continue };
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    (p.width, p.height, &p.rgba).hash(&mut h);
                    (Key::Pointer, p.pic(), h.finish(), None)
                }
            };
            self.draw_item(r, key, (pic, revision, changed), item, screen);
        }
        let frame = self.frame;
        self.entries.retain(|_, e| {
            let keep = frame - e.used <= KEEP_FRAMES;
            if !keep {
                r.destroy_texture(e.texture);
            }
            keep
        });
        true
    }

    /// Draws one item of the list: `picture` is its picture, revision and the parts its last change drew
    /// ([`Core::picture_changed`]).
    fn draw_item(&mut self, r: &mut Renderer, key: Key, picture: Picture, item: &Item, screen: (i32, i32)) {
        let (pic, revision, changed) = picture;
        if pic.width == 0 || pic.height == 0 || item.scale.0 <= 0.0 || item.scale.1 <= 0.0 {
            return;
        }
        let clip = item.clip.on_frame(screen.0 as usize, screen.1 as usize);
        if clip.x0 >= clip.x1 || clip.y0 >= clip.y1 {
            return;
        }
        // The texels whose pixels fall inside the clip.
        let span = |lo: i32, hi: i32, origin: f64, scale: f64, len: u32| {
            let a = ((lo as f64 + 0.5 - origin) / scale).floor().max(0.0) as u32;
            let b = (((hi as f64 - 0.5 - origin) / scale).floor() + 1.0).clamp(0.0, len as f64) as u32;
            (a.min(len), b)
        };
        let (tx0, tx1) = span(clip.x0, clip.x1, item.origin.0, item.scale.0, pic.width);
        let (ty0, ty1) = span(clip.y0, clip.y1, item.origin.1, item.scale.1, pic.height);
        if tx0 >= tx1 || ty0 >= ty1 {
            return;
        }
        let whole = pic.width as u64 * pic.height as u64 <= WHOLE_MAX as u64;
        let part = if whole {
            (0, 0, pic.width, pic.height)
        } else {
            (tx0, ty0, tx1 - tx0, ty1 - ty0)
        };
        let fits = |e: &Entry| {
            if whole {
                e.size == (pic.width, pic.height)
            } else {
                e.size.0 >= part.2 && e.size.1 >= part.3
            }
        };
        if self.entries.get(&key).is_some_and(|e| !fits(e)) {
            let e = self.entries.remove(&key).expect("checked");
            r.destroy_texture(e.texture);
        }
        let e = match self.entries.entry(key) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                // A part's texture is made a little bigger than the part, so a view that grows by a few pixels keeps
                // it.
                let size = if whole {
                    (pic.width, pic.height)
                } else {
                    (part.2.next_multiple_of(64), part.3.next_multiple_of(64))
                };
                let Some(texture) = r.create_texture(size) else { return };
                v.insert(Entry {
                    texture,
                    size,
                    revision: revision.wrapping_add(1),
                    part: (0, 0, 0, 0),
                    tint: 1.0f32.to_bits(),
                    used: 0,
                })
            }
        };
        e.used = self.frame;
        let tint = item.tint.to_bits();
        if e.revision != revision || e.part != part || e.tint != tint {
            let pitch = pic.width as usize * 4;
            let at = part.1 as usize * pitch + part.0 as usize * 4;
            if item.tint == 1.0 {
                // What to take, in the picture's pixels: only the parts its last change drew, within the texture's
                // part, when the texture holds that part as it was before that change, else all of the texture's part.
                let all = [part];
                let rects = changed
                    .filter(|&(before, _)| before == e.revision && e.part == part && e.tint == tint)
                    .map_or(&all[..], |(_, parts)| parts);
                for &rect in rects {
                    let Some((x, y, w, h)) = clip_part(rect, part) else {
                        continue;
                    };
                    let at = y as usize * pitch + x as usize * 4;
                    r.update_texture(e.texture, (x - part.0, y - part.1, w, h), &pic.rgba[at..], pitch);
                }
            } else {
                // A tint is put on the colours here, in linear light as the CPU's composition does, rather than by
                // the GPU, which would multiply the sRGB values: a fade is a few frames, the cost is small.
                let table = cpu::tint_table(item.tint);
                let w = part.2 as usize * 4;
                let mut tinted = Vec::with_capacity(w * part.3 as usize);
                for row in pic.rgba[at..].chunks(pitch).take(part.3 as usize) {
                    tinted.extend(
                        row[..w]
                            .chunks_exact(4)
                            .flat_map(|p| [table[p[0] as usize], table[p[1] as usize], table[p[2] as usize], p[3]]),
                    );
                }
                r.update_texture(e.texture, (0, 0, part.2, part.3), &tinted, w);
            }
            e.revision = revision;
            e.part = part;
            e.tint = tint;
        }
        let (sx, sy) = (item.scale.0 as f32, item.scale.1 as f32);
        let dst = (
            item.origin.0 as f32 + part.0 as f32 * sx,
            item.origin.1 as f32 + part.1 as f32 * sy,
            part.2 as f32 * sx,
            part.3 as f32 * sy,
        );
        let src = (0.0, 0.0, part.2 as f32, part.3 as f32);
        let clip = (clip.x0, clip.y0, clip.x1 - clip.x0, clip.y1 - clip.y0);
        r.draw_texture(e.texture, src, dst, clip);
    }
}

#[cfg(test)]
mod tests {
    use super::clip_part;

    #[test]
    fn a_part_is_clipped_to_what_the_texture_holds() {
        let held = (100, 50, 200, 100);
        assert_eq!(clip_part((90, 40, 20, 20), held), Some((100, 50, 10, 10)));
        assert_eq!(clip_part((150, 60, 10, 10), held), Some((150, 60, 10, 10)));
        assert_eq!(clip_part((300, 60, 10, 10), held), None, "just past the right edge");
        assert_eq!(clip_part((0, 0, 400, 400), held), Some(held));
    }
}
