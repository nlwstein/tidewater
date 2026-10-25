//! The map view's geometry: which part of the map is in view, at what zoom, and where on the screen
//! (`docs/design/client-core.md`, migration step 2). It replaces the Bevy camera that used to hold the view's centre
//! and turn pointer positions into map positions; nothing here depends on an engine.
//!
//! All positions are in map pixels with y down, the screen's are in screen pixels (640 × 480). The zoom is map pixels
//! per screen pixel (`display`): 1, as the original, unless `--free-zoom`.
//!
//! The camera's timing is kept exactly, so that the game runs the same:
//! - The view's size is known only from the end of the first frame the view exists in ([`MapView::end_frame`]).
//!   Before that, the view counts as one screen pixel across for the game cycle and the frame's drawing, and as
//!   nothing across when it is kept on the map.
//! - The pointer is turned into a map position with the view as it was drawn at the end of the last frame, so a view
//!   moved this frame moves the map under the pointer on the next.

use crate::screen::ViewGeom;
use tidewater_sim::TILE_PX;

/// A rectangle of screen pixels: left, top, width, height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl ScreenRect {
    fn centre(&self) -> (f32, f32) {
        (self.x as f32 + self.w as f32 / 2.0, self.y as f32 + self.h as f32 / 2.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapView {
    /// The map pixel at the view's centre.
    pub centre: (f32, f32),
    /// Map pixels per screen pixel.
    pub zoom: f32,
    /// The view's place on the screen, once the interface is laid out.
    pub rect: Option<ScreenRect>,
    /// Whether a frame has ended since the view was made: its size is known from then on.
    ready: bool,
    /// The centre and zoom as the last frame drew them.
    drawn: ((f32, f32), f32),
}

impl MapView {
    pub fn new(centre: (f32, f32)) -> MapView {
        MapView {
            centre,
            zoom: 1.0,
            rect: None,
            ready: false,
            drawn: ((0.0, 0.0), 1.0),
        }
    }

    /// The end of a frame: the view as it is now is what the frame drew.
    pub fn end_frame(&mut self) {
        self.ready = true;
        self.drawn = (self.centre, self.zoom);
    }

    /// The view's size in map pixels, or `fallback` screen pixels each way while it isn't known.
    fn size(&self, fallback: f32) -> (f32, f32) {
        match self.rect.filter(|_| self.ready) {
            Some(r) => (r.w as f32 * self.zoom, r.h as f32 * self.zoom),
            None => (fallback * self.zoom, fallback * self.zoom),
        }
    }

    /// The view in tiles: left, top, right, bottom.
    pub fn tiles(&self) -> (f32, f32, f32, f32) {
        let (w, h) = self.size(1.0);
        let (hx, hy) = (w / 2.0, h / 2.0);
        let t = TILE_PX as f32;
        (
            (self.centre.0 - hx) / t,
            (self.centre.1 - hy) / t,
            (self.centre.0 + hx) / t,
            (self.centre.1 + hy) / t,
        )
    }

    /// The map pixels in view, clipped to a map `map` pixels big: left, top, right and bottom (exclusive).
    pub fn pixels(&self, map: (i32, i32)) -> (i32, i32, i32, i32) {
        let (l, t, r, b) = self.tiles();
        let p = TILE_PX as f32;
        (
            ((l * p).floor() as i32).clamp(0, map.0),
            ((t * p).floor() as i32).clamp(0, map.1),
            ((r * p).ceil() as i32).clamp(0, map.0),
            ((b * p).ceil() as i32).clamp(0, map.1),
        )
    }

    /// The view position, its size in tiles and the map's (`map` in tiles).
    pub fn geom(&self, map: (u32, u32)) -> ViewGeom {
        let (l, t, r, b) = self.tiles();
        let size = |n: f32| {
            let n = n.round() as i32;
            // Before the view's size is known: the original screen's view.
            if n < 1 { crate::minimap::VIEW_TILES } else { n }
        };
        ViewGeom {
            pos: (l.round() as i32, t.round() as i32),
            tiles: (size(r - l), size(b - t)),
            map: (map.0 as i32, map.1 as i32),
        }
    }

    /// Moves the view so that its top-left tile is `pos`; [`MapView::keep_on_map`] keeps it on the map.
    pub fn move_to(&mut self, pos: (i32, i32)) {
        let (l, t, _, _) = self.tiles();
        let px = TILE_PX as f32;
        self.centre.0 += (pos.0 as f32 - l) * px;
        self.centre.1 += (pos.1 as f32 - t) * px;
    }

    /// Keeps the view on a map `map` pixels big and on the tile grid: the view position is a whole tile (screen spec,
    /// "Layout"), so its top-left corner goes onto the grid first; a map narrower than the view is centred.
    pub fn keep_on_map(&mut self, map: (f32, f32)) {
        let (w, h) = self.size(0.0);
        let (hx, hy) = (w / 2.0, h / 2.0);
        let t = TILE_PX as f32;
        let left = ((self.centre.0 - hx) / t).round() * t;
        let top = ((self.centre.1 - hy) / t).round() * t;
        self.centre = (keep_on_map(left + hx, hx, map.0), keep_on_map(top + hy, hy, map.1));
    }

    /// The view as the last frame drew it, which pointer positions are read against: its place on the screen, centre
    /// and zoom; `None` before a frame has ended. A pointer at screen pixel `p` is over map pixel
    /// `centre + (p - rect's centre) × zoom` ([`MapView::pointer_map`]).
    pub fn drawn(&self) -> Option<(ScreenRect, (f32, f32), f32)> {
        let r = self.rect.filter(|_| self.ready)?;
        Some((r, self.drawn.0, self.drawn.1))
    }

    /// The map position under a pointer at screen pixel `p` (not kept on the screen), by the view the last frame drew.
    pub fn pointer_map(&self, p: (f32, f32)) -> Option<(f32, f32)> {
        let (r, (mx, my), zoom) = self.drawn()?;
        let (cx, cy) = r.centre();
        Some((mx + (p.0 - cx) * zoom, my + (p.1 - cy) * zoom))
    }

    /// The screen pixel over map pixel `m`, by the view the last frame drew: the inverse of [`MapView::pointer_map`].
    pub fn screen_of_map(&self, m: (f32, f32)) -> Option<(f32, f32)> {
        let (r, (mx, my), zoom) = self.drawn()?;
        let (cx, cy) = r.centre();
        let z = zoom.max(0.001);
        Some((cx + (m.0 - mx) / z, cy + (m.1 - my) / z))
    }
}

/// A view centre on one axis that keeps a view of half-width `half` inside a map `len` long; a map narrower than
/// the view is centred.
pub fn keep_on_map(c: f32, half: f32, len: f32) -> f32 {
    if 2.0 * half >= len {
        len / 2.0
    } else {
        c.clamp(half, len - half)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laid_out(centre: (f32, f32)) -> MapView {
        let mut v = MapView::new(centre);
        v.rect = Some(ScreenRect {
            x: 176,
            y: 16,
            w: 448,
            h: 448,
        });
        v
    }

    #[test]
    fn the_view_stays_on_the_map() {
        assert_eq!(keep_on_map(10.0, 100.0, 1000.0), 100.0);
        assert_eq!(keep_on_map(990.0, 100.0, 1000.0), 900.0);
        assert_eq!(keep_on_map(500.0, 100.0, 1000.0), 500.0);
        assert_eq!(keep_on_map(10.0, 600.0, 1000.0), 500.0, "a small map is centred");
    }

    #[test]
    fn the_size_is_known_after_the_first_frame() {
        let mut v = laid_out((640.0, 640.0));
        // One screen pixel across until then.
        assert_eq!(v.tiles(), (639.5 / 32.0, 639.5 / 32.0, 640.5 / 32.0, 640.5 / 32.0));
        assert_eq!(v.pointer_map((400.0, 240.0)), None);
        v.end_frame();
        assert_eq!(v.tiles(), (13.0, 13.0, 27.0, 27.0));
        assert_eq!(
            v.geom((64, 64)),
            ViewGeom {
                pos: (13, 13),
                tiles: (14, 14),
                map: (64, 64)
            }
        );
    }

    /// A widescreen view (`crate::widescreen`): the 864 pixel screen's view is 21 tiles across, and a pointer past the
    /// old 640 pixel edge is over map, by the wide window's mapping of window points to screen pixels.
    #[test]
    fn a_wide_view_maps_the_pointer_past_the_old_edge() {
        let mut v = MapView::new((656.0, 640.0));
        v.rect = Some(ScreenRect {
            x: 176,
            y: 16,
            w: 672,
            h: 448,
        });
        v.end_frame();
        assert_eq!(
            v.geom((64, 64)),
            ViewGeom {
                pos: (10, 13),
                tiles: (21, 14),
                map: (64, 64)
            }
        );
        // The wide window: 1728 x 960 physical pixels at scale 2, the screen 864 pixels across.
        let mut w = crate::input::WindowGeom::new((1728, 960), 1.0);
        w.screen = (864, 480);
        let lb = w.letterbox();
        assert_eq!((lb.scale, lb.offset), (2.0, (0, 0)));
        // Window point (1700, 400) is screen pixel (850, 200): past 640, in the view (176 to 848 is the view).
        let (sx, sy) = lb.screen_point((1690.0, 400.0), w.factor);
        assert_eq!((sx, sy), (845.0, 200.0));
        // The view's centre is screen pixel (512, 240).
        assert_eq!(v.pointer_map((512.0, 240.0)), Some((656.0, 640.0)));
        assert_eq!(v.pointer_map((sx, sy)), Some((656.0 + (845.0 - 512.0), 640.0 - 40.0)));
        // The far right of the view (pixel 847) is 21 tiles from the left edge: map pixel 10 * 32 + 672 - 1.
        assert_eq!(v.pointer_map((847.0, 240.0)).map(|p| p.0), Some(10.0 * 32.0 + 671.0));
        // Through the 640 screen's mapping the same window point would have been clamped to the old edge.
        let old = crate::input::WindowGeom::new((1728, 960), 1.0).letterbox();
        assert!(old.screen_point((1690.0, 400.0), 1.0).0 > 639.0);
    }

    #[test]
    fn the_pointer_uses_the_view_the_last_frame_drew() {
        let mut v = laid_out((640.0, 640.0));
        v.end_frame();
        // The view's centre is screen pixel (400, 240).
        assert_eq!(v.pointer_map((400.0, 240.0)), Some((640.0, 640.0)));
        assert_eq!(v.pointer_map((176.0, 16.0)), Some((416.0, 416.0)));
        v.move_to((20, 13));
        assert_eq!(v.centre, (864.0, 640.0));
        assert_eq!(v.pointer_map((400.0, 240.0)), Some((640.0, 640.0)));
        v.end_frame();
        assert_eq!(v.pointer_map((400.0, 240.0)), Some((864.0, 640.0)));
    }

    #[test]
    fn the_view_is_kept_on_the_map_and_the_grid() {
        let mut v = laid_out((100.0, 5000.0));
        v.end_frame();
        v.keep_on_map((2048.0, 2048.0));
        assert_eq!(v.centre, (224.0, 1824.0));
        // A map narrower than the view is centred.
        v.keep_on_map((320.0, 2048.0));
        assert_eq!(v.centre.0, 160.0);
        // Off the grid: the top-left corner goes to the nearest tile.
        let mut v = laid_out((650.0, 670.0));
        v.end_frame();
        v.keep_on_map((2048.0, 2048.0));
        assert_eq!(v.centre, (640.0, 672.0));
    }
}
