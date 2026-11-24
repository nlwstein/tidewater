//! The rules of the optional on-screen information (`extras`, `docs/design/extras.md`): health bars over units and
//! the game clock with the player's actions per minute. Only rules and text here; `view` draws the bars and `hud` the
//! clock. Nothing of this runs with the options at their defaults.
//!
//! Our own design, not the original's: the original shows a unit's hit points only in the information panel.
//!
//! - **Health bars.** A bar is [`BAR_HEIGHT`] rows high and as wide as the unit's selection box, [`BAR_GAP`] rows above
//!   the box. Its colour is the information panel's health colour ([`crate::info::health_bar`]); the empty part is
//!   [`EMPTY`], the information panel's empty colour. A unit with any hit points shows at least one filled pixel.
//! - **Progress bars** (option `progress-income`) sit [`BAR_GAP`] rows under the selection box of the player's own
//!   buildings that are under construction or training, researching or upgrading, [`PROGRESS_HEIGHT`] rows high and as
//!   wide as the box, in a one-pixel black frame ([`PROGRESS_FRAME`]). The fill is the progress the information panel's bar shows for that building
//!   ([`crate::info::Subject::progress`]), so nothing is shown that selecting the building would not.
//! - **The clock** counts the game's own clock (`savegame::GameClock`: minutes and seconds, with hours once there are
//!   any). **Actions per minute** are the commands the player gave (selecting included) per minute of that clock, shown
//!   from the first full second and as a whole number.

use crate::extras::{ClockMode, HealthBars};

/// Rows of a health bar.
pub const BAR_HEIGHT: i32 = 3;
/// Rows between a bar and the unit's selection box.
pub const BAR_GAP: i32 = 2;
/// The palette colour of the empty part of a bar (info-panel spec, "Health bar colours").
pub const EMPTY: u8 = 99;

/// Rows of a production progress bar: thinner than a health bar, so the two read as different things.
pub const PROGRESS_HEIGHT: i32 = 2;
/// The palette colour of the filled part of a progress bar (the minimap's white view rectangle).
pub const PROGRESS: u8 = 246;

/// The palette colour of a progress bar's one-pixel frame: black, so the bar shows on snow and sand.
pub const PROGRESS_FRAME: u8 = 0;

/// How many of `width` pixels are filled for a shown progress `p` (0 to 255, [`crate::info::Subject::progress`], the
/// value the information panel's bar shows). Work that has begun shows at least one pixel; 255 fills the bar.
pub fn progress_fill(p: u8, width: i32) -> i32 {
    if width <= 0 {
        return 0;
    }
    (width as i64 * p as i64 / 255).clamp(1, width as i64) as i32
}

/// The progress a unit's bar shows, if it shows one: only the local player's own buildings (`owner` is `me`, a player
/// 0 to 7; an observer, an ally, an enemy or a neutral building never), alive, and at work, i.e. `progress` is
/// `Some` (`crate::info::Subject::progress` of a busy building).
pub fn progress_shown(
    on: bool,
    gone: bool,
    building: bool,
    owner: u8,
    me: u8,
    progress: impl FnOnce() -> Option<u8>,
) -> Option<u8> {
    if on && !gone && building && me < 8 && owner == me {
        progress()
    } else {
        None
    }
}

/// Whether a unit shows a health bar under `mode`.
pub fn bar_shown(mode: HealthBars, selected: bool, hovered: bool) -> bool {
    match mode {
        HealthBars::Off => false,
        HealthBars::Hovered => hovered,
        HealthBars::Selected => selected,
        HealthBars::Always => true,
    }
}

/// How many of `width` pixels are filled for `hp` of `max` hit points, and the colour of the filled part. `None` for a
/// unit with no hit points to show (nothing, or a type without any).
pub fn bar_fill(hp: u16, max: u16, width: i32) -> Option<(i32, u8)> {
    if hp == 0 || max == 0 || width <= 0 {
        return None;
    }
    let fill = (width as i64 * hp as i64 / max as i64).clamp(1, width as i64) as i32;
    Some((fill, crate::info::health_bar(hp, max).1))
}

/// Game time as `M:SS`, or `H:MM:SS` from the first hour.
pub fn clock_text(ms: u32) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Actions per minute after `actions` commands in `ms` of game time. Nothing is shown before the first second.
pub fn apm(actions: u32, ms: u32) -> Option<u32> {
    (ms >= 1000).then(|| (actions as u64 * 60_000 / ms as u64) as u32)
}

/// The overlay's line, if the mode shows one: `12:34` or `12:34  APM 87`.
pub fn overlay_text(mode: ClockMode, ms: u32, actions: u32) -> Option<String> {
    match mode {
        ClockMode::Off => None,
        ClockMode::Clock => Some(clock_text(ms)),
        ClockMode::ClockApm => Some(match apm(actions, ms) {
            Some(a) => format!("{}  APM {a}", clock_text(ms)),
            None => format!("{}  APM -", clock_text(ms)),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_choose_which_units_show_a_bar() {
        // (selected, hovered)
        let cases = [(false, false), (true, false), (false, true), (true, true)];
        let shown = |m| cases.map(|(s, h)| bar_shown(m, s, h));
        assert_eq!(shown(HealthBars::Off), [false; 4]);
        assert_eq!(shown(HealthBars::Hovered), [false, false, true, true]);
        assert_eq!(shown(HealthBars::Selected), [false, true, false, true]);
        assert_eq!(shown(HealthBars::Always), [true; 4]);
    }

    #[test]
    fn a_bar_fills_by_hit_points_and_takes_the_panels_colours() {
        assert_eq!(bar_fill(60, 60, 31), Some((31, 149)));
        assert_eq!(bar_fill(30, 60, 31), Some((15, 251)));
        assert_eq!(bar_fill(10, 60, 31), Some((5, 249)));
        // A sliver still shows; a dead or hit-point-less unit shows nothing.
        assert_eq!(bar_fill(1, 400, 31), Some((1, 249)));
        assert_eq!(bar_fill(0, 60, 31), None);
        assert_eq!(bar_fill(5, 0, 31), None);
        // More than the maximum (a boosted unit) stays inside the bar.
        assert_eq!(bar_fill(90, 60, 31).map(|f| f.0), Some(31));
    }

    #[test]
    fn a_progress_bar_fills_with_the_shown_progress() {
        assert_eq!(progress_fill(0, 48), 1, "work that has begun shows a sliver");
        assert_eq!(progress_fill(127, 48), 23);
        assert_eq!(progress_fill(255, 48), 48);
        assert_eq!(progress_fill(255, 1), 1);
        assert_eq!(progress_fill(100, 0), 0);
    }

    #[test]
    fn only_your_own_buildings_at_work_show_progress() {
        let at_work = || Some(100);
        assert_eq!(progress_shown(true, false, true, 2, 2, at_work), Some(100));
        assert_eq!(progress_shown(false, false, true, 2, 2, at_work), None, "off");
        assert_eq!(
            progress_shown(true, true, true, 2, 2, at_work),
            None,
            "a building being destroyed"
        );
        assert_eq!(
            progress_shown(true, false, false, 2, 2, at_work),
            None,
            "a unit is not a building"
        );
        assert_eq!(
            progress_shown(true, false, true, 3, 2, at_work),
            None,
            "an enemy's or ally's"
        );
        assert_eq!(progress_shown(true, false, true, 15, 2, at_work), None, "a neutral's");
        assert_eq!(
            progress_shown(true, false, true, 9, 9, at_work),
            None,
            "an observer owns nothing"
        );
        assert_eq!(progress_shown(true, false, true, 2, 2, || None), None, "idle");
        // An enemy's progress is never even looked up.
        progress_shown(true, false, true, 3, 2, || panic!("read an enemy building"));
    }

    #[test]
    fn the_clock_reads_minutes_and_seconds_then_hours() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(59_999), "0:59");
        assert_eq!(clock_text(61_000), "1:01");
        assert_eq!(clock_text(3_599_000), "59:59");
        assert_eq!(clock_text(3_600_000), "1:00:00");
        assert_eq!(clock_text(3_723_000), "1:02:03");
    }

    #[test]
    fn actions_per_minute_wait_for_a_full_second() {
        assert_eq!(apm(5, 999), None);
        assert_eq!(apm(5, 1000), Some(300));
        assert_eq!(apm(30, 60_000), Some(30));
        assert_eq!(apm(0, 90_000), Some(0));
        assert_eq!(apm(100, 150_000), Some(40));
    }

    #[test]
    fn the_overlay_line_follows_the_mode() {
        assert_eq!(overlay_text(ClockMode::Off, 5000, 3), None);
        assert_eq!(overlay_text(ClockMode::Clock, 65_000, 3).as_deref(), Some("1:05"));
        assert_eq!(
            overlay_text(ClockMode::ClockApm, 65_000, 13).as_deref(),
            Some("1:05  APM 12")
        );
        assert_eq!(
            overlay_text(ClockMode::ClockApm, 500, 0).as_deref(),
            Some("0:00  APM -")
        );
    }
}
