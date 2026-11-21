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
//! - **The page.** A legend (a color square and the name of every player, wrapped onto more lines if the names are wide)
//!   over up to six graphs, two to a row; a graph where nobody has anything (oil on a map with no water) is left out, and
//!   a last graph on its own spans the width. Each graph names its unit at the top right, labels its vertical axis at
//!   every grid line (round numbers, [`axis`]) and its horizontal axis from the first to the last game time. Every size
//!   is taken from the fonts it is given, so a wider font (the clear font) moves things instead of overlapping them.
//!
//! Drawing is in [`draw`]; the scaling rules ([`axis`], [`plot_x`], [`plot_y`]) and the layout ([`graph_rects`],
//! [`legend_cells`]) are plain functions with tests.

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
/// What each number counts, shown at the top right of its graph.
pub const UNITS: [&str; METRICS] = ["gold", "lumber", "oil", "units", "workers", "buildings"];

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

/// Most grid intervals a graph's vertical axis has.
const MAX_INTERVALS: u64 = 5;

/// A graph's vertical axis: a grid line every `step`, the highest at `top` (a whole number of steps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Axis {
    pub step: u32,
    pub top: u32,
}

/// The axis for a largest value: the smallest step of 1, 2 or 5 times a power of ten that needs at most `max` grid
/// intervals (2 to [`MAX_INTERVALS`]; a short graph takes fewer so its labels don't touch) to reach it, and at least
/// two, so a graph of small counts doesn't stretch one unit over the whole height. With 5: 21,000 gives steps of
/// 5,000 up to 25,000; 3 gives steps of 1 up to 3; 0 gives 1 and 2.
pub fn axis(peak: u32, max: u64) -> Axis {
    let max = max.clamp(2, MAX_INTERVALS);
    let mut base = 1u64;
    loop {
        for m in [1u64, 2, 5] {
            let step = m * base;
            let n = u64::from(peak).div_ceil(step).max(2);
            if n <= max {
                let top = u32::try_from(n * step).unwrap_or(u32::MAX);
                return Axis {
                    step: u32::try_from(step).unwrap_or(u32::MAX),
                    top,
                };
            }
        }
        base *= 10;
    }
}

/// A grid line's label: whole thousands as `25k`, anything else in full.
pub fn tick_label(v: u32) -> String {
    if v >= 1000 && v.is_multiple_of(1000) {
        format!("{}k", v / 1000)
    } else {
        v.to_string()
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
const BASE: [u8; 3] = [130, 110, 50];
const PANEL: [u8; 3] = [14, 12, 8];

/// The page's side margin and the bottom of the graphs.
const MARGIN: i32 = 16;
const GRAPHS_BOTTOM: i32 = 440;
/// The gap between graphs, and the tallest one (so a page of one or two graphs isn't a pair of tall boxes).
const GAP: i32 = 8;
const MAX_GRAPH_H: i32 = 150;
/// The legend's color square and the room between its entries.
const SWATCH: i32 = 10;
const ENTRY_GAP: i32 = 14;

/// Where the legend's entries go: the top-left of each entry's color square row (x, y) for entries of the widths
/// given (name widths in pixels), starting at `y0`, wrapping at the page's right margin; and the y under the last row.
pub fn legend_cells(widths: &[i32], line_h: i32, y0: i32, page_w: i32) -> (Vec<(i32, i32)>, i32) {
    let (mut x, mut y) = (MARGIN, y0);
    let mut at = Vec::new();
    for &w in widths {
        let cell = SWATCH + 4 + w;
        if x > MARGIN && x + cell > page_w - MARGIN {
            x = MARGIN;
            y += line_h;
        }
        at.push((x, y));
        x += cell + ENTRY_GAP;
    }
    (at, y + line_h)
}

/// The boxes of `count` graphs in two columns between `top` and [`GRAPHS_BOTTOM`] on a page `page_w` wide; a last
/// graph with no partner spans both columns.
pub fn graph_rects(count: usize, top: i32, page_w: i32) -> Vec<Rect> {
    let rows = count.div_ceil(2).max(1) as i32;
    let h = ((GRAPHS_BOTTOM - top - GAP * (rows - 1)) / rows).clamp(40, MAX_GRAPH_H);
    let full = page_w - 2 * MARGIN;
    let half = (full - GAP) / 2;
    (0..count)
        .map(|k| {
            let (col, row) = (k as i32 % 2, k as i32 / 2);
            let alone = k + 1 == count && count % 2 == 1;
            Rect {
                x: MARGIN + if alone { 0 } else { col * (half + GAP) },
                y: top + row * (h + GAP),
                w: (if alone { full } else { half }) as u32,
                h: h as u32,
            }
        })
        .collect()
}

/// Draws the page over `p` (the score screen's background, already shaded by the caller's choice): the heading, a
/// legend and the graphs, at 640 x 480.
pub fn draw(p: &mut Picture, big: &Font, font: &Font, small: &Font, h: &History, who: &[Who]) {
    let head = "Statistics";
    let hw = big.width(head.as_bytes()) as i32;
    hud::text(p, big, (p.width as i32 - hw) / 2, 8, head);
    // The legend: a color square and the name of every player.
    let slots: Vec<u8> = who.iter().map(|w| w.slot).collect();
    let line_h = i32::from(small.height).max(SWATCH) + 4;
    let names: Vec<String> = who.iter().map(|w| clip(small, w.name, 300)).collect();
    let widths: Vec<i32> = names.iter().map(|n| small.width(n.as_bytes()) as i32).collect();
    let y0 = (12 + i32::from(big.height)).max(46);
    let (cells, below) = legend_cells(&widths, line_h, y0, p.width as i32);
    for ((w, name), &(x, y)) in who.iter().zip(&names).zip(&cells) {
        let sy = y + (line_h - 4 - SWATCH) / 2;
        hud::fill(
            p,
            Rect {
                x,
                y: sy,
                w: SWATCH as u32,
                h: SWATCH as u32,
            },
            PLAYER_RGB[w.slot as usize & 7],
        );
        hud::text(p, small, x + SWATCH + 4, y, name);
    }
    // Only the graphs that have something to show.
    let shown: Vec<usize> = (0..METRICS).filter(|&k| h.peak(k, &slots) > 0).collect();
    for (&k, r) in shown.iter().zip(graph_rects(shown.len(), below + 4, p.width as i32)) {
        graph(p, font, small, r, k, h, &slots);
    }
    let foot = "Horizontal axis: game time (minutes:seconds)   Press a key to continue";
    let foot = clip(small, foot, p.width - 2 * MARGIN as u32);
    let fw = small.width(foot.as_bytes()) as i32;
    hud::text(p, small, (p.width as i32 - fw) / 2, GRAPHS_BOTTOM + 8, &foot);
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

fn frame(p: &mut Picture, r: Rect) {
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
}

/// One graph of number `metric` in box `r`: title and unit on top, a labeled grid, a line per player, the times below.
fn graph(p: &mut Picture, font: &Font, small: &Font, r: Rect, metric: usize, h: &History, slots: &[u8]) {
    hud::fill(p, r, PANEL);
    // A one-pixel gold frame, as the score screen's bars.
    frame(p, r);
    let (fh, sh) = (i32::from(font.height).max(8), i32::from(small.height).max(6));
    let title = NAMES[metric];
    hud::text(p, font, r.x + 6, r.y + 2, title);
    let (tw, unit) = (font.width(title.as_bytes()) as i32, UNITS[metric]);
    let uw = small.width(unit.as_bytes()) as i32;
    if tw + uw + 18 <= r.w as i32 {
        hud::text(p, small, r.x + r.w as i32 - uw - 6, r.y + 2 + (fh - sh).max(0), unit);
    }
    // The most grid lines whose labels fit one above the other in the box.
    let room = i64::from(r.h) - i64::from(fh) - 12 - i64::from(sh) * 2;
    let ax = axis(h.peak(metric, slots), (room / i64::from(sh + 6)).max(2) as u64);
    let ticks: Vec<u32> = (0..=ax.top / ax.step).map(|k| k * ax.step).collect();
    let label_w = ticks
        .iter()
        .map(|&t| small.width(tick_label(t).as_bytes()) as i32)
        .max()
        .unwrap_or(0);
    // The plot, inside the frame: labels to its left, the title above, the times below; half a label of room above
    // and below the plot so the first and last labels stay inside the frame.
    let left = r.x + 6 + label_w + 5;
    let top = r.y + fh + 6 + sh / 2;
    let plot = Rect {
        x: left,
        y: top,
        w: (r.x + r.w as i32 - 8 - left).max(8) as u32,
        h: (r.y + r.h as i32 - 4 - sh - 2 - top).max(8) as u32,
    };
    for &t in &ticks {
        let y = plot.y + plot_y(t, ax.top, plot.h as i32);
        hud::fill(
            p,
            Rect {
                x: plot.x,
                y,
                w: plot.w,
                h: 1,
            },
            if t == 0 { BASE } else { GRID },
        );
        let l = tick_label(t);
        let lw = small.width(l.as_bytes()) as i32;
        hud::text(p, small, plot.x - 4 - lw, y - sh / 2, &l);
    }
    let start = h.samples.first().map_or(0, |s| s.ms);
    let end = h.end_ms();
    for &slot in slots {
        let mut prev: Option<(i32, i32)> = None;
        for s in &h.samples {
            let pt = (
                plot.x + plot_x(s.ms, start, end, plot.w as i32),
                plot.y + plot_y(s.players[slot as usize & 7][metric], ax.top, plot.h as i32),
            );
            if let Some(a) = prev {
                line(p, a, pt, PLAYER_RGB[slot as usize & 7]);
            }
            prev = Some(pt);
        }
    }
    let y = plot.y + plot.h as i32 + 4;
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
    fn axis_rounds_up_to_whole_steps_of_1_2_5() {
        assert_eq!(axis(0, 5), Axis { step: 1, top: 2 });
        assert_eq!(axis(3, 5), Axis { step: 1, top: 3 });
        assert_eq!(axis(5, 5), Axis { step: 1, top: 5 });
        assert_eq!(axis(6, 5), Axis { step: 2, top: 6 });
        assert_eq!(axis(11, 5), Axis { step: 5, top: 15 });
        assert_eq!(
            axis(21_000, 5),
            Axis {
                step: 5000,
                top: 25_000
            }
        );
        assert_eq!(
            axis(12_400, 5),
            Axis {
                step: 5000,
                top: 15_000
            }
        );
        assert_eq!(
            axis(100_000, 5),
            Axis {
                step: 20_000,
                top: 100_000
            }
        );
        let a = axis(u32::MAX, 5);
        assert_eq!(a.top, u32::MAX, "no overflow");
        for v in [0, 1, 7, 99, 1234, 56_789, 3_000_000] {
            let a = axis(v, 5);
            assert!(
                a.top >= v && a.top.is_multiple_of(a.step) && a.top / a.step <= 5,
                "{v}: {a:?}"
            );
        }
        assert_eq!(
            axis(21_000, 3),
            Axis {
                step: 10_000,
                top: 30_000
            },
            "fewer lines in a short graph"
        );
        assert_eq!(axis(7, 1), axis(7, 2), "at least two intervals");
        assert_eq!(tick_label(0), "0");
        assert_eq!(tick_label(500), "500");
        assert_eq!(tick_label(25_000), "25k");
        assert_eq!(tick_label(2500), "2500");
    }

    #[test]
    fn graphs_fill_the_page_without_overlap() {
        for count in 1..=6 {
            let rs = graph_rects(count, 70, 640);
            assert_eq!(rs.len(), count);
            for (i, a) in rs.iter().enumerate() {
                assert!(
                    a.x >= MARGIN && a.x + a.w as i32 <= 640 - MARGIN,
                    "{count} {i} in width"
                );
                assert!(a.y >= 70 && a.y + a.h as i32 <= GRAPHS_BOTTOM, "{count} {i} in height");
                for b in &rs[i + 1..] {
                    let apart = a.x + a.w as i32 <= b.x
                        || b.x + b.w as i32 <= a.x
                        || a.y + a.h as i32 <= b.y
                        || b.y + b.h as i32 <= a.y;
                    assert!(apart, "{count}: {a:?} and {b:?} overlap");
                }
            }
        }
        let five = graph_rects(5, 70, 640);
        assert_eq!(five[4].w, 608, "a lone last graph spans the width");
    }

    #[test]
    fn the_legend_wraps_instead_of_overflowing() {
        let (at, below) = legend_cells(&[60, 60, 60], 14, 50, 640);
        assert_eq!(at, vec![(16, 50), (104, 50), (192, 50)]);
        assert_eq!(below, 64);
        let wide = [200; 8];
        let (at, below) = legend_cells(&wide, 14, 50, 640);
        assert!(at.iter().all(|&(x, _)| x + 214 <= 640 - MARGIN), "{at:?}");
        assert!(at.windows(2).all(|w| w[1].1 >= w[0].1));
        assert!(below > 50 + 14, "wrapped onto more lines: {below}");
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
