//! Widescreen (option `widescreen`, `docs/design/extras.md`; design in `docs/design/client-core.md`, "Widescreen"): the
//! game screen is wider than 640 pixels and the extra width is map. Our own extension; the original has one fixed
//! 640 × 480 screen.
//!
//! **The width.** The screen stays 480 lines high, so the interface's art keeps its size and the map view keeps its
//! height; only the width changes. Every width is 640 plus whole tiles (32 pixels), so the map view always shows whole
//! tiles across, as the original's 14-tile view does and as the view position (a whole tile) assumes:
//!
//! | Setting | Screen | Map view | Shape |
//! |---|---|---|---|
//! | `off` | 640 × 480 | 14 tiles | 4:3, the original |
//! | `16:10` | 768 × 480 | 18 tiles | exactly 16:10 |
//! | `16:9` | 864 × 480 | 21 tiles | 1.8, the tile multiple nearest 16:9 (853.3 pixels) |
//! | `auto` | 640 to 864 | 14 to 21 tiles | the tile multiple nearest the window's shape, never wider than `16:9` |
//!
//! **What stays.** The interface (the left panel with the minimap and command buttons, the resource bar's items, the
//! status line) keeps its size and its place against the left edge ([`crate::hud::Layout`] with a canvas of the new
//! width: the top and bottom bars repeat along the longer view, the resource items and the status text start where
//! they did, and the clock overlay and the message list keep their distance from the view's corners). The view
//! shows more map to the right and nothing is zoomed out, so the extra width is the only gain: it shows nothing
//! vertically that the 640 × 480 screen doesn't. That is why the option is **C+L**: it gives a small edge, so in a game
//! with other players the lobby sets it for everyone.
//!
//! **What doesn't widen.** The menus, briefings, videos, the score screen and the game's own dialogs (game menu, save
//! and load, the outcome) are 640 × 480 pictures; in a wider frame they sit in the middle with black on both sides
//! ([`crate::display::Letterbox::flow_of`] takes the pointer back to their own pixels). Outside a game the screen is
//! 640 × 480 and the window keeps whatever width a game gave it.
//!
//! **Touch.** On a touch device the pillarbox beside a 4:3 picture holds the touch buttons (`chrome`), so the option
//! picks between them and the wider map: a game screen wider than 640 hides the buttons (`crate::touch::step`), and
//! `off` (or `auto` in a 4:3 window) keeps them. Without them a player loses the control groups, the Shift, Ctrl and
//! Alt latches, Pause and the keyboard button; the game menu is still the interface's Menu button and the keyboard
//! opens by itself for a text box (`crate::touch`, "keyboard").

use crate::display::SCREEN;
use crate::extras::Widescreen;

/// A tile in pixels, the step between the widths.
pub const TILE: u32 = 32;
/// The widest screen, 27 tiles across: the multiple of a tile nearest 16:9.
pub const MAX_WIDTH: u32 = SCREEN.0 + 7 * TILE;
/// The screen of `16:10`: exactly 16:10.
pub const WIDTH_16_10: u32 = SCREEN.0 + 4 * TILE;

/// The game screen's width for a setting.
///
/// `window` is the window's usable size in physical pixels (what `auto` matches; `None` without a window counts as
/// 4:3).
pub fn width(mode: Widescreen, window: Option<(u32, u32)>) -> u32 {
    match mode {
        Widescreen::Off => SCREEN.0,
        Widescreen::Ratio16x10 => WIDTH_16_10,
        Widescreen::Ratio16x9 => MAX_WIDTH,
        Widescreen::Auto => window.map_or(SCREEN.0, |(w, h)| width_for_shape(w, h)),
    }
}

/// The width that matches a window `w` × `h` pixels: 480 lines times the window's shape, to the nearest whole tile,
/// from 640 (a window narrower than 4:3 gets the original screen) to [`MAX_WIDTH`] (an ultrawide window gets 16:9
/// and black bars).
pub fn width_for_shape(w: u32, h: u32) -> u32 {
    if h == 0 {
        return SCREEN.0;
    }
    // 480 w / h pixels, in 32nds of a tile so the rounding is to the nearest tile.
    let wanted = (SCREEN.1 as u64 * w as u64 * 2 + h as u64) / (2 * h as u64);
    let extra_tiles = (wanted.saturating_sub(SCREEN.0 as u64) + TILE as u64 / 2) / TILE as u64;
    SCREEN.0 + TILE * extra_tiles.min(((MAX_WIDTH - SCREEN.0) / TILE) as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_widths_are_whole_tiles_past_640() {
        assert_eq!(width(Widescreen::Off, None), 640);
        assert_eq!(width(Widescreen::Ratio16x10, None), 768);
        assert_eq!(width(Widescreen::Ratio16x9, None), 864);
        for w in [WIDTH_16_10, MAX_WIDTH] {
            assert_eq!((w - 640) % TILE, 0);
            // The view (less the 176-pixel panel and the 16-pixel bar) is whole tiles too.
            assert_eq!((w - 192) % TILE, 0);
        }
        // 16:10 is exact; the 16:9 width is the nearest tile multiple to 853.3.
        assert_eq!(WIDTH_16_10 * 10, 480 * 16);
        assert!((MAX_WIDTH as i32 - 853).abs() < 16);
    }

    #[test]
    fn auto_matches_the_window_up_to_16_9() {
        let auto = |w, h| width(Widescreen::Auto, Some((w, h)));
        assert_eq!(auto(1280, 960), 640, "4:3 is the original screen");
        assert_eq!(auto(1280, 800), 768, "16:10");
        assert_eq!(auto(1920, 1200), 768);
        assert_eq!(auto(1920, 1080), 864, "16:9");
        assert_eq!(auto(2560, 1440), 864);
        assert_eq!(auto(3440, 1440), 864, "ultrawide is clamped to 16:9");
        assert_eq!(auto(5120, 1440), 864);
        assert_eq!(auto(800, 1200), 640, "a window narrower than 4:3 gets the original");
        assert_eq!(
            auto(1700, 1000),
            832,
            "1.7 is 816 pixels: the nearest tile multiple is 832"
        );
        assert_eq!(auto(1600, 1000), 768, "1.6 is 768 pixels exactly");
        assert_eq!(auto(0, 0), 640, "no shape to match");
        assert_eq!(width(Widescreen::Auto, None), 640, "no window");
    }

    #[test]
    fn auto_only_grows_with_the_window() {
        let mut last = 0;
        for w in (800..4000).step_by(8) {
            let x = width_for_shape(w, 1000);
            assert!(x >= last && (640..=864).contains(&x));
            last = x;
        }
    }
}
