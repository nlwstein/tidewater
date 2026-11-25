//! The replay viewer's controls (`tidewater --replay`, README "Replays"; `docs/design/multiplayer-determinism.md`, "The replay viewer"): a timeline bar over the bottom of the map view
//! and the keys that move about the recording. Our own design, not the original's, which has no replays. Only a
//! replay session has a timeline ([`crate::session::Session::seek_info`]); a live game never shows or reads any of this.
//!
//! **The bar.** [`BAR_H`] rows high, [`MARGIN`] pixels in from the map view's left, right and bottom edges: the time now
//! and the length (`3:21 / 9:58`), the track with its knob, and a button with the vision (`View: Player 2`). A press on
//! the track jumps there and dragging moves the jump as the pointer goes; a press on the button takes the next vision.
//! A tap does the same (touch becomes the pointer, `gesture`).
//!
//! **The keys.** Chosen to be free in a game: the arrow, Home and End keys scroll the map, but not with Alt held, and
//! the original binds no Alt chord to them. Alt+Left and Alt+Right jump [`SHORT_JUMP_MS`] back and ahead (Shift: [`LONG_JUMP_MS`]),
//! Alt+Home and Alt+End go to the start and the end, and Tab takes the next vision. Pause (Ctrl+P) and speed (`+`, `-`)
//! are the viewer's own already.

use tidewater_replay::seek::Vision;

use crate::hud::Rect;
use crate::input::Key;

/// Rows of the bar.
pub const BAR_H: i32 = 14;
/// Pixels between the bar and the map view's edges.
pub const MARGIN: i32 = 4;
/// Pixels the time text takes at the bar's left, the vision button at its right, and the gap round the track.
const TIME_W: i32 = 92;
const BUTTON_W: i32 = 124;
const GAP: i32 = 6;
/// The jump of Alt+Left and Alt+Right, and with Shift.
pub const SHORT_JUMP_MS: u64 = 10_000;
pub const LONG_JUMP_MS: u64 = 60_000;
/// Cycles played out in one frame of a jump: a jump of a whole game (30,000 cycles) takes about two seconds, with the
/// timeline moving, and a frame stays short enough for the program to answer.
pub const CYCLES_PER_FRAME: u32 = 250;

/// The viewer's state between frames, on the game's `Sim`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ui {
    /// The left button went down on the track and is still down: the pointer drags the jump.
    pub drag: bool,
    /// A jump or a change of vision was asked for: when it has arrived the client's own pictures are made again.
    pub refresh: bool,
    /// The vision went to another player: the view goes to that player's start and the selection is dropped.
    pub recentre: bool,
}

/// What the bar shows, from the session and the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    /// The cycle the knob is at: the world's, or the jump's target while one is under way (so the knob does not lag
    /// behind the pointer that drags it).
    pub tick: u32,
    pub total: u32,
    pub now_ms: u64,
    pub total_ms: u64,
    /// `View: Player 2` or `View: Everyone`.
    pub vision: String,
    /// A jump is being played out.
    pub seeking: bool,
}

/// The bar over the map view `view`.
pub fn bar_rect(view: Rect) -> Rect {
    Rect::new(
        view.x + MARGIN,
        view.y + view.h as i32 - MARGIN - BAR_H,
        (view.w as i32 - 2 * MARGIN).max(0) as u32,
        BAR_H as u32,
    )
}

/// The track inside the bar: between the time and the vision button.
pub fn track_rect(bar: Rect) -> Rect {
    let x = bar.x + TIME_W + GAP;
    let w = (bar.w as i32 - TIME_W - BUTTON_W - 3 * GAP).max(0);
    Rect::new(x, bar.y, w as u32, bar.h)
}

/// The vision button inside the bar.
pub fn button_rect(bar: Rect) -> Rect {
    Rect::new(bar.x + bar.w as i32 - BUTTON_W, bar.y, BUTTON_W as u32, bar.h)
}

/// The time text's area at the bar's left.
pub fn time_rect(bar: Rect) -> Rect {
    Rect::new(bar.x, bar.y, TIME_W as u32, bar.h)
}

/// The cycle a press at pixel `x` of the track means: the track's left end is cycle 0 and its right end the last.
/// Outside the track it is the nearer end.
pub fn tick_at(track: Rect, x: f32, total: u32) -> u32 {
    if track.w == 0 {
        return 0;
    }
    let f = ((x - track.x as f32) / track.w as f32).clamp(0.0, 1.0);
    (f64::from(f) * f64::from(total)).round() as u32
}

/// The pixel of the knob at cycle `tick`, inside the track.
pub fn knob_x(track: Rect, tick: u32, total: u32) -> i32 {
    if total == 0 {
        return track.x;
    }
    track.x + (i64::from(tick.min(total)) * i64::from(track.w) / i64::from(total)) as i32
}

/// A time as the bar writes it: `M:SS`, or `H:MM:SS` from the first hour.
pub fn time_text(ms: u64) -> String {
    crate::overlay::clock_text(u32::try_from(ms).unwrap_or(u32::MAX))
}

/// The vision button's words; a computer player's view is marked, since it has no fog of war.
pub fn vision_text(v: Vision, computer: bool) -> String {
    match v {
        Vision::Everyone => "View: Everyone".to_string(),
        Vision::Player(p) if computer => format!("View: Player {} (AI)", u32::from(p) + 1),
        Vision::Player(p) => format!("View: Player {}", u32::from(p) + 1),
    }
}

/// What a key does to the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAct {
    /// Jump back or ahead by this many milliseconds of game time.
    Back(u64),
    Ahead(u64),
    Start,
    End,
    NextVision,
}

/// The viewer action of a key press with the modifiers held, if it has one.
pub fn key_act(key: Key, shift: bool, ctrl: bool, alt: bool) -> Option<KeyAct> {
    let jump = if shift { LONG_JUMP_MS } else { SHORT_JUMP_MS };
    match key {
        Key::Left if alt && !ctrl => Some(KeyAct::Back(jump)),
        Key::Right if alt && !ctrl => Some(KeyAct::Ahead(jump)),
        Key::Home if alt && !ctrl => Some(KeyAct::Start),
        Key::End if alt && !ctrl => Some(KeyAct::End),
        Key::Tab if !alt && !ctrl => Some(KeyAct::NextVision),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Rect = Rect::new(176, 16, 448, 448);

    #[test]
    fn the_bar_sits_along_the_bottom_of_the_view_and_its_parts_do_not_overlap() {
        let bar = bar_rect(VIEW);
        assert_eq!((bar.x, bar.w as i32), (180, 440));
        assert_eq!(bar.y + bar.h as i32, 16 + 448 - MARGIN);
        let (t, b, time) = (track_rect(bar), button_rect(bar), time_rect(bar));
        assert!(time.x + time.w as i32 <= t.x);
        assert!(t.x + t.w as i32 <= b.x);
        assert_eq!(b.x + b.w as i32, bar.x + bar.w as i32);
        assert!(t.w > 100, "the track has room to aim at");
    }

    #[test]
    fn a_press_on_the_track_is_a_cycle_and_the_ends_are_the_start_and_the_end() {
        let track = track_rect(bar_rect(VIEW));
        assert_eq!(tick_at(track, track.x as f32, 3000), 0);
        assert_eq!(tick_at(track, (track.x + track.w as i32) as f32, 3000), 3000);
        assert_eq!(tick_at(track, (track.x + track.w as i32 / 2) as f32, 3000), 1500);
        assert_eq!(tick_at(track, 0.0, 3000), 0, "left of the track");
        assert_eq!(tick_at(track, 10_000.0, 3000), 3000, "right of it");
        assert_eq!(tick_at(Rect::new(0, 0, 0, 14), 5.0, 3000), 0);
    }

    #[test]
    fn the_knob_and_the_press_agree() {
        let track = track_rect(bar_rect(VIEW));
        for tick in [0, 1, 750, 1499, 3000] {
            let x = knob_x(track, tick, 3000);
            let back = tick_at(track, x as f32, 3000);
            // One pixel of the track is `3000 / w` cycles.
            assert!(back.abs_diff(tick) <= 3000 / track.w + 1, "{tick} -> {x} -> {back}");
        }
        assert_eq!(knob_x(track, 10, 0), track.x, "an empty recording");
        assert_eq!(
            knob_x(track, 9999, 3000),
            track.x + track.w as i32,
            "past the end stays on the track"
        );
    }

    #[test]
    fn times_and_visions_read_plainly() {
        assert_eq!(time_text(0), "0:00");
        assert_eq!(time_text(201_000), "3:21");
        assert_eq!(time_text(3_725_000), "1:02:05");
        assert_eq!(vision_text(Vision::Player(1), false), "View: Player 2");
        assert_eq!(vision_text(Vision::Player(0), true), "View: Player 1 (AI)");
        assert_eq!(vision_text(Vision::Everyone, false), "View: Everyone");
    }

    #[test]
    fn the_keys_are_alt_chords_that_the_map_scroll_does_not_use_and_tab() {
        let act = |k, shift, ctrl, alt| key_act(k, shift, ctrl, alt);
        assert_eq!(act(Key::Left, false, false, true), Some(KeyAct::Back(SHORT_JUMP_MS)));
        assert_eq!(act(Key::Right, false, false, true), Some(KeyAct::Ahead(SHORT_JUMP_MS)));
        assert_eq!(act(Key::Left, true, false, true), Some(KeyAct::Back(LONG_JUMP_MS)));
        assert_eq!(act(Key::Home, false, false, true), Some(KeyAct::Start));
        assert_eq!(act(Key::End, false, false, true), Some(KeyAct::End));
        assert_eq!(act(Key::Tab, false, false, false), Some(KeyAct::NextVision));
        // Without Alt the arrow and Home keys are the map's scroll keys: no jump.
        for k in [Key::Left, Key::Right, Key::Home, Key::End] {
            assert_eq!(act(k, false, false, false), None);
            assert_eq!(act(k, true, false, false), None);
        }
        assert_eq!(act(Key::Left, false, true, true), None, "Ctrl with Alt is not ours");
        assert_eq!(act(Key::Up, false, false, true), None);
    }
}
