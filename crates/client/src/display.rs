//! How the 640 × 480 screen (spec `docs/specs/ui/screen-and-cursors.md`, "Layout") goes into the window: the window
//! size, the whole-number scale and the black letterbox around it, the pointer's way back to screen pixels, and the
//! map view's zoom. No SDL types here.
//!
//! Our choices (the original runs full screen at 640 × 480, so it has none of these):
//! - **Scale.** The screen is drawn at a whole number of physical pixels per screen pixel, the largest that fits the
//!   window, and is never smaller than 640 × 480 or squashed. The rest of the window is black, centred as a full-screen
//!   game would be. The default window is the largest such scale that fits the monitor, less [`RESERVE`] points for
//!   the menu bar, title bar and task bar.
//! - **Pointer.** A pointer over the black border is taken to be at the nearest screen edge, as the original's pointer
//!   stops at the screen's edge: pushing it into the border scrolls the view.
//! - **Zoom.** The map view draws one map pixel per screen pixel, as the original does. The `--free-zoom` development
//!   option (not part of the original) zooms with the mouse wheel in powers of two, from [`MIN_ZOOM`] to [`MAX_ZOOM`]
//!   map pixels per screen pixel. The `map-zoom` option (`game::pan_and_zoom`) is the player's version: the wheel over the
//!   map view magnifies it from 1 to 4 times and back, never showing more map than the original.
//! - **Mouse lock.** In a game the pointer is kept in the window, as a full-screen game's pointer can't leave the
//!   screen, so pushing it against an edge scrolls the view however fast the mouse moves (as StarCraft's windowed
//!   "W-Mode" and Diablo II: Resurrected's "lock cursor" option do). [`lock_wanted`] says when: only while a game runs (not while a game menu or the outcome dialog is
//!   up, which use the system's pointer), only
//!   while the window has the focus (switching away, Cmd-Tab, frees it; a click back in locks it again), and not while
//!   Cmd (the Windows key elsewhere) is held. Where the system can confine the pointer to the window, it does; macOS
//!   can't, so there the system pointer is held still and the game's own pointer moves by the mouse's motion, kept on
//!   the screen ([`Letterbox::keep_on_screen`]). `--no-mouse-lock` turns it off.
//! - **Fractional scale** (client-only, not part of the original). Touch platforms fit the screen by a fraction of
//!   the window's safe area instead of a whole number, drawn "sharp bilinear": the picture is first scaled up by a
//!   whole number with the nearest texel and then scaled linearly down to the fractional size, so pixels stay crisp
//!   without a thick uneven edge ([`Letterbox::fit_area`], [`sharp_steps`]). Desktop follows the `scaling` extra
//!   ([`Fit`]): `auto` (the default) keeps the whole-number scale when it covers at least [`AUTO_COVER`] (90 %) of the
//!   limiting dimension of the safe area and otherwise fills it fractionally, so a window sized to a whole scale looks
//!   as it always did while a full screen or a resized window with a poor whole fit fills; `integer` is always whole
//!   numbers, `sharp` always fractional ([`fractional_for`]). The safe area is the part of the window the system doesn't cover (a phone's rounded
//!   corners, notch and home bar).
//! - **Widescreen** (client-only, not part of the original; [`crate::widescreen`]). A game may have a screen wider than
//!   640 pixels (the `widescreen` option). The [`Letterbox`] then carries the screen's size, so the scale, the black
//!   border, the pointer's way back to screen pixels and its clamp to the screen all use it; the 640 × 480 pictures of
//!   the menus and dialogs sit in the middle of it ([`Letterbox::flow_of`]). The window grows to hold the wider screen
//!   at the scale it had (`shell`).

/// The screen in its own pixels.
pub const SCREEN: (u32, u32) = (640, 480);

/// Logical points of the monitor kept free for the system's bars (menu bar, title bar, dock or task bar): across and
/// down.
pub const RESERVE: (f64, f64) = (32.0, 120.0);

/// The zoom's range with `--free-zoom`: map pixels per screen pixel.
pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 8.0;

/// The largest whole number of physical pixels per screen pixel whose 640 × 480 fits `(width, height)` physical
/// pixels; at least 1.
pub fn fit_scale(size: (u32, u32)) -> u32 {
    fit_scale_of(size, SCREEN)
}

/// [`fit_scale`] for a screen of another size (a widescreen game's).
pub fn fit_scale_of(size: (u32, u32), screen: (u32, u32)) -> u32 {
    (size.0 / screen.0.max(1)).min(size.1 / screen.1.max(1)).max(1)
}

/// The default scale in physical pixels per screen pixel for a monitor of `size` physical pixels at `factor` physical
/// pixels per logical point: the largest whose window fits the monitor less [`RESERVE`]; at least 1.
pub fn monitor_scale(size: (u32, u32), factor: f64) -> u32 {
    let factor = if factor > 0.0 { factor } else { 1.0 };
    let usable = |len: u32, reserve: f64| (len as f64 - reserve * factor).max(0.0) as u32;
    fit_scale((usable(size.0, RESERVE.0), usable(size.1, RESERVE.1)))
}

/// The window size in physical pixels for a scale.
pub fn window_size(scale: u32) -> (u32, u32) {
    (SCREEN.0 * scale, SCREEN.1 * scale)
}

/// A rectangle of physical window pixels: left, top, width, height.
pub type Area = (u32, u32, u32, u32);

/// Where the screen sits in a window: its scale and its top-left corner, in physical pixels. The scale is a whole
/// number unless the letterbox was made by [`Letterbox::fit_area`] in the fractional mode. The screen is 640 × 480
/// unless a game has a wider one (`widescreen`); then `screen` is its size in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Letterbox {
    pub scale: f32,
    pub offset: (u32, u32),
    pub screen: (u32, u32),
}

impl Default for Letterbox {
    fn default() -> Self {
        Self::fit((SCREEN.0, SCREEN.1))
    }
}

impl Letterbox {
    /// The 640 × 480 screen in a window of `size` physical pixels: the largest whole scale that fits, centred.
    pub fn fit(size: (u32, u32)) -> Self {
        Self::fit_screen(size, SCREEN)
    }

    /// A `screen` in a window of `size` physical pixels: the largest whole scale that fits, centred.
    pub fn fit_screen(size: (u32, u32), screen: (u32, u32)) -> Self {
        let scale = fit_scale_of(size, screen);
        let (w, h) = (screen.0 * scale, screen.1 * scale);
        Self {
            scale: scale as f32,
            offset: (size.0.saturating_sub(w) / 2, size.1.saturating_sub(h) / 2),
            screen,
        }
    }

    /// The 640 × 480 screen in the `safe` area of a window of `size` physical pixels, centred in it. `fractional` fits
    /// by a fraction of the area ("sharp bilinear", module doc); otherwise by a whole number, as [`Letterbox::fit`]
    /// does for the whole window. A fraction within a thousandth of a whole number is that whole number, so it draws
    /// crisply.
    pub fn fit_area(size: (u32, u32), safe: Area, fractional: bool) -> Self {
        Self::fit_area_screen(size, safe, fractional, SCREEN)
    }

    /// A `screen` in the `safe` area by a [`Fit`] mode: [`fractional_for`] decides whether the scale is whole.
    pub fn fit_with(size: (u32, u32), safe: Area, fit: Fit, screen: (u32, u32)) -> Self {
        let (_, _, sw, sh) = clip_area(safe, size);
        Self::fit_area_screen(size, safe, fractional_for(fit, (sw, sh), screen), screen)
    }

    /// [`Letterbox::fit_area`] for a `screen` of another size.
    pub fn fit_area_screen(size: (u32, u32), safe: Area, fractional: bool, screen: (u32, u32)) -> Self {
        let (sx, sy, sw, sh) = clip_area(safe, size);
        if !fractional {
            let inner = Self::fit_screen((sw, sh), screen);
            return Self {
                scale: inner.scale,
                offset: (sx + inner.offset.0, sy + inner.offset.1),
                screen,
            };
        }
        let raw = (sw as f32 / screen.0.max(1) as f32)
            .min(sh as f32 / screen.1.max(1) as f32)
            .max(MIN_FRACTION);
        let scale = if (raw - raw.round()).abs() < 0.001 && raw.round() >= 1.0 {
            raw.round()
        } else {
            raw
        };
        let (w, h) = (
            (screen.0 as f32 * scale).round() as u32,
            (screen.1 as f32 * scale).round() as u32,
        );
        Self {
            scale,
            offset: (sx + sw.saturating_sub(w) / 2, sy + sh.saturating_sub(h) / 2),
            screen,
        }
    }

    /// Whether the scale is a whole number (drawn with the nearest texel, no sharp-bilinear pass).
    pub fn is_whole(&self) -> bool {
        self.scale.fract() == 0.0
    }

    /// A rectangle of screen pixels (`x`, `y`, width, height) in physical window pixels.
    pub fn rect(&self, x: i32, y: i32, w: u32, h: u32) -> (u32, u32, u32, u32) {
        let s = self.scale;
        let px = |len: f32| (len * s).round() as u32;
        (
            self.offset.0 + px(x.max(0) as f32),
            self.offset.1 + px(y.max(0) as f32),
            px(w as f32),
            px(h as f32),
        )
    }

    /// The whole screen in physical window pixels.
    pub fn screen(&self) -> (u32, u32, u32, u32) {
        self.rect(0, 0, self.screen.0, self.screen.1)
    }

    /// A window point in logical points (`factor` physical pixels each) in screen pixels, not kept on the screen: a
    /// point on the black border is outside the screen.
    pub fn screen_of(&self, point: (f32, f32), factor: f32) -> (f32, f32) {
        let s = self.scale;
        let on = |p: f32, offset: u32| (p * factor - offset as f32) / s;
        (on(point.0, self.offset.0), on(point.1, self.offset.1))
    }

    /// A screen pixel as a window point in logical points: the inverse of [`Letterbox::screen_of`].
    pub fn window_of(&self, screen: (f32, f32), factor: f32) -> (f32, f32) {
        let f = factor.max(0.01);
        let on = |p: f32, offset: u32| (p * self.scale + offset as f32) / f;
        (on(screen.0, self.offset.0), on(screen.1, self.offset.1))
    }

    /// A window point in logical points (`factor` physical pixels each) in screen pixels, kept on the screen: a point
    /// on the black border goes to the nearest screen edge.
    pub fn screen_point(&self, point: (f32, f32), factor: f32) -> (f32, f32) {
        let (x, y) = self.screen_of(point, factor);
        (
            x.clamp(0.0, self.screen.0 as f32 - 0.001),
            y.clamp(0.0, self.screen.1 as f32 - 0.001),
        )
    }

    /// Where the 640 × 480 pictures of the menus, dialogs and flow screens sit on the screen: their top-left corner in
    /// screen pixels, `(0, 0)` unless the screen is wider (`widescreen`), when they are centred with black beside them.
    pub fn flow_origin(&self) -> (i32, i32) {
        (
            (self.screen.0 as i32 - SCREEN.0 as i32) / 2,
            (self.screen.1 as i32 - SCREEN.1 as i32) / 2,
        )
    }

    /// A window point in logical points in the pixels of the 640 × 480 pictures of the menus and dialogs
    /// ([`Letterbox::flow_origin`] is their corner), not kept on the screen.
    pub fn flow_of(&self, point: (f32, f32), factor: f32) -> (f32, f32) {
        let (x, y) = self.screen_of(point, factor);
        let o = self.flow_origin();
        (x - o.0 as f32, y - o.1 as f32)
    }

    /// A window point in logical points (`factor` physical pixels each) kept on the screen, still in logical points:
    /// a point on the black border or outside the window goes to the nearest screen edge. The locked pointer lives here.
    pub fn keep_on_screen(&self, point: (f32, f32), factor: f32) -> (f32, f32) {
        let s = self.scale;
        let on = |p: f32, offset: u32, len: u32| {
            let lo = offset as f32 / factor;
            p.clamp(lo, lo + (len as f32 - 0.001) * s / factor)
        };
        (
            on(point.0, self.offset.0, self.screen.0),
            on(point.1, self.offset.1, self.screen.1),
        )
    }

    /// The middle of the screen in logical points.
    pub fn centre(&self, factor: f32) -> (f32, f32) {
        let s = self.scale;
        (
            (self.offset.0 as f32 + self.screen.0 as f32 * s / 2.0) / factor,
            (self.offset.1 as f32 + self.screen.1 as f32 * s / 2.0) / factor,
        )
    }
}

/// How the picture is fitted to the window (the `scaling` extra; touch platforms always use [`Fit::Sharp`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Whole numbers when they fill the area well ([`AUTO_COVER`]), otherwise fractional.
    Auto,
    /// Always whole numbers.
    Whole,
    /// Always fractional, drawn sharp bilinear.
    Sharp,
}

/// What share of the limiting dimension (width or height, whichever binds) the whole-number scale must cover for
/// [`Fit::Auto`] to keep it.
pub const AUTO_COVER: f32 = 0.9;

/// Whether `fit` draws `screen` at a fractional scale in a safe area of `area` physical pixels (width, height).
/// For [`Fit::Auto`]: the whole-number scale ([`fit_scale_of`]) is kept when it is at least [`AUTO_COVER`] of the
/// fractional one (so it covers that share of the limiting dimension), and a poorer fit goes fractional.
pub fn fractional_for(fit: Fit, area: (u32, u32), screen: (u32, u32)) -> bool {
    match fit {
        Fit::Whole => false,
        Fit::Sharp => true,
        Fit::Auto => {
            let raw = (area.0 as f32 / screen.0.max(1) as f32).min(area.1 as f32 / screen.1.max(1) as f32);
            (fit_scale_of(area, screen) as f32) < AUTO_COVER * raw
        }
    }
}

/// The smallest fractional scale: a window much smaller than the screen still shows all of it.
pub const MIN_FRACTION: f32 = 0.25;

/// The whole-number scale the sharp-bilinear pass first draws at for a fractional `scale`: the next whole number up.
pub fn sharp_steps(scale: f32) -> u32 {
    (scale.ceil() as u32).max(1)
}

/// `area` kept inside a window of `size` physical pixels; an empty result becomes the whole window.
fn clip_area(area: Area, size: (u32, u32)) -> Area {
    let x = area.0.min(size.0);
    let y = area.1.min(size.1);
    let (w, h) = (area.2.min(size.0 - x), area.3.min(size.1 - y));
    if w == 0 || h == 0 {
        (0, 0, size.0, size.1)
    } else {
        (x, y, w, h)
    }
}

/// What decides the mouse lock.
#[derive(Clone, Copy, Debug, Default)]
pub struct LockInput {
    /// Not turned off with `--no-mouse-lock`.
    pub enabled: bool,
    /// A game is on screen, drawing its own pointer.
    pub in_game: bool,
    pub focused: bool,
    /// Cmd (the Windows key elsewhere) is held: the pointer is free while it is.
    pub held_free: bool,
}

/// Whether the pointer is locked to the window (module doc, "Mouse lock").
pub fn lock_wanted(i: LockInput) -> bool {
    i.enabled && i.in_game && i.focused && !i.held_free
}

/// The zoom after `steps` mouse-wheel steps (positive zooms in: fewer map pixels per screen pixel): powers of two
/// from [`MIN_ZOOM`] to [`MAX_ZOOM`], so map pixels stay whole blocks of screen pixels when zoomed in. A zoom that
/// isn't a power of two goes to the nearest one first.
pub fn zoom_step(zoom: f32, steps: i32) -> f32 {
    let k = zoom.max(f32::MIN_POSITIVE).log2().round() as i32 - steps;
    2f32.powi(k).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Mouse-wheel steps from a scroll amount in lines (`pixels` false) or in pixels (trackpads), with the remainder kept
/// in `acc` for the next frame. One line or [`WHEEL_PIXELS`] pixels make a step.
pub fn wheel_steps(acc: &mut f32, delta: f32, pixels: bool) -> i32 {
    *acc += if pixels { delta / WHEEL_PIXELS } else { delta };
    let steps = acc.trunc();
    *acc -= steps;
    steps as i32
}

/// Trackpad pixels per wheel step.
pub const WHEEL_PIXELS: f32 = 50.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_of_undoes_screen_of() {
        let lb = Letterbox::fit((2388, 1668));
        for p in [(0.0, 0.0), (176.0, 16.0), (639.0, 479.0)] {
            let w = lb.window_of(p, 2.0);
            let back = lb.screen_of(w, 2.0);
            assert!(
                (back.0 - p.0).abs() < 1e-3 && (back.1 - p.1).abs() < 1e-3,
                "{p:?} {back:?}"
            );
        }
    }

    #[test]
    fn monitor_scale_picks_the_largest_that_fits() {
        // A Retina laptop: 1512 × 982 points; 3 physical px per screen pixel is a 960 × 720-point window.
        assert_eq!(monitor_scale((3024, 1964), 2.0), 3);
        assert_eq!(window_size(3), (1920, 1440));
        // 1080p at 1: 1280 × 960 fits under the bars.
        assert_eq!(monitor_scale((1920, 1080), 1.0), 2);
        assert_eq!(monitor_scale((2560, 1440), 1.0), 2);
        assert_eq!(monitor_scale((3840, 2160), 1.0), 4);
        assert_eq!(monitor_scale((3840, 2160), 2.0), 4);
        // Small screens get 1, even when 640 × 480 doesn't quite fit.
        assert_eq!(monitor_scale((1024, 768), 1.0), 1);
        assert_eq!(monitor_scale((800, 600), 1.0), 1);
        assert_eq!(monitor_scale((640, 480), 1.0), 1);
        assert_eq!(monitor_scale((0, 0), 0.0), 1);
    }

    #[test]
    fn fit_is_whole_and_never_smaller_than_the_screen() {
        assert_eq!(fit_scale((1920, 1440)), 3);
        assert_eq!(fit_scale((1919, 1440)), 2);
        assert_eq!(fit_scale((3024, 1964)), 4);
        assert_eq!(fit_scale((100, 100)), 1);
    }

    #[test]
    fn letterbox_centres_the_screen() {
        // Exactly the scaled screen: no border.
        assert_eq!(
            Letterbox::fit((1920, 1440)),
            Letterbox {
                scale: 3.0,
                offset: (0, 0),
                screen: SCREEN,
            }
        );
        // Full screen on the Retina laptop: scale 4 (2560 × 1920), 232 px left and right, 22 above and below.
        let lb = Letterbox::fit((3024, 1964));
        assert_eq!(
            lb,
            Letterbox {
                scale: 4.0,
                offset: (232, 22),
                screen: SCREEN,
            }
        );
        assert_eq!(lb.screen(), (232, 22, 2560, 1920));
        // A window too wide keeps its scale and gets side bars.
        assert_eq!(Letterbox::fit((2000, 960)).offset, (360, 0));
        // The map view (screen 176, 16, 448 × 448) in the window.
        assert_eq!(lb.rect(176, 16, 448, 448), (232 + 704, 22 + 64, 1792, 1792));
    }

    #[test]
    fn pointer_goes_through_the_offset() {
        let lb = Letterbox::fit((3024, 1964));
        // Logical points at factor 2: physical (232, 22) is screen (0, 0).
        assert_eq!(lb.screen_point((116.0, 11.0), 2.0), (0.0, 0.0));
        // Physical (232 + 4 × 176, 22 + 4 × 16) is the map view's top-left pixel.
        assert_eq!(lb.screen_point((468.0, 43.0), 2.0), (176.0, 16.0));
        // Half a screen pixel in.
        assert_eq!(lb.screen_point((469.0, 44.0), 2.0), (176.5, 16.5));
        // On the black border: the nearest edge.
        let (x, y) = lb.screen_point((10.0, 10.0), 2.0);
        assert_eq!((x, y), (0.0, 0.0));
        let (x, y) = lb.screen_point((1500.0, 980.0), 2.0);
        assert_eq!((x.floor(), y.floor()), (639.0, 479.0));
        // No border at factor 1.
        let lb = Letterbox::fit((1280, 960));
        assert_eq!(lb.screen_point((300.0, 200.0), 1.0), (150.0, 100.0));
    }

    #[test]
    fn locked_pointer_stays_on_the_screen() {
        // Full screen on the Retina laptop at factor 2: the screen is logical 116..1396 across, 11..971 down.
        let lb = Letterbox::fit((3024, 1964));
        assert_eq!(lb.keep_on_screen((500.0, 300.0), 2.0), (500.0, 300.0));
        let (x, y) = lb.keep_on_screen((-40.0, 2000.0), 2.0);
        assert_eq!(x, 116.0);
        assert!(y < 971.0 && y > 970.99);
        // Kept points are screen pixels 0 and 479, never past the edge.
        assert_eq!(lb.screen_point((x, y), 2.0).1.floor(), 479.0);
        let (x, _) = lb.keep_on_screen((5000.0, 0.0), 2.0);
        assert_eq!(lb.screen_point((x, 0.0), 2.0).0.floor(), 639.0);
        assert_eq!(lb.centre(2.0), (756.0, 491.0));
        assert_eq!(lb.screen_point(lb.centre(2.0), 2.0), (320.0, 240.0));
    }

    #[test]
    fn a_fractional_fit_fills_the_safe_area() {
        // A phone in landscape, 2532 × 1170 pixels: the height limits the scale to 2.4375, the screen is 1560 × 1170.
        let phone = (2532, 1170);
        let lb = Letterbox::fit_area(phone, (0, 0, 2532, 1170), true);
        assert_eq!(lb.scale, 1170.0 / 480.0);
        assert!(!lb.is_whole());
        assert_eq!(lb.screen(), (486, 0, 1560, 1170));
        // The integer mode in the same window keeps whole numbers: 2 × 640 = 1280 across, 960 down.
        let whole = Letterbox::fit_area(phone, (0, 0, 2532, 1170), false);
        assert_eq!(whole, Letterbox::fit(phone));
        assert_eq!(whole.scale, 2.0);
        // The safe area (a notch on the left, a home bar below) shrinks and shifts it.
        let safe = Letterbox::fit_area(phone, (132, 0, 2268, 1100), true);
        assert_eq!(safe.scale, 1100.0 / 480.0);
        let (x, y, w, h) = safe.screen();
        assert_eq!((y, h), (0, 1100));
        assert!(x >= 132 && x + w <= 132 + 2268, "inside the safe area: {x} {w}");
        assert!((x - 132).abs_diff(132 + 2268 - (x + w)) <= 1, "centred in it");
        // A whole-number fit in the fractional mode is crisp: no sharp-bilinear pass needed.
        let exact = Letterbox::fit_area((1920, 1440), (0, 0, 1920, 1440), true);
        assert_eq!((exact.scale, exact.is_whole()), (3.0, true));
        assert_eq!(sharp_steps(exact.scale), 3);
        // A fractional scale first draws at the next whole number up.
        assert_eq!(sharp_steps(2.4375), 3);
        assert_eq!(sharp_steps(0.5), 1);
    }

    #[test]
    fn a_fractional_fit_can_be_smaller_than_the_screen_and_survives_a_bad_area() {
        let lb = Letterbox::fit_area((320, 240), (0, 0, 320, 240), true);
        assert_eq!(lb.scale, 0.5);
        assert_eq!(lb.screen(), (0, 0, 320, 240));
        // Never smaller than the floor, never a divide by nothing.
        assert_eq!(Letterbox::fit_area((10, 10), (0, 0, 10, 10), true).scale, MIN_FRACTION);
        // An empty or overhanging safe area is the whole window.
        assert_eq!(
            Letterbox::fit_area((1280, 960), (0, 0, 0, 0), true),
            Letterbox::fit_area((1280, 960), (0, 0, 1280, 960), true)
        );
        assert_eq!(
            Letterbox::fit_area((1280, 960), (1000, 0, 5000, 960), false).offset.1,
            240,
            "the overhang is cut to the window (280 wide), the screen centred in what is left"
        );
    }

    #[test]
    fn auto_keeps_a_good_whole_fit_and_fills_a_poor_one() {
        let auto = |size: (u32, u32), screen| Letterbox::fit_with(size, (0, 0, size.0, size.1), Fit::Auto, screen);
        // The notched laptop's full screen: 4 x 480 > 1900, so whole is 3 (76 % of the height): fill instead.
        let lb = auto((3024, 1900), SCREEN);
        assert!(!lb.is_whole());
        assert!((lb.scale - 1900.0 / 480.0).abs() < 1e-6, "{}", lb.scale);
        assert_eq!(lb.screen().3, 1900, "the whole height");
        // A window sized to a whole scale stays whole, and is the integer fit exactly.
        for size in [(2560, 1920), (1280, 960), (1920, 1440), (640, 480)] {
            assert_eq!(auto(size, SCREEN), Letterbox::fit(size), "{size:?}");
        }
        // A resized window: 2 of 2.208 (91 %) keeps whole, 2 of 2.225 (90 %, just under) fills; 1 of 1.875 fills.
        assert!(auto((1920, 1060), SCREEN).is_whole());
        assert!(!auto((1920, 1068), SCREEN).is_whole());
        assert!(!auto((1500, 900), SCREEN).is_whole());
        // Smaller than the screen: whole 1 is kept when it nearly fits, as `integer` does; a tiny window shrinks.
        assert_eq!(auto((620, 470), SCREEN).scale, Letterbox::fit((620, 470)).scale);
        // `integer` and `sharp` mean what they say.
        let area = (0, 0, 3024, 1900);
        assert_eq!(Letterbox::fit_with((3024, 1900), area, Fit::Whole, SCREEN).scale, 3.0);
        let sharp = Letterbox::fit_with((2560, 1920), (0, 0, 2560, 1920), Fit::Sharp, SCREEN);
        assert_eq!(sharp.scale, 4.0);
        let sharp = Letterbox::fit_with((3024, 1900), area, Fit::Sharp, SCREEN);
        assert_eq!(sharp.screen().3, 1900);
        // The safe area, not the window, is what counts.
        let lb = Letterbox::fit_with((3024, 1964), (0, 64, 3024, 1900), Fit::Auto, SCREEN);
        assert_eq!(lb.screen().3, 1900);
        assert_eq!(lb.offset.1, 64);
    }

    #[test]
    fn auto_fills_the_notched_full_screen_with_a_widescreen_game_too() {
        use crate::extras::Widescreen;
        use crate::widescreen::width;
        let area = (3024, 1900);
        let lb_of = |w: u32| Letterbox::fit_with(area, (0, 0, area.0, area.1), Fit::Auto, (w, 480));
        // `auto` widescreen takes the window's shape: 768 x 480 (16:10), width-limited at 3.9375 (3024 / 768).
        let w = width(Widescreen::Auto, Some(area));
        assert_eq!(w, 768);
        let lb = lb_of(w);
        assert!((lb.scale - 3024.0 / 768.0).abs() < 1e-6);
        assert_eq!(
            lb.screen(),
            (0, 5, 3024, 1890),
            "the whole width, 10 pixels of height spare"
        );
        // 16:9 (864 wide) is width-limited at 3.5 (whole 3 would be 86 %): it fills the width, 1680 high.
        let lb = lb_of(864);
        assert!((lb.scale - 3024.0 / 864.0).abs() < 1e-6);
        assert_eq!(lb.screen().2, 3024);
        // A 16:9 screen in a window of exactly 2 x 864 x 2 x 480 stays whole.
        let lb = Letterbox::fit_with((3456, 1920), (0, 0, 3456, 1920), Fit::Auto, (864, 480));
        assert_eq!((lb.scale, lb.is_whole()), (4.0, true));
    }

    #[test]
    fn the_pointer_goes_through_a_fractional_scale() {
        let lb = Letterbox::fit_area((1560, 1170), (0, 0, 1560, 1170), true);
        assert_eq!(lb.scale, 2.4375);
        assert_eq!(lb.screen_point((0.0, 0.0), 1.0), (0.0, 0.0));
        let (x, y) = lb.screen_point((780.0, 585.0), 1.0);
        assert!((x - 320.0).abs() < 1e-3 && (y - 240.0).abs() < 1e-3, "{x} {y}");
        // A point on the other side of a point-sized window (factor 2) is the same screen pixel.
        let (x, _) = lb.screen_point((390.0, 292.5), 2.0);
        assert!((x - 320.0).abs() < 1e-3);
        // Rectangles are rounded to whole physical pixels.
        assert_eq!(lb.rect(176, 16, 448, 448), (429, 39, 1092, 1092));
    }

    /// The widescreen game's 864 × 480 screen.
    const WIDE: (u32, u32) = (864, 480);

    #[test]
    fn a_wide_screen_is_fitted_and_centred_by_its_own_size() {
        // 864 × 480 at scale 2 is 1728 × 960: it needs a window that wide, and a 4:3 window of the 640 × 480 screen's
        // scale 2 (1280 × 960) can only show it at scale 1.
        assert_eq!(fit_scale_of((1728, 960), WIDE), 2);
        assert_eq!(fit_scale_of((1280, 960), WIDE), 1);
        assert_eq!(fit_scale_of((100, 100), WIDE), 1, "never smaller than 1");
        assert_eq!(fit_scale_of((1280, 960), SCREEN), fit_scale((1280, 960)));
        let lb = Letterbox::fit_screen((1920, 1080), WIDE);
        assert_eq!((lb.scale, lb.offset, lb.screen), (2.0, (96, 60), WIDE));
        assert_eq!(lb.screen(), (96, 60, 1728, 960));
        // The 640 × 480 fit is the same call with the original size.
        assert_eq!(
            Letterbox::fit((1920, 1440)),
            Letterbox::fit_screen((1920, 1440), SCREEN)
        );
        // The fractional fit fills the safe area by the wide screen's shape: 1920 / 864 = 2.22, 1080 / 480 = 2.25.
        let sharp = Letterbox::fit_area_screen((1920, 1080), (0, 0, 1920, 1080), true, WIDE);
        assert!((sharp.scale - 1920.0 / 864.0).abs() < 1e-6);
        assert_eq!(sharp.screen(), (0, 6, 1920, 1067));
        // The whole-number fit inside a safe area.
        let whole = Letterbox::fit_area_screen((1920, 1080), (0, 0, 1920, 1080), false, WIDE);
        assert_eq!(whole, lb);
    }

    #[test]
    fn the_pointer_reaches_the_extra_width_and_stops_at_the_wide_edge() {
        // A 1728 × 960 window at factor 1: scale 2, no border; screen pixel x is window point 2x.
        let lb = Letterbox::fit_screen((1728, 960), WIDE);
        assert_eq!(
            lb.screen_point((1000.0, 400.0), 1.0),
            (500.0, 200.0),
            "past the old 640 edge"
        );
        let (x, _) = lb.screen_point((1727.0, 0.0), 1.0);
        assert_eq!(x.floor(), 863.0);
        let (x, _) = lb.screen_point((5000.0, 0.0), 1.0);
        assert_eq!(x.floor(), 863.0, "the nearest edge of the wide screen, not of 640");
        let (kx, _) = lb.keep_on_screen((5000.0, 0.0), 1.0);
        assert_eq!(lb.screen_point((kx, 0.0), 1.0).0.floor(), 863.0);
        assert_eq!(lb.centre(1.0), (864.0, 480.0));
        assert_eq!(lb.screen_point(lb.centre(1.0), 1.0), (432.0, 240.0));
    }

    #[test]
    fn menus_keep_their_640_pixels_in_the_middle_of_a_wide_screen() {
        let lb = Letterbox::fit_screen((1728, 960), WIDE);
        assert_eq!(lb.flow_origin(), (112, 0));
        // The wide screen's middle is the menu picture's middle; its left edge is 112 pixels in.
        assert_eq!(lb.flow_of(lb.centre(1.0), 1.0), (320.0, 240.0));
        assert_eq!(lb.flow_of((224.0, 0.0), 1.0), (0.0, 0.0));
        let (x, _) = lb.flow_of((0.0, 0.0), 1.0);
        assert_eq!(x, -112.0, "the black beside the menu is outside its picture");
        // On the original screen the two are the same.
        let lb = Letterbox::fit((1280, 960));
        assert_eq!(lb.flow_origin(), (0, 0));
        assert_eq!(lb.flow_of((300.0, 200.0), 1.0), lb.screen_of((300.0, 200.0), 1.0));
    }

    #[test]
    fn lock_only_in_a_focused_game() {
        let on = LockInput {
            enabled: true,
            in_game: true,
            focused: true,
            held_free: false,
        };
        assert!(lock_wanted(on));
        assert!(!lock_wanted(LockInput { enabled: false, ..on }));
        assert!(!lock_wanted(LockInput { in_game: false, ..on }));
        assert!(!lock_wanted(LockInput { focused: false, ..on }));
        assert!(!lock_wanted(LockInput { held_free: true, ..on }));
    }

    #[test]
    fn wheel_zoom_steps_in_powers_of_two() {
        assert_eq!(zoom_step(1.0, 1), 0.5);
        assert_eq!(zoom_step(1.0, -1), 2.0);
        assert_eq!(zoom_step(0.25, 1), MIN_ZOOM);
        assert_eq!(zoom_step(8.0, -3), MAX_ZOOM);
        // An odd start goes to the nearest power of two.
        assert_eq!(zoom_step(1.3, 0), 1.0);
        assert_eq!(zoom_step(3.0, 1), 2.0);
        let mut acc = 0.0;
        assert_eq!(wheel_steps(&mut acc, 1.0, false), 1);
        assert_eq!(wheel_steps(&mut acc, -2.0, false), -2);
        assert_eq!(wheel_steps(&mut acc, 30.0, true), 0);
        assert_eq!(wheel_steps(&mut acc, 30.0, true), 1);
        assert!((acc - 0.2).abs() < 1e-6);
    }
}
