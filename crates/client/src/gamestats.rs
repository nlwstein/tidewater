//! The post-game statistics page (option `game-stats`, `extras`, `docs/design/extras.md`): line graphs over game time
//! of each player's gold, lumber and oil gathered, army size, workers and buildings, shown after the original score
//! screen. Our own design, not the original's; the score screen itself is unchanged.
//!
//! - **Sampling.** [`History::record`] runs after every game cycle and keeps a [`Sample`] each [`History::interval`]
//!   cycles, read from the world (it never writes to it). The first cycle after sampling starts is sampled too, so a
//!   loaded game's graphs begin at the load, and so do the ones of a game where the option was turned on late.
//!   A replay is sampled like a played game. [`History::finish`] adds the last cycle at the game's end.
//! - **Bounded size.** At [`MAX_SAMPLES`] samples every second one is dropped and the interval doubles, so a very long
//!   game stays small and its graph keeps its shape.
//! - **Time axis.** Each sample holds the game clock's milliseconds (what the clock option shows), which is what the
//!   graphs' horizontal axis counts; a run with no clock uses [`FALLBACK_MS_PER_CYCLE`].
//! - **What a graph shows.** Gold, lumber and oil are the totals gathered (what the score screen's bars end at).
//!   Army is the player's finished units that are neither workers nor oil tankers; workers are peasants and peons;
//!   buildings are finished buildings that count in the score (`World::counted`). Units dying or already gone don't count.
//! - **Hidden information.** The numbers are the sim's, for every player. They are shown only on the page that follows
//!   the score screen, when the game is over.
//!
//! Drawing is in [`draw`]; the scaling rules ([`nice_max`], [`plot_x`], [`plot_y`]) are plain functions with tests.

use tidewater_formats::fnt::Font;
use tidewater_formats::udta::flags;
use tidewater_sim::World;

use crate::hud::{self, Rect};
use crate::scene::Picture;

/// Cycles between two samples at first.
pub const FIRST_INTERVAL: u32 = 120;
/// Samples kept before the history thins itself.
pub const MAX_SAMPLES: usize = 1024;
/// Milliseconds a cycle counts when there is no game clock (the Enhanced normal speed's cycle).
pub const FALLBACK_MS_PER_CYCLE: u32 = 50;

/// The numbers graphed, in the order of [`NAMES`].
pub const METRICS: usize = 6;
pub const NAMES: [&str; METRICS] = [
    "Gold gathered",
    "Lumber gathered",
    "Oil gathered",
    "Army",
    "Workers",
    "Buildings",
];

/// One reading of every player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// The game cycle and the game clock's milliseconds.
    pub tick: u32,
    pub ms: u32,
    pub players: [[u32; METRICS]; 8],
}

/// The readings of a game so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub samples: Vec<Sample>,
    /// Cycles between samples now.
    pub interval: u32,
    next: u32,
    /// [`History::finish`] has run: the game is over and nothing more is taken.
    ended: bool,
}

impl Default for History {
    fn default() -> Self {
        History {
            samples: Vec::new(),
            interval: FIRST_INTERVAL,
            next: 0,
            ended: false,
        }
    }
}

/// Player `p`'s numbers now (see the module doc).
pub fn measure(w: &World, p: u8) -> [u32; METRICS] {
    let pl = &w.players[p as usize];
    let (mut army, mut workers, mut buildings) = (0, 0, 0);
    for u in w
        .units
        .iter()
        .filter(|u| u.owner == p && !u.dying && !u.removing && World::counted(u))
    {
        if u.building {
            buildings += 1;
            continue;
        }
        let f = w.stats.0.get(u.kind as usize).map_or(0, |s| s.flags);
        if f & flags::PEON != 0 {
            workers += 1;
        } else if f & flags::TANKER == 0 {
            army += 1;
        }
    }
    [pl.gathered[0], pl.gathered[1], pl.gathered[2], army, workers, buildings]
}

impl History {
    /// Takes a sample if the world is at or past the next one due. `ms` is the game clock now, if there is one.
    pub fn record(&mut self, w: &World, ms: Option<u32>) {
        self.record_with(w.tick, ms, |p| measure(w, p));
    }

    /// [`History::record`] with the readings given by `read` (player to numbers).
    pub fn record_with(&mut self, tick: u32, ms: Option<u32>, read: impl Fn(u8) -> [u32; METRICS]) {
        if self.ended || tick < self.next && !self.samples.is_empty() {
            return;
        }
        self.push(tick, ms, read);
        self.next = tick.saturating_add(self.interval);
    }

    /// Takes a sample now whatever is due (the game's end), unless the last one is already of this cycle.
    pub fn finish(&mut self, w: &World, ms: Option<u32>) {
        self.finish_with(w.tick, ms, |p| measure(w, p));
    }

    /// [`History::finish`] with the readings given by `read`.
    pub fn finish_with(&mut self, tick: u32, ms: Option<u32>, read: impl Fn(u8) -> [u32; METRICS]) {
        if !self.ended && self.samples.last().is_none_or(|s| s.tick != tick) {
            self.push(tick, ms, read);
        }
        self.ended = true;
    }

    fn push(&mut self, tick: u32, ms: Option<u32>, read: impl Fn(u8) -> [u32; METRICS]) {
        let players = std::array::from_fn(|p| read(p as u8));
        let ms = ms.unwrap_or_else(|| tick.saturating_mul(FALLBACK_MS_PER_CYCLE));
        self.samples.push(Sample { tick, ms, players });
        if self.samples.len() > MAX_SAMPLES {
            // Every second sample goes, the first and the newest stay.
            let last = self.samples.len() - 1;
            let mut i = 0;
            self.samples.retain(|_| {
                let keep = i % 2 == 0 || i == last;
                i += 1;
                keep
            });
            self.interval = self.interval.saturating_mul(2);
        }
    }

    /// The game clock's reading at the last sample.
    pub fn end_ms(&self) -> u32 {
        self.samples.last().map_or(0, |s| s.ms)
    }

    /// Whether there is anything to draw: a graph needs two points.
    pub fn drawable(&self) -> bool {
        self.samples.len() >= 2 && self.end_ms() > self.samples[0].ms
    }

    /// The highest value of `metric` over the `slots` shown.
    pub fn peak(&self, metric: usize, slots: &[u8]) -> u32 {
        self.samples
            .iter()
            .flat_map(|s| slots.iter().map(move |&p| s.players[p as usize & 7][metric]))
            .max()
            .unwrap_or(0)
    }
}

/// The axis top for a largest value: 1, 2 or 5 times a power of ten that is not below it, and at least 5 so a graph of
/// small counts doesn't stretch one unit over the whole height. 0 gives 5.
pub fn nice_max(v: u32) -> u32 {
    let mut step = 1u64;
    loop {
        for m in [1u64, 2, 5] {
            let top = m * step;
            if top >= u64::from(v) && top >= 5 {
                return u32::try_from(top).unwrap_or(u32::MAX);
            }
        }
        step *= 10;
    }
}

/// The pixel column of time `ms` in a plot `w` pixels wide that spans `start_ms` to `end_ms` (0 to `w - 1`).
pub fn plot_x(ms: u32, start_ms: u32, end_ms: u32, w: i32) -> i32 {
    let span = i64::from(end_ms.saturating_sub(start_ms)).max(1);
    let at = i64::from(ms.saturating_sub(start_ms)).min(span);
    (at * i64::from((w - 1).max(0)) / span) as i32
}

/// The pixel row of `v` in a plot `h` pixels high whose top is `max`: 0 is the top row, `h - 1` the bottom (zero).
pub fn plot_y(v: u32, max: u32, h: i32) -> i32 {
    let max = i64::from(max).max(1);
    let v = i64::from(v).min(max);
    let rise = v * i64::from((h - 1).max(0)) / max;
    (h - 1).max(0) - rise as i32
}

/// A player's graph line colour: the minimap's player colours as the original's palette gives them (red, blue, green,
/// violet, orange, black shown as grey so it can be seen, white, yellow).
pub const PLAYER_RGB: [[u8; 3]; 8] = [
    [220, 40, 40],
    [60, 100, 240],
    [40, 190, 70],
    [170, 70, 200],
    [240, 140, 30],
    [130, 130, 130],
    [240, 240, 240],
    [240, 220, 50],
];

/// The game time as `M:SS` or `H:MM:SS`.
pub fn time_text(ms: u32) -> String {
    crate::overlay::clock_text(ms)
}

/// A player in the legend: the slot and the name shown.
pub struct Who<'a> {
    pub slot: u8,
    pub name: &'a str,
}

const GOLD: [u8; 3] = [200, 170, 60];
const GRID: [u8; 3] = [70, 60, 30];
const PANEL: [u8; 3] = [14, 12, 8];

/// Draws the page over `p` (the score screen's background, already shaded by the caller's choice): the heading, a
/// legend and six graphs in two columns, at 640 x 480.
pub fn draw(p: &mut Picture, big: &Font, font: &Font, small: &Font, h: &History, who: &[Who]) {
    let head = "Statistics";
    let hw = big.width(head.as_bytes()) as i32;
    hud::text(p, big, (p.width as i32 - hw) / 2, 8, head);
    // The legend: a colour square and the name, spread across the width.
    let slots: Vec<u8> = who.iter().map(|w| w.slot).collect();
    let cell = 608 / who.len().clamp(1, 8) as i32;
    for (i, w) in who.iter().enumerate() {
        let x = 16 + i as i32 * cell;
        hud::fill(p, Rect { x, y: 56, w: 10, h: 10 }, PLAYER_RGB[w.slot as usize & 7]);
        let name = clip(small, w.name, (cell - 16).max(8) as u32);
        hud::text(p, small, x + 14, 54, &name);
    }
    for (k, name) in NAMES.iter().enumerate() {
        let (col, row) = (k as i32 % 2, k as i32 / 2);
        let r = Rect {
            x: 16 + col * 312,
            y: 76 + row * 116,
            w: 296,
            h: 108,
        };
        graph(p, font, small, r, name, h, k, &slots);
    }
    let foot = format!("Game time {}   Press a key to continue", time_text(h.end_ms()));
    let fw = small.width(foot.as_bytes()) as i32;
    hud::text(p, small, (p.width as i32 - fw) / 2, 440, &foot);
}

/// `s` cut to fit `max` pixels.
fn clip(font: &Font, s: &str, max: u32) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let mut t = out.clone();
        t.push(c);
        if font.width(t.as_bytes()) > max {
            break;
        }
        out = t;
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn graph(p: &mut Picture, font: &Font, small: &Font, r: Rect, title: &str, h: &History, metric: usize, slots: &[u8]) {
    hud::fill(p, r, PANEL);
    // A one-pixel gold frame, as the score screen's bars.
    for edge in [
        Rect {
            x: r.x,
            y: r.y,
            w: r.w,
            h: 1,
        },
        Rect {
            x: r.x,
            y: r.y + r.h as i32 - 1,
            w: r.w,
            h: 1,
        },
        Rect {
            x: r.x,
            y: r.y,
            w: 1,
            h: r.h,
        },
        Rect {
            x: r.x + r.w as i32 - 1,
            y: r.y,
            w: 1,
            h: r.h,
        },
    ] {
        hud::fill(p, edge, GOLD);
    }
    let top = nice_max(h.peak(metric, slots));
    hud::text(p, font, r.x + 6, r.y + 2, title);
    let label = top.to_string();
    let lw = small.width(label.as_bytes()) as i32;
    hud::text(p, small, r.x + r.w as i32 - lw - 6, r.y + 4, &label);
    // The plot, inside the frame with room for the title above and the times below.
    let plot = Rect {
        x: r.x + 8,
        y: r.y + 22,
        w: r.w - 16,
        h: r.h - 22 - 16,
    };
    for frac in [0, 1, 2] {
        let y = plot.y + plot_y(top * frac / 2, top, plot.h as i32);
        hud::fill(
            p,
            Rect {
                x: plot.x,
                y,
                w: plot.w,
                h: 1,
            },
            GRID,
        );
    }
    let start = h.samples.first().map_or(0, |s| s.ms);
    let end = h.end_ms();
    for &slot in slots {
        let mut prev: Option<(i32, i32)> = None;
        for s in &h.samples {
            let pt = (
                plot.x + plot_x(s.ms, start, end, plot.w as i32),
                plot.y + plot_y(s.players[slot as usize & 7][metric], top, plot.h as i32),
            );
            if let Some(a) = prev {
                line(p, a, pt, PLAYER_RGB[slot as usize & 7]);
            }
            prev = Some(pt);
        }
    }
    let y = plot.y + plot.h as i32 + 3;
    hud::text(p, small, plot.x, y, &time_text(start));
    let e = time_text(end);
    let ew = small.width(e.as_bytes()) as i32;
    hud::text(p, small, plot.x + plot.w as i32 - ew, y, &e);
}

/// A two-pixel-thick line from `a` to `b` (Bresenham), so a line stays visible at 640 x 480.
fn line(p: &mut Picture, a: (i32, i32), b: (i32, i32), rgb: [u8; 3]) {
    let (mut x, mut y) = a;
    let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
    let (sx, sy) = (if a.0 < b.0 { 1 } else { -1 }, if a.1 < b.1 { 1 } else { -1 });
    let mut err = dx + dy;
    loop {
        hud::fill(p, Rect { x, y, w: 2, h: 2 }, rgb);
        if (x, y) == b {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(tick: u32, ms: u32, gold: u32) -> Sample {
        let mut players = [[0; METRICS]; 8];
        players[0][0] = gold;
        Sample { tick, ms, players }
    }

    #[test]
    fn nice_max_rounds_up_to_1_2_5() {
        assert_eq!(nice_max(0), 5);
        assert_eq!(nice_max(3), 5);
        assert_eq!(nice_max(5), 5);
        assert_eq!(nice_max(6), 10);
        assert_eq!(nice_max(11), 20);
        assert_eq!(nice_max(20), 20);
        assert_eq!(nice_max(21), 50);
        assert_eq!(nice_max(12_400), 20_000);
        assert_eq!(nice_max(u32::MAX), u32::MAX, "no overflow");
    }

    #[test]
    fn plot_scales_to_the_box() {
        assert_eq!(plot_x(0, 0, 1000, 101), 0);
        assert_eq!(plot_x(500, 0, 1000, 101), 50);
        assert_eq!(plot_x(1000, 0, 1000, 101), 100);
        assert_eq!(plot_x(5000, 0, 1000, 101), 100, "past the end stays in");
        assert_eq!(
            plot_x(700, 600, 1000, 101),
            25,
            "a start that is not zero (a loaded game)"
        );
        assert_eq!(plot_x(5, 5, 5, 101), 0, "no span");
        assert_eq!(plot_y(0, 100, 51), 50, "zero is the bottom row");
        assert_eq!(plot_y(100, 100, 51), 0, "the top is the first row");
        assert_eq!(plot_y(50, 100, 51), 25);
        assert_eq!(plot_y(500, 100, 51), 0, "above the top stays in");
        assert_eq!(plot_y(0, 0, 51), 50, "a zero top");
    }

    #[test]
    fn peak_reads_only_the_shown_slots() {
        let h = History {
            samples: vec![sample(0, 0, 10), sample(120, 1000, 90)],
            ..History::default()
        };
        assert_eq!(h.peak(0, &[0]), 90);
        assert_eq!(h.peak(0, &[1]), 0);
        assert!(h.drawable());
        assert!(!History::default().drawable());
    }

    #[test]
    fn thinning_keeps_ends_and_doubles_the_interval() {
        let mut h = History::default();
        for i in 0..=MAX_SAMPLES as u32 {
            h.samples.push(sample(i, i * 10, i));
        }
        // One more through `push` crosses the limit.
        h.push(5000, Some(99_999), |_| [0; METRICS]);
        assert!(h.samples.len() <= MAX_SAMPLES / 2 + 3, "{}", h.samples.len());
        assert_eq!(h.samples[0].tick, 0, "the first stays");
        assert_eq!(h.end_ms(), 99_999, "the newest stays");
        assert_eq!(h.interval, FIRST_INTERVAL * 2);
    }

    #[test]
    fn recording_waits_for_the_interval_and_finish_adds_the_end() {
        let read = |p: u8| [u32::from(p) + 1, 0, 0, 0, 0, 0];
        let mut h = History::default();
        h.record_with(1000, Some(5_000), read);
        assert_eq!(h.samples.len(), 1, "the first cycle after a load is sampled");
        assert_eq!(h.samples[0].tick, 1000);
        assert_eq!(h.samples[0].players[3][0], 4, "each player is read");
        h.record_with(1050, Some(5_500), read);
        assert_eq!(h.samples.len(), 1, "not due");
        h.record_with(1120, Some(6_000), read);
        assert_eq!(h.samples.len(), 2);
        h.finish_with(1120, Some(6_000), read);
        assert_eq!(h.samples.len(), 2, "the last cycle is already in");
        h.finish_with(1130, None, read);
        assert_eq!(h.samples.len(), 2, "finish ended it");
        let mut h2 = History::default();
        h2.finish_with(1130, None, read);
        assert_eq!(
            h2.samples[0].ms,
            1130 * FALLBACK_MS_PER_CYCLE,
            "no clock: the cycle's length"
        );
        h2.record_with(2000, None, read);
        assert_eq!(h2.samples.len(), 1, "nothing after the end");
    }

    #[test]
    fn drawing_puts_each_players_colour_on_the_graph() {
        let (big, font, small) = (test_font(), test_font(), test_font());
        let h = History {
            samples: vec![sample(0, 0, 0), sample(120, 60_000, 1000)],
            ..History::default()
        };
        let mut p = Picture {
            width: 640,
            height: 480,
            rgba: vec![0; 640 * 480 * 4],
        };
        draw(&mut p, &big, &font, &small, &h, &[Who { slot: 0, name: "You" }]);
        let red = PLAYER_RGB[0];
        let has = p.rgba.chunks_exact(4).any(|px| px[..3] == red);
        assert!(has, "player 0's colour is drawn");
    }

    fn test_font() -> Font {
        Font {
            first: 0,
            max_width: 8,
            height: 8,
            glyphs: Vec::new(),
        }
    }
}
