//! Colour cycling and palette effects as `docs/specs/ui/palette-effects.md` describes them: the 8 cycles per era, the
//! real-time step rule, the placement pulse, screen fades and the screenshot flash. No SDL types here.
//!
//! [`Cycling`] owns the current 256-entry palette: it starts as the era's base palette, and every turn of a cycle
//! changes entries of it. The caller converts indexed pictures with [`Cycling::palette`] each frame (see `main.rs`).
//! Which cycles a step turned comes back as a mask (bit n for cycle n), so the caller can redraw only the terrain tiles
//! that use the changed entries ([`Entries`]).
//!
//! Choices made where the spec has gaps:
//!
//! - **Pulse state.** Cycle 0 turns on at the first owner change ([`Cycling::owner_changed`]). The spec says the on
//!   state is saved with the game; there are no saved games yet and the sim has no field for it, so the state lives
//!   here and `World::state_hash` is untouched. Saving will need a sim field (or the on set in the save) later.
//! - **Placing.** Cycle 7 follows "is the player placing" every frame, whether or not a step happens; it is off
//!   whenever placing ends, for any reason (the spec's default for its open question).
//! - **First step.** The time of the last step is reset when the first frame is seen, which stands in for "the main
//!   loop starts"; no step happens on that frame.
//! - **Palette values.** Colour lists are 8-bit values as the spec gives them, put straight into the palette.
//! - **Fades.** [`Fade::frame_times`] is the schedule only; waiting out a frame is the caller's business.
//! - **Shown palette.** The cycled palette is what the game draws with; what the screen shows is [`shown`] of it: the
//!   greyscale while paused (`ui/game-menu.md`, "Greyscale while paused"), the negative during the screenshot flash,
//!   and the game start's fade-in. The caller redraws whatever uses the entries that changed ([`Entries::changed`]).
//! - **Game end fade.** The 1 ms fade-out at the end of a game is a cut to black, which leaving the game screen for
//!   the next screen already is (the front end clears to black); the greyscale goes with the game screen.
//! - **Game start fade.** The 10 ms fade-in shows its frame at t = 0 (all black) for one frame, then t = 10 (the
//!   fade's frames last at least 16 ms, so there are no frames in between).

use tidewater_formats::pud::Era;
use tidewater_formats::tileset::Palette;

/// A colour, R, G, B.
pub type Rgb = [u8; 3];

/// The shortest time between steps in milliseconds ("Steps").
pub const STEP_MS: u64 = 20;
/// The countdown a cycle gets after a turn is its period plus this.
pub const AFTER_TURN: u32 = 1;
/// A cycle this close to due (countdown after lowering) is pulled along by a turn.
pub const PULL_MARGIN: u32 = 2;
/// The number of cycles.
pub const CYCLES: usize = 8;
/// The pulse cycle (cycle 0).
pub const PULSE_CYCLE: usize = 0;
/// The placement image's cycle and its entry.
pub const PLACEMENT_CYCLE: usize = 7;
pub const PLACEMENT_ENTRY: u8 = 10;
/// The minimap attack mark's entry (`minimap::colour::ATTACKED`).
#[cfg(test)]
pub const MARK_ENTRY: u8 = 3;

const PULSE: [Rgb; 10] = [
    [255, 0, 0],
    [223, 0, 0],
    [190, 0, 0],
    [158, 0, 0],
    [125, 0, 0],
    [150, 0, 0],
    [174, 0, 0],
    [203, 0, 0],
    [227, 0, 0],
    [255, 0, 0],
];
const MARK: [Rgb; 2] = [[255, 0, 0], [0, 255, 0]];
/// The first 16 placement shades, going up to pure red.
const PLACEMENT_UP: [Rgb; 16] = [
    [77, 60, 60],
    [89, 65, 65],
    [97, 69, 69],
    [109, 69, 69],
    [121, 69, 69],
    [134, 69, 69],
    [146, 69, 69],
    [158, 65, 65],
    [170, 60, 60],
    [182, 56, 56],
    [195, 48, 48],
    [207, 40, 40],
    [219, 28, 28],
    [231, 20, 20],
    [243, 8, 8],
    [255, 0, 0],
];
const fn placement() -> [Rgb; 32] {
    let mut o = [[0u8; 3]; 32];
    let mut i = 0;
    while i < 16 {
        o[i] = PLACEMENT_UP[i];
        o[31 - i] = PLACEMENT_UP[i];
        i += 1;
    }
    o
}
/// The 32 placement shades: 16 up, then the same 16 down.
const PLACEMENT: [Rgb; 32] = placement();

/// What a cycle does when it turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colours {
    /// Each entry takes the colour of the one before it; the first takes the last's.
    Rotate,
    /// The entries take colours from a list, one position on each turn.
    List(&'static [Rgb]),
}

/// A cycle's fixed data ("Cycle tables").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Def {
    pub period: u32,
    pub first: u8,
    pub last: u8,
    pub colours: Colours,
}

const fn def(period: u32, first: u8, last: u8, colours: Colours) -> Def {
    Def {
        period,
        first,
        last,
        colours,
    }
}

const R: Colours = Colours::Rotate;
const PULSE_LIST: Colours = Colours::List(&PULSE);
const MARK_LIST: Colours = Colours::List(&MARK);

/// The era's cycles 0 upward ("Cycle tables"; cycle 7 is separate: [`placement_def`]).
pub fn defs(era: Era) -> Vec<Def> {
    match era {
        Era::Forest => vec![
            def(4, 202, 202, PULSE_LIST),
            def(8, 38, 47, R),
            def(12, 48, 56, R),
            def(20, 240, 244, R),
            def(8, 3, 3, MARK_LIST),
        ],
        Era::Winter => vec![
            def(4, 202, 202, PULSE_LIST),
            def(10, 40, 47, R),
            def(12, 48, 54, R),
            def(20, 240, 244, R),
            def(20, 205, 207, R),
            def(8, 3, 3, MARK_LIST),
        ],
        Era::Wasteland => vec![
            def(4, 202, 202, PULSE_LIST),
            def(12, 38, 47, R),
            def(8, 64, 70, R),
            def(20, 240, 244, R),
            def(8, 3, 3, MARK_LIST),
        ],
        Era::Swamp => vec![
            def(4, 4, 4, PULSE_LIST),
            def(12, 38, 47, R),
            def(8, 88, 95, R),
            def(20, 240, 244, R),
            def(8, 5, 9, R),
            def(8, 3, 3, MARK_LIST),
        ],
    }
}

/// The cycles that start on in a new game, bit n for cycle n: all of the era's but the pulse (cycle 0).
pub fn start_on(era: Era) -> u8 {
    ((1u16 << defs(era).len()) - 2) as u8
}

/// Cycle 7, the placement image.
pub fn placement_def() -> Def {
    def(1, PLACEMENT_ENTRY, PLACEMENT_ENTRY, Colours::List(&PLACEMENT))
}

/// A set of palette entries, to ask whether a picture uses entries a step changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Entries([u64; 4]);

impl Entries {
    pub fn insert(&mut self, i: u8) {
        self.0[i as usize / 64] |= 1 << (i % 64);
    }

    /// The entries used by a picture of palette indices.
    pub fn of_pixels(px: &[u8]) -> Self {
        let mut e = Entries::default();
        for &p in px {
            e.insert(p);
        }
        e
    }

    /// The entries whose colours differ between two palettes.
    pub fn changed(a: &Palette, b: &Palette) -> Self {
        let mut e = Entries::default();
        for (i, (x, y)) in a.0.iter().zip(&b.0).enumerate() {
            if x != y {
                e.insert(i as u8);
            }
        }
        e
    }

    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }

    pub fn intersects(&self, o: &Entries) -> bool {
        self.0.iter().zip(&o.0).any(|(a, b)| a & b != 0)
    }

    pub fn contains(&self, i: u8) -> bool {
        self.0[i as usize / 64] >> (i % 64) & 1 == 1
    }

    /// Adds `o`'s entries.
    pub fn union(&mut self, o: &Entries) {
        for (a, b) in self.0.iter_mut().zip(&o.0) {
            *a |= b;
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Cycle {
    def: Def,
    countdown: u32,
    position: usize,
    on: bool,
}

/// The cycles and the current palette.
#[derive(Clone, Debug)]
pub struct Cycler {
    cycles: [Option<Cycle>; CYCLES],
    pal: Palette,
}

impl Cycler {
    /// A new game's cycles for an era over its base palette: the era's start-on set, and cycle 7 off.
    pub fn new(era: Era, base: &Palette) -> Self {
        Self::loaded(era, base, start_on(era))
    }

    /// The same with the on set (bit n for cycle n) taken from a saved game. Cycle 7 starts off.
    pub fn loaded(era: Era, base: &Palette, on: u8) -> Self {
        let mut cycles = [None; CYCLES];
        let mut set_up = |n: usize, def: Def, on: bool| {
            cycles[n] = Some(Cycle {
                def,
                countdown: def.period,
                position: 0,
                on,
            });
        };
        for (n, d) in defs(era).into_iter().enumerate() {
            set_up(n, d, on >> n & 1 == 1);
        }
        set_up(PLACEMENT_CYCLE, placement_def(), false);
        Cycler {
            cycles,
            pal: base.clone(),
        }
    }

    /// The current palette.
    pub fn palette(&self) -> &Palette {
        &self.pal
    }

    /// Which of the game's cycles are on, bit n for cycle n (the pulse is cycle 0; the placement cycle isn't part of
    /// a saved game).
    pub fn on_set(&self) -> u8 {
        (0..PLACEMENT_CYCLE).fold(0, |m, n| m | (self.cycles[n].is_some_and(|c| c.on) as u8) << n)
    }

    pub fn set_on(&mut self, n: usize, on: bool) {
        if let Some(c) = self.cycles[n].as_mut() {
            c.on = on;
        }
    }

    #[cfg(test)]
    pub fn is_on(&self, n: usize) -> bool {
        self.cycles[n].is_some_and(|c| c.on)
    }

    /// The entries the cycles in `mask` (bit n for cycle n) change.
    #[cfg(test)]
    pub fn entries_of(&self, mask: u8) -> Entries {
        let mut e = Entries::default();
        for (n, c) in self.cycles.iter().enumerate() {
            if let Some(c) = c.filter(|_| mask >> n & 1 == 1) {
                (c.def.first..=c.def.last).for_each(|i| e.insert(i));
            }
        }
        e
    }

    /// One step ("In each step"). Returns the cycles that turned, bit n for cycle n.
    pub fn step(&mut self) -> u8 {
        // The ready test looks before anything changes in this step.
        let ready = self.cycles.iter().flatten().any(|c| c.on && c.countdown <= 1);
        let mut turned = 0u8;
        for n in 0..CYCLES {
            let Some(c) = self.cycles[n].as_mut().filter(|c| c.on) else {
                continue;
            };
            c.countdown = c.countdown.saturating_sub(1);
            if c.countdown <= PULL_MARGIN && ready {
                c.countdown = c.def.period + AFTER_TURN;
                turned |= 1 << n;
                let Def {
                    first, last, colours, ..
                } = c.def;
                let (a, b) = (first as usize, last as usize);
                match colours {
                    Colours::Rotate => self.pal.0[a..=b].rotate_right(1),
                    Colours::List(list) => {
                        c.position = (c.position + 1) % list.len();
                        for k in 0..=(b - a) {
                            self.pal.0[a + k] = list[(c.position + k) % list.len()];
                        }
                    }
                }
            }
        }
        turned
    }
}

/// What lets cycling run ("Steps").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    /// The colour-cycling preference (on by default).
    pub preference: bool,
    pub paused: bool,
    pub menu_open: bool,
    pub focused: bool,
}

impl Gate {
    fn open(&self) -> bool {
        self.preference && !self.paused && !self.menu_open && self.focused
    }
}

/// The cycles, the step clock and the player's state that drives them.
#[derive(Clone, Debug)]
pub struct Cycling {
    cycler: Cycler,
    last_step: Option<u64>,
}

impl Cycling {
    pub fn new(era: Era, base: &Palette) -> Self {
        Cycling {
            cycler: Cycler::new(era, base),
            last_step: None,
        }
    }

    pub fn palette(&self) -> &Palette {
        self.cycler.palette()
    }

    pub fn cycler(&self) -> &Cycler {
        &self.cycler
    }

    /// A loaded game: the on set comes from the saved game (`Cycler::loaded`); the steps start counting again.
    pub fn loaded(era: Era, base: &Palette, on: u8) -> Self {
        Cycling {
            cycler: Cycler::loaded(era, base, on),
            last_step: None,
        }
    }

    /// A unit changed owner: the pulse stays on for the rest of the game ("Cycle 0, the pulse").
    pub fn owner_changed(&mut self) {
        self.cycler.set_on(PULSE_CYCLE, true);
    }

    /// Once per frame with the millisecond clock. Returns the cycles that turned (bit n for cycle n), 0 when no step
    /// happened.
    pub fn frame(&mut self, now_ms: u64, gate: Gate, placing: bool) -> u8 {
        self.cycler.set_on(PLACEMENT_CYCLE, placing);
        let Some(last) = self.last_step else {
            self.last_step = Some(now_ms);
            return 0;
        };
        if !gate.open() || now_ms.saturating_sub(last) < STEP_MS {
            return 0;
        }
        self.last_step = Some(now_ms);
        self.cycler.step()
    }
}

// --- fades ---------------------------------------------------------------------------------------------------------

/// A straight-line fade between two palettes over `total_ms` ("Screen fades").
#[derive(Clone, Debug)]
#[allow(dead_code)] // For the screen changes, which need app states.
pub struct Fade {
    pub start: Palette,
    pub end: Palette,
    pub total_ms: u32,
}

/// The least length of a fade frame in milliseconds.
#[allow(dead_code)]
pub const FADE_FRAME_MS: u32 = 16;

/// The fade lengths in milliseconds ("Screen fades" table).
#[allow(dead_code)]
pub mod fades {
    pub const GAME_START_IN: u32 = 10;
    pub const GAME_END_OUT: u32 = 1;
    pub const CREDITS: u32 = 100;
    pub const TITLE: u32 = 250;
    pub const TITLE_CUT_SHORT_OUT: u32 = 1;
    /// The act screen: the text fades in at countdown 60, the picture at 50, and the close fades out.
    pub const ACT_TEXT_IN: u32 = 300;
    pub const ACT_PICTURE_IN: u32 = 600;
    pub const ACT_CLOSE_OUT: u32 = 400;
    pub const MENUS: u32 = 200;
    /// A campaign chosen on the new-campaign menu: out, after a fade of all sound (round 74).
    pub const CAMPAIGN_CHOSEN_OUT: u32 = 200;
    /// The briefing: in as it opens, and out when it closes, after a fade of all sound (round 74).
    pub const BRIEFING: u32 = 100;
    /// The score screen closing: out, after a fade of all sound (round 74, static study).
    pub const SCORE_CLOSE_OUT: u32 = 150;
}

#[allow(dead_code)]
impl Fade {
    /// A fade-in from all black to `screen`.
    pub fn fade_in(screen: &Palette, total_ms: u32) -> Self {
        Fade {
            start: Palette([[0; 3]; 256]),
            end: screen.clone(),
            total_ms,
        }
    }

    /// A fade-out from `screen` to all black.
    pub fn fade_out(screen: &Palette, total_ms: u32) -> Self {
        Fade {
            start: screen.clone(),
            end: Palette([[0; 3]; 256]),
            total_ms,
        }
    }

    /// The palette of the frame at `t` ms after the fade began: start + floor-toward-zero(t × (end − start) / T).
    pub fn at(&self, t: u32) -> Palette {
        let t = t.min(self.total_ms) as i64;
        let total = self.total_ms.max(1) as i64;
        let mut p = self.start.clone();
        for (c, e) in p.0.iter_mut().zip(&self.end.0) {
            for k in 0..3 {
                let s = c[k] as i64;
                c[k] = (s + t * (e[k] as i64 - s) / total) as u8;
            }
        }
        p
    }

    /// The times t of the frames when each lasts `frame_ms` (at least [`FADE_FRAME_MS`]): 0, then a frame at a time,
    /// the last at exactly T.
    pub fn frame_times(&self, frame_ms: u32) -> Vec<u32> {
        let step = frame_ms.max(FADE_FRAME_MS);
        let mut out = vec![0];
        let mut t = 0;
        while t < self.total_ms {
            t = (t + step).min(self.total_ms);
            out.push(t);
        }
        out
    }
}

// --- greyscale while paused ---------------------------------------------------------------------------------------

/// A colour's grey level while paused (`ui/game-menu.md`): each term rounded down on its own.
pub fn grey_level(c: Rgb) -> u8 {
    let [r, g, b] = c.map(u32::from);
    (77 * r / 256 + 151 * g / 256 + 28 * b / 256) as u8
}

/// The whole palette in grey.
#[cfg(test)]
pub fn greyscale(pal: &Palette) -> Palette {
    Palette(pal.0.map(|c| [grey_level(c); 3]))
}

/// What the screen shows over the game's cycled palette `pal`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub paused: bool,
    /// The screenshot flash.
    pub negative: bool,
    /// A fade-in running: (ms since it began, its length).
    pub fade_in: Option<(u32, u32)>,
}

/// A colour as the screen shows it: the greyscale while paused, then the flash's negative, then the fade-in (the
/// fade's start + t × (end − start) / T with a black start, per channel).
pub fn shown_colour(c: Rgb, e: Effects) -> Rgb {
    let mut c = if e.paused { [grey_level(c); 3] } else { c };
    if e.negative {
        c = c.map(|v| 255 - v);
    }
    if let Some((t, total)) = e.fade_in {
        let (t, total) = (t.min(total), total.max(1));
        c = c.map(|v| (v as u32 * t / total) as u8);
    }
    c
}

/// The palette the screen shows over the cycled palette `pal`.
pub fn shown(pal: &Palette, e: Effects) -> Palette {
    Palette(pal.0.map(|c| shown_colour(c, e)))
}

impl Effects {
    pub fn any(&self) -> bool {
        *self != Effects::default()
    }
}

/// Puts the effects over an RGBA picture drawn with the cycled palette (the interface, which has colours of its own
/// besides palette entries): each pixel as [`shown_colour`] gives it. A clear pixel shows nowhere, so it is left as it
/// is.
pub fn apply(rgba: &mut [u8], e: Effects) {
    if !e.any() {
        return;
    }
    let t = channels(e);
    for px in rgba.chunks_exact_mut(4) {
        if px[3] != 0 {
            let c = shown_with(&t, e.paused, [px[0], px[1], px[2]]);
            px[..3].copy_from_slice(&c);
        }
    }
}

/// `src` with the effects put over it ([`apply`]) into `dst`, of the same length, in one pass.
pub fn apply_from(dst: &mut [u8], src: &[u8], e: Effects) {
    if !e.any() {
        dst.copy_from_slice(src);
        return;
    }
    let t = channels(e);
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let c = [s[0], s[1], s[2]];
        let c = if s[3] == 0 { c } else { shown_with(&t, e.paused, c) };
        d[..3].copy_from_slice(&c);
        d[3] = s[3];
    }
}

/// The effects that work channel by channel (the negative, the fade-in) as a table from a channel's value.
fn channels(e: Effects) -> [u8; 256] {
    let e = Effects { paused: false, ..e };
    std::array::from_fn(|v| shown_colour([v as u8; 3], e)[0])
}

/// [`shown_colour`] with its channel steps from [`channels`].
#[inline(always)]
fn shown_with(t: &[u8; 256], paused: bool, c: Rgb) -> Rgb {
    if paused {
        [t[grey_level(c) as usize]; 3]
    } else {
        c.map(|v| t[v as usize])
    }
}

// --- screenshot flash ----------------------------------------------------------------------------------------------

/// The palette shown in negative while a screenshot is saved: each channel v becomes 255 − v.
/// (`shown_colour` applies it per colour; this whole-palette form is for the tests.)
#[cfg(test)]
pub fn negative(pal: &Palette) -> Palette {
    Palette(pal.0.map(|c| c.map(|v| 255 - v)))
}

/// The first of `screen000.pcx` to `screen999.pcx` for which `exists` is false; `None` if all 1000 exist.
pub fn screenshot_name(exists: impl Fn(&str) -> bool) -> Option<String> {
    (0..1000)
        .map(|n| format!("screen{n:03}.pcx"))
        .find(|name| !exists(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Palette {
        let mut p = Palette([[0; 3]; 256]);
        for (i, c) in p.0.iter_mut().enumerate() {
            *c = [i as u8, (i / 2) as u8, 7];
        }
        p.0[3] = [0, 0, 255];
        p
    }

    /// Runs `n` steps from step 1 and lists (step, cycles that turned) for the steps where something turned.
    fn turns(c: &mut Cycler, n: u32) -> Vec<(u32, Vec<usize>)> {
        (1..=n)
            .filter_map(|s| {
                let m = c.step();
                (m != 0).then(|| (s, (0..CYCLES).filter(|&i| m >> i & 1 == 1).collect()))
            })
            .collect()
    }

    fn expect(v: &[(u32, &[usize])]) -> Vec<(u32, Vec<usize>)> {
        v.iter().map(|&(s, c)| (s, c.to_vec())).collect()
    }

    #[test]
    fn a_single_cycle_turns_at_p_then_every_p_plus_1() {
        for (period, want) in [(8, vec![8, 17, 26]), (1, vec![1, 3, 5])] {
            let mut c = Cycler::new(Era::Forest, &base());
            for n in 0..CYCLES {
                c.set_on(n, false);
            }
            let one = c.cycles[1].as_mut().unwrap();
            one.def.period = period;
            one.countdown = period;
            c.set_on(1, true);
            let got: Vec<u32> = turns(&mut c, 26).into_iter().map(|t| t.0).collect();
            assert_eq!(got[..3], want[..]);
        }
    }

    #[test]
    fn forest_defaults() {
        let mut c = Cycler::new(Era::Forest, &base());
        let want = expect(&[
            (8, &[1, 4]),
            (12, &[2]),
            (17, &[1, 4]),
            (20, &[3]),
            (25, &[1, 2, 4]),
            (34, &[1, 4]),
            (38, &[2]),
            (41, &[1, 3, 4]),
            (50, &[1, 2, 4]),
            (59, &[1, 4]),
            (62, &[2, 3]),
            (68, &[1, 4]),
        ]);
        assert_eq!(turns(&mut c, 68), want);
    }

    #[test]
    fn forest_while_placing_from_the_start() {
        let mut c = Cycler::new(Era::Forest, &base());
        c.set_on(PLACEMENT_CYCLE, true);
        let got = turns(&mut c, 39);
        let sevens: Vec<u32> = got.iter().filter(|t| t.1.contains(&7)).map(|t| t.0).collect();
        assert_eq!(sevens, (1..=39).step_by(2).collect::<Vec<_>>());
        let others: Vec<(u32, Vec<usize>)> = got
            .into_iter()
            .map(|(s, v)| (s, v.into_iter().filter(|&n| n != 7).collect::<Vec<_>>()))
            .filter(|t| !t.1.is_empty())
            .collect();
        let want = expect(&[
            (7, &[1, 4]),
            (11, &[2]),
            (15, &[1, 4]),
            (19, &[3]),
            (23, &[1, 2, 4]),
            (31, &[1, 4]),
            (35, &[2]),
            (39, &[1, 3, 4]),
        ]);
        assert_eq!(others, want);
    }

    #[test]
    fn winter_with_the_pulse_on() {
        let mut c = Cycler::loaded(Era::Winter, &base(), start_on(Era::Winter) | 1);
        let want = expect(&[
            (4, &[0]),
            (8, &[0, 1, 5]),
            (12, &[0, 2]),
            (17, &[0, 1, 5]),
            (20, &[0, 3, 4]),
            (25, &[0, 2, 5]),
            (28, &[0, 1]),
            (33, &[0, 5]),
            (38, &[0, 1, 2]),
            (41, &[0, 3, 4, 5]),
        ]);
        assert_eq!(turns(&mut c, 41), want);
    }

    #[test]
    fn the_on_set_reads_back_the_cycles_that_are_on() {
        let mut c = Cycler::new(Era::Forest, &base());
        assert_eq!(c.on_set(), start_on(Era::Forest));
        c.set_on(0, true);
        c.set_on(1, false);
        assert_eq!(c.on_set() & 0b11, 0b01, "the pulse is bit 0");
        assert_eq!(
            c.on_set() >> PLACEMENT_CYCLE,
            0,
            "the placement cycle isn't part of a save"
        );
    }

    #[test]
    fn the_start_on_sets() {
        assert_eq!(start_on(Era::Forest), 0b11110);
        assert_eq!(start_on(Era::Winter), 0b111110);
        assert_eq!(start_on(Era::Wasteland), 0b11110);
        assert_eq!(start_on(Era::Swamp), 0b111110);
        assert!(!Cycler::new(Era::Forest, &base()).is_on(PLACEMENT_CYCLE));
    }

    #[test]
    fn a_rotation_moves_colours_towards_higher_entries() {
        let b = base();
        let mut c = Cycler::new(Era::Forest, &b);
        let mut m = 0;
        while m & 2 == 0 {
            m = c.step();
        }
        let p = c.palette();
        assert_eq!(p.0[38], b.0[47]);
        assert_eq!(p.0[39], b.0[38]);
        assert_eq!(p.0[47], b.0[46]);
        assert_eq!(p.0[37], b.0[37]);
    }

    #[test]
    fn the_attack_mark_alternates_green_and_red_from_blue() {
        let mut c = Cycler::new(Era::Forest, &base());
        assert_eq!(c.palette().0[MARK_ENTRY as usize], [0, 0, 255]);
        let mut seen = vec![];
        for _ in 0..40 {
            if c.step() & 0b10000 != 0 {
                seen.push(c.palette().0[MARK_ENTRY as usize]);
            }
        }
        assert_eq!(seen, [[0, 255, 0], [255, 0, 0], [0, 255, 0], [255, 0, 0]]);
    }

    fn gate() -> Gate {
        Gate {
            preference: true,
            paused: false,
            menu_open: false,
            focused: true,
        }
    }

    #[test]
    fn the_pulse_goes_through_its_colours_after_a_rescue() {
        let mut cy = Cycling::new(Era::Forest, &base());
        cy.frame(0, gate(), false);
        cy.owner_changed();
        let mut seen = vec![];
        for s in 1..=400u64 {
            if cy.frame(s * 20, gate(), false) & 1 != 0 {
                seen.push(cy.palette().0[202][0]);
            }
        }
        assert_eq!(seen[..11], [223, 190, 158, 125, 150, 174, 203, 227, 255, 255, 223]);
    }

    #[test]
    fn the_swamp_pulse_uses_entry_4() {
        let mut c = Cycler::new(Era::Swamp, &base());
        c.set_on(0, true);
        turns(&mut c, 4);
        assert_eq!(c.palette().0[4], [223, 0, 0]);
    }

    #[test]
    fn the_placement_shades_go_up_and_down() {
        assert_eq!(PLACEMENT[0], [77, 60, 60]);
        assert_eq!(PLACEMENT[15], [255, 0, 0]);
        assert_eq!(PLACEMENT[16], [255, 0, 0]);
        assert_eq!(PLACEMENT[31], [77, 60, 60]);
        let mut c = Cycler::new(Era::Forest, &base());
        c.set_on(PLACEMENT_CYCLE, true);
        c.step();
        assert_eq!(c.palette().0[10], PLACEMENT[1]);
    }

    #[test]
    fn placing_follows_the_player_and_steps_are_gated() {
        let mut cy = Cycling::new(Era::Forest, &base());
        let steps = |cy: &mut Cycling, t: u64, g: Gate| {
            let before = cy.cycler.cycles[4].unwrap().countdown;
            cy.frame(t, g, false);
            before != cy.cycler.cycles[4].unwrap().countdown
        };
        assert!(!steps(&mut cy, 1000, gate()), "the first frame only sets the clock");
        assert!(!steps(&mut cy, 1019, gate()));
        assert!(steps(&mut cy, 1020, gate()));
        assert!(!steps(&mut cy, 1030, gate()));
        for g in [
            Gate {
                preference: false,
                ..gate()
            },
            Gate { paused: true, ..gate() },
            Gate {
                menu_open: true,
                ..gate()
            },
            Gate {
                focused: false,
                ..gate()
            },
        ] {
            assert!(!steps(&mut cy, 11_000, g), "{g:?}");
        }
        // After 10 s paused the cycles carry on from where they were.
        let held = cy.cycler.cycles[4].unwrap().countdown;
        assert!(steps(&mut cy, 11_010, gate()));
        assert_eq!(cy.cycler.cycles[4].unwrap().countdown, held - 1);
        assert!(!cy.cycler.is_on(PLACEMENT_CYCLE));
        cy.frame(11_040, gate(), true);
        assert!(cy.cycler.is_on(PLACEMENT_CYCLE));
        cy.frame(11_050, gate(), false);
        assert!(!cy.cycler.is_on(PLACEMENT_CYCLE));
    }

    #[test]
    fn a_fade_in_over_200_ms() {
        let mut screen = Palette([[0; 3]; 256]);
        screen.0[7] = [200, 100, 0];
        let f = Fade::fade_in(&screen, 200);
        let times = f.frame_times(16);
        assert_eq!(times.len(), 14);
        assert_eq!(times[..3], [0, 16, 32]);
        assert_eq!(times[12..], [192, 200]);
        assert_eq!(f.at(96).0[7], [96, 48, 0]);
        assert_eq!(f.at(0).0[7], [0, 0, 0]);
        assert_eq!(f.at(200).0[7], [200, 100, 0]);
        // A fade-out truncates toward zero too: 200 − 96 = 104 at t = 96.
        let out = Fade::fade_out(&screen, 200);
        assert_eq!(out.at(96).0[7][0], 104);
        // Frames shorter than 16 ms take 16.
        assert_eq!(f.frame_times(5), times);
        assert_eq!(Fade::fade_in(&screen, 1).frame_times(16), [0, 1]);
    }

    #[test]
    fn the_flash_and_file_names() {
        let n = negative(&base());
        assert_eq!(n.0[3], [255, 255, 0]);
        let have = ["screen000.pcx", "screen001.pcx"];
        assert_eq!(screenshot_name(|f| have.contains(&f)).as_deref(), Some("screen002.pcx"));
        assert_eq!(screenshot_name(|_| true), None);
    }

    #[test]
    fn entries_tell_which_pictures_a_turn_changes() {
        let c = Cycler::new(Era::Forest, &base());
        let water = Entries::of_pixels(&[1, 40, 200]);
        let land = Entries::of_pixels(&[1, 200]);
        assert!(water.intersects(&c.entries_of(0b10)));
        assert!(!land.intersects(&c.entries_of(0b11110)));
        assert_eq!(c.entries_of(0), Entries::default());
    }

    #[test]
    fn greyscale_rounds_each_term_down() {
        assert_eq!(grey_level([255, 0, 0]), 76);
        assert_eq!(grey_level([255, 255, 255]), 76 + 150 + 27);
        let g = greyscale(&base());
        assert_eq!(g.0[3], [27; 3], "pure blue: floor(28 * 255 / 256)");
        assert!(g.0.iter().all(|c| c[0] == c[1] && c[1] == c[2]));
    }

    #[test]
    fn the_shown_palette_puts_the_effects_together() {
        let b = base();
        assert_eq!(shown(&b, Effects::default()), b);
        assert_eq!(
            shown(
                &b,
                Effects {
                    paused: true,
                    ..Effects::default()
                }
            ),
            greyscale(&b)
        );
        let neg = shown(
            &b,
            Effects {
                negative: true,
                ..Effects::default()
            },
        );
        assert_eq!(neg.0[3], [255, 255, 0]);
        let start = Effects {
            fade_in: Some((0, 10)),
            ..Effects::default()
        };
        assert!(shown(&b, start).0.iter().all(|c| *c == [0; 3]), "t = 0 is black");
        let end = Effects {
            fade_in: Some((10, 10)),
            ..Effects::default()
        };
        assert_eq!(shown(&b, end), b);
        // `apply` and `apply_from` give each pixel `shown_colour`'s colour under every mix of effects.
        let pixels: Vec<u8> = (0..=255u8).flat_map(|v| [v, v.wrapping_mul(7), 255 - v, 255]).collect();
        for paused in [false, true] {
            for negative in [false, true] {
                for fade_in in [None, Some((96, 200))] {
                    let e = Effects {
                        paused,
                        negative,
                        fade_in,
                    };
                    let mut a = pixels.clone();
                    apply(&mut a, e);
                    let mut b = vec![0; pixels.len()];
                    apply_from(&mut b, &pixels, e);
                    for (px, s) in a.chunks_exact(4).zip(pixels.chunks_exact(4)) {
                        assert_eq!(px[..3], shown_colour([s[0], s[1], s[2]], e), "{e:?}");
                    }
                    assert_eq!(a, b, "{e:?}");
                }
            }
        }
        // The interface's own colours get the same treatment as palette entries; a clear pixel is left.
        let mut px = [255, 0, 0, 255, 10, 20, 30, 0];
        apply(
            &mut px,
            Effects {
                paused: true,
                ..Effects::default()
            },
        );
        assert_eq!(px, [76, 76, 76, 255, 10, 20, 30, 0]);
        // The fade agrees with `Fade::at`.
        let half = Effects {
            fade_in: Some((96, 200)),
            ..Effects::default()
        };
        assert_eq!(shown(&b, half), Fade::fade_in(&b, 200).at(96));
        // Resuming puts back the current palette: only the entries that differ need redrawing.
        let e = Entries::changed(&b, &greyscale(&b));
        assert!(!e.is_empty() && Entries::changed(&b, &b).is_empty());
    }
}
