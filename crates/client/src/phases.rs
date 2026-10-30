//! Where a frame's time goes, for measuring (`TIDEWATER_FRAME_STATS`, [`crate::host::FrameStats`]): the core and the
//! shell time their parts of a frame with [`timed`], and the frame stats take the sums once a frame ([`take`]). Off,
//! [`timed`] only runs its work.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// A part of a frame. The game's drawing (terrain, map view, interface) is part of the frame's systems.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Time, input and the flow's change of screen (`Core::advance`'s steps 1 to 3).
    Input,
    /// The game's cycles.
    Cycles,
    /// The frame's systems, the game's drawing among them.
    Systems,
    /// The terrain picture's redraw (`game::redraw_terrain`), with the palette's cycling.
    Terrain,
    /// The map view's picture: units, missiles, fog (`game::draw_view`).
    View,
    /// Of the map view: the fog (`fog::draw`).
    Fog,
    /// Of the map view: its palette indices made RGBA (`view::Canvas::rgba`).
    Colours,
    /// The interface picture (`game::draw_hud`).
    Hud,
    /// The layers composed into the frame (`compose_screen`).
    Compose,
    /// The shell's copy of the frame to the screen's texture, and its drawing.
    Upload,
    /// The shell's showing of the drawn screen (with vsync, its wait for the display).
    Present,
}

impl Phase {
    pub const ALL: [Phase; 11] = [
        Phase::Input,
        Phase::Cycles,
        Phase::Systems,
        Phase::Terrain,
        Phase::View,
        Phase::Fog,
        Phase::Colours,
        Phase::Hud,
        Phase::Compose,
        Phase::Upload,
        Phase::Present,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Phase::Input => "input",
            Phase::Cycles => "cycles",
            Phase::Systems => "systems",
            Phase::Terrain => "terrain",
            Phase::View => "view",
            Phase::Fog => "fog",
            Phase::Colours => "colours",
            Phase::Hud => "hud",
            Phase::Compose => "compose",
            Phase::Upload => "upload",
            Phase::Present => "present",
        }
    }
}

static ON: AtomicBool = AtomicBool::new(false);

thread_local! {
    static SPENT: Cell<[f32; Phase::ALL.len()]> = const { Cell::new([0.0; Phase::ALL.len()]) };
}

/// Starts (or stops) the timing.
pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Runs `work`, adding its time to `phase`'s while the timing is on.
pub fn timed<R>(phase: Phase, work: impl FnOnce() -> R) -> R {
    if !ON.load(Ordering::Relaxed) {
        return work();
    }
    let start = Instant::now();
    let r = work();
    add(phase, start);
    r
}

/// Adds the time since `start` to `phase`'s while the timing is on, for work that doesn't fit in a closure.
pub fn add(phase: Phase, start: Instant) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    let ms = start.elapsed().as_secs_f32() * 1000.0;
    SPENT.with(|s| {
        let mut v = s.get();
        v[phase as usize] += ms;
        s.set(v);
    });
}

/// Each phase's milliseconds since the last call, in [`Phase::ALL`]'s order.
pub fn take() -> [f32; Phase::ALL.len()] {
    SPENT.with(|s| s.replace([0.0; Phase::ALL.len()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_work_adds_to_its_phase_only_while_on() {
        take();
        timed(Phase::View, || std::thread::sleep(std::time::Duration::from_millis(2)));
        assert_eq!(take(), [0.0; Phase::ALL.len()], "off");
        set_on(true);
        let r = timed(Phase::View, || {
            std::thread::sleep(std::time::Duration::from_millis(2));
            7
        });
        set_on(false);
        assert_eq!(r, 7);
        let t = take();
        assert!(t[Phase::View as usize] >= 2.0);
        assert_eq!(t[Phase::Hud as usize], 0.0);
        assert_eq!(take(), [0.0; Phase::ALL.len()], "taken");
    }
}
