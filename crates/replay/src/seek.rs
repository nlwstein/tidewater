//! Jumping about in a replay: the viewer's timeline (`docs/design/multiplayer-determinism.md`, "The replay viewer").
//!
//! A replay is the start plus the commands of each cycle, so a cycle can be reached only by playing up to it. To make a
//! jump back cheap the [`Seeker`] keeps a copy of the world every [`KEYFRAME_EVERY`] cycles as the replay plays (a
//! [`Keyframes`] store that thins itself, so a long game never holds more than [`KEYFRAME_CAP`] copies); a jump back
//! restores the nearest copy before the target and plays forward from it, a jump ahead just plays forward. Playing
//! forward here is the plain `World::step`, so every checkpoint the recording holds on the way is compared as it is
//! reached: a seek can never land on a state the recording would not have.
//!
//! The viewer's vision is a display matter and never reaches the hash: [`Vision::Everyone`] only sets
//! `World::observing`, which is what a network observer's game uses, and [`Vision::Player`] makes the world's
//! `vision.local` another player's. The fog maps are the local player's alone and grow as the game goes on, so a new
//! local player means playing the game again from the start; the copies are dropped and the jump to where the viewer
//! was is made from the origin.
//!
//! Seeking is for replay playback only: nothing here is reachable from a live game, and with the seeker unused a
//! replay plays exactly as it did.

use tidewater_sim::World;
use tidewater_sim::ai::controller;
use tidewater_sim::cheats::Flags;
use tidewater_sim::timing::{CLASSIC_MS, CLASSIC_ROW, enhanced_ms};
use tidewater_sim::Command;

use crate::format::Replay;
use crate::run::Mismatch;

/// Cycles between two copies of the world while the replay plays. A jump back replays at most this many cycles (about
/// a twelfth of a second of computing per 1,000), and the copies of a half-hour game stay under the cap.
pub const KEYFRAME_EVERY: u32 = 500;

/// The most copies kept. When they are full every second one is dropped and the spacing doubles, so the cost of a jump
/// back grows with the length of the game but the memory does not. Must be even.
pub const KEYFRAME_CAP: usize = 64;

/// Whose view the viewer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vision {
    /// The fog of war and the units as that player has them.
    Player(u8),
    /// The whole map with no fog, every unit shown (a replay is not a live game, so this costs nothing).
    Everyone,
}

/// Copies of something taken every `every` cycles, kept to a fixed count: the first at `every`, then at each multiple
/// after it, with no gaps. (Cycle 0 is not kept; the seeker rebuilds that from the origin.)
#[derive(Clone, Debug)]
pub struct Keyframes<T> {
    every: u32,
    cap: usize,
    frames: Vec<(u32, T)>,
}

impl<T> Keyframes<T> {
    pub fn new(every: u32, cap: usize) -> Self {
        assert!(every > 0 && cap >= 2 && cap.is_multiple_of(2), "a spacing and an even count");
        Keyframes {
            every,
            cap,
            frames: Vec::new(),
        }
    }

    /// The spacing between copies now.
    pub fn every(&self) -> u32 {
        self.every
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The cycles that are kept, in order.
    pub fn ticks(&self) -> impl Iterator<Item = u32> + '_ {
        self.frames.iter().map(|f| f.0)
    }

    /// Forgets every copy; the next one is at `every` again.
    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// Keeps a copy for cycle `tick` if it is the next one due, making it only then. A cycle already behind the last
    /// copy (the replay was wound back and is playing through it again) is not kept twice. Returns whether it was kept.
    pub fn offer(&mut self, tick: u32, make: impl FnOnce() -> T) -> bool {
        let next = self.frames.last().map_or(self.every, |f| f.0 + self.every);
        if tick != next {
            return false;
        }
        if self.frames.len() == self.cap {
            // Keep the copies at the even multiples (the 2nd, 4th ...) and double the spacing.
            let mut keep = false;
            self.frames.retain(|_| {
                keep = !keep;
                !keep
            });
            self.every *= 2;
            if !tick.is_multiple_of(self.every) {
                return false;
            }
        }
        self.frames.push((tick, make()));
        true
    }

    /// The latest copy at or before `tick`.
    pub fn at_or_before(&self, tick: u32) -> Option<(u32, &T)> {
        let i = self.frames.partition_point(|f| f.0 <= tick);
        self.frames.get(i.checked_sub(1)?).map(|f| (f.0, &f.1))
    }
}

/// Where a jump starts playing from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The world as it is: the target is ahead and no copy is any nearer.
    Continue,
    /// The copy at this cycle.
    Keyframe(u32),
    /// The game's start: a jump back with no copy before it.
    Origin,
}

/// Where to start a jump from `now` to `target`, given the latest copy at or before the target (`best`).
pub fn plan(now: u32, target: u32, best: Option<u32>) -> Source {
    if target < now {
        return best.map_or(Source::Origin, Source::Keyframe);
    }
    match best {
        Some(b) if b > now => Source::Keyframe(b),
        _ => Source::Continue,
    }
}

/// What a call to [`Seeker::work`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// No jump is under way.
    Idle,
    /// Part of a jump is done.
    Running,
    /// The jump has reached its cycle.
    Arrived,
}

/// The length of a cycle in milliseconds at a game speed (the pacing the timeline's clock counts in).
fn cycle_ms(enhanced: bool, speed: u8) -> f64 {
    if enhanced {
        f64::from(enhanced_ms(speed))
    } else {
        let row = &CLASSIC_MS[CLASSIC_ROW[usize::from(speed).min(8)]];
        f64::from(row.iter().sum::<u32>()) / 3.0
    }
}

/// The time a recording has taken at each cycle: the cycles' lengths at the game speed as the recorded speed commands
/// set it. Two cycles in a row at the same speed are one run.
#[derive(Clone, Debug)]
struct Clock {
    enhanced: bool,
    /// (first cycle of the run, milliseconds at its start, speed in it)
    runs: Vec<(u32, f64, u8)>,
}

impl Clock {
    fn new(replay: &Replay, origin: &World) -> Clock {
        let enhanced = origin.rules.enhanced;
        let top = origin.rules.extras.top_speed(enhanced);
        let mut c = Clock {
            enhanced,
            runs: vec![(0, 0.0, origin.speed)],
        };
        for cc in &replay.commands {
            for cmd in &cc.commands {
                if let Command::Speed { speed, .. } = *cmd
                    && speed <= top
                {
                    let &(from, ms, now) = c.runs.last().expect("one run to start with");
                    if speed == now {
                        continue;
                    }
                    let at = ms + f64::from(cc.cycle - from) * cycle_ms(enhanced, now);
                    c.runs.push((cc.cycle, at, speed));
                }
            }
        }
        c
    }

    fn ms_at(&self, tick: u32) -> u64 {
        let i = self.runs.partition_point(|r| r.0 <= tick).max(1) - 1;
        let (from, ms, speed) = self.runs[i];
        (ms + f64::from(tick - from) * cycle_ms(self.enhanced, speed)) as u64
    }
}

/// The last cycle up to `total` whose time `ms_at` gives is at most `ms` (the times never go down).
fn last_at_or_before(total: u32, ms_at: impl Fn(u32) -> u64, ms: u64) -> u32 {
    let (mut lo, mut hi) = (0, total);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if ms_at(mid) <= ms {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// The viewer's way about a replay: the copies of the world, the pending jump and the vision.
pub struct Seeker {
    origin: World,
    start_cheats: u32,
    keyframes: Keyframes<World>,
    target: Option<u32>,
    total: u32,
    local: u8,
    everyone: bool,
    players: Vec<u8>,
    clock: Clock,
}

impl Seeker {
    /// A seeker for `replay`, whose world was built from `origin`: the world as [`crate::start::origin_world`] makes
    /// it, before the start-up cycles. The viewer starts in the recording player's own view.
    pub fn new(origin: World, replay: &Replay) -> Seeker {
        let players = origin
            .players
            .iter()
            .take(8)
            .enumerate()
            .filter(|(_, p)| p.controller == controller::COMPUTER || p.controller == controller::HUMAN)
            .map(|(i, _)| i as u8)
            .collect();
        Seeker {
            start_cheats: replay.header.start_cheats,
            keyframes: Keyframes::new(KEYFRAME_EVERY, KEYFRAME_CAP),
            target: None,
            total: replay.cycles,
            local: origin.vision.local,
            everyone: false,
            players,
            clock: Clock::new(replay, &origin),
            origin,
        }
    }

    /// The recording's length in cycles.
    pub fn total(&self) -> u32 {
        self.total
    }

    /// The players a viewer can take the view of: the computer and human players of the map.
    pub fn players(&self) -> &[u8] {
        &self.players
    }

    /// The vision now.
    pub fn vision(&self) -> Vision {
        if self.everyone {
            Vision::Everyone
        } else {
            Vision::Player(self.local)
        }
    }

    /// The vision after this one when the key that cycles it is pressed: each player in turn, then everyone.
    pub fn next_vision(&self) -> Vision {
        let at = self.players.iter().position(|&p| p == self.local);
        match (self.everyone, at) {
            (false, Some(i)) if i + 1 < self.players.len() => Vision::Player(self.players[i + 1]),
            (false, _) => Vision::Everyone,
            (true, _) => self.players.first().map_or(Vision::Everyone, |&p| Vision::Player(p)),
        }
    }

    /// The player whose maps the world carries.
    pub fn local(&self) -> u8 {
        self.local
    }

    /// The cycle a jump is heading for, if one is under way.
    pub fn target(&self) -> Option<u32> {
        self.target
    }

    /// How many copies of the world are held.
    pub fn keyframes(&self) -> &Keyframes<World> {
        &self.keyframes
    }

    /// The time at cycle `tick`, in milliseconds of the recorded game's speeds.
    pub fn ms_at(&self, tick: u32) -> u64 {
        self.clock.ms_at(tick)
    }

    /// The last cycle whose time is at most `ms` (a jump of so many seconds from a time), at most the end.
    pub fn tick_at_ms(&self, ms: u64) -> u32 {
        last_at_or_before(self.total, |t| self.clock.ms_at(t), ms)
    }

    /// Called after each cycle that ran: keeps a copy of `world` when one is due.
    pub fn note(&mut self, world: &World) {
        self.keyframes.offer(world.tick, || world.clone());
    }

    /// Asks for cycle `target` (past the end counts as the end). The world is put at the nearest place to play on from
    /// at once; [`Seeker::work`] then plays it forward.
    pub fn seek(&mut self, world: &mut World, target: u32) {
        let target = target.min(self.total);
        let best = self.keyframes.at_or_before(target).map(|f| f.0);
        match plan(world.tick, target, best) {
            Source::Continue => {}
            Source::Keyframe(t) => self.restore(world, t),
            Source::Origin => self.rebuild(world),
        }
        self.target = (world.tick != target).then_some(target);
    }

    /// Changes the vision. Everyone, or a player whose maps the world already carries, is the flag alone; another
    /// player's view plays the game again from the start to where the viewer was (or is heading).
    pub fn set_vision(&mut self, world: &mut World, vision: Vision) {
        match vision {
            Vision::Everyone => {
                self.everyone = true;
                world.observing = true;
            }
            Vision::Player(p) => {
                self.everyone = false;
                world.observing = false;
                if p != self.local {
                    let back_to = self.target.unwrap_or(world.tick);
                    self.local = p;
                    self.keyframes.clear();
                    self.rebuild(world);
                    self.target = (back_to > 0).then_some(back_to);
                }
            }
        }
    }

    /// Plays the pending jump forward by at most `budget` cycles, comparing each checkpoint the recording has on the
    /// way through `check` (the recorded hash is compared by the caller's session, so that a mismatch stops it as it
    /// does in normal playback). The cycles' events and sounds are dropped: a jump is silent. A mismatch ends the jump
    /// where it is.
    pub fn work(
        &mut self,
        world: &mut World,
        replay: &Replay,
        budget: u32,
        mut check: impl FnMut(u32, u64) -> Result<(), Mismatch>,
    ) -> Result<Progress, Mismatch> {
        let Some(target) = self.target else {
            return Ok(Progress::Idle);
        };
        for _ in 0..budget {
            if world.tick >= target {
                break;
            }
            world.step(replay.commands_at(world.tick));
            quiet(world);
            self.note(world);
            if replay.checkpoint_at(world.tick).is_some()
                && let Err(m) = check(world.tick, world.state_hash())
            {
                self.target = None;
                return Err(m);
            }
        }
        if world.tick >= target {
            self.target = None;
            Ok(Progress::Arrived)
        } else {
            Ok(Progress::Running)
        }
    }

    /// The world at the game's start as the viewer's vision has it: the origin through the start-up cycles and the
    /// cheat flags the game began with (what `start::new_world` does).
    fn rebuild(&self, world: &mut World) {
        let mut w = self.origin.clone();
        w.vision.local = self.local;
        w.observing = self.everyone;
        w.start_up();
        if self.start_cheats != 0 {
            w.start_cheats(Flags(self.start_cheats));
        }
        *world = w;
    }

    fn restore(&self, world: &mut World, tick: u32) {
        let (_, kf) = self
            .keyframes
            .at_or_before(tick)
            .expect("the copy was found a moment ago");
        *world = kf.clone();
        quiet(world);
        world.observing = self.everyone;
    }
}

/// Drops what a cycle queued for the screen and the speakers.
fn quiet(world: &mut World) {
    world.events.clear();
    world.sounds.cues.clear();
    world.sounds.events.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(k: &mut Keyframes<u32>, to: u32) {
        for t in 1..=to {
            k.offer(t, || t);
        }
    }

    #[test]
    fn copies_are_kept_at_each_multiple_and_found_by_the_latest_at_or_before() {
        let mut k = Keyframes::new(10, 4);
        fill(&mut k, 35);
        assert_eq!(k.ticks().collect::<Vec<_>>(), [10, 20, 30]);
        assert_eq!(k.at_or_before(9), None, "cycle 0 is the origin's");
        assert_eq!(k.at_or_before(10).map(|f| f.0), Some(10));
        assert_eq!(k.at_or_before(29).map(|f| f.0), Some(20));
        assert_eq!(k.at_or_before(1000).map(|f| f.0), Some(30));
    }

    #[test]
    fn a_copy_is_made_only_when_it_is_kept() {
        let mut k = Keyframes::new(10, 4);
        let mut made = 0;
        for t in 1..=25 {
            k.offer(t, || {
                made += 1;
                t
            });
        }
        assert_eq!(made, 2);
        // Playing through cycle 20 again after a wind back keeps nothing new.
        assert!(!k.offer(20, || unreachable!()));
        assert!(!k.offer(10, || unreachable!()));
    }

    #[test]
    fn full_copies_thin_to_every_second_one_and_the_spacing_doubles() {
        let mut k = Keyframes::new(10, 4);
        fill(&mut k, 1000);
        // The store never holds more than its cap, whatever the length of the game ...
        assert!(k.len() <= 4);
        // ... every copy is at a multiple of the spacing, with none missing between the first and the last ...
        let e = k.every();
        let ticks: Vec<u32> = k.ticks().collect();
        assert!(ticks.iter().all(|t| t % e == 0), "{ticks:?} at spacing {e}");
        assert!(ticks.windows(2).all(|w| w[1] - w[0] == e), "{ticks:?}");
        // ... and the latest copy is recent: within a few spacings of the end.
        assert!(1000 - ticks.last().unwrap() <= 2 * e, "{ticks:?}");
        assert!(e > 10);
    }

    #[test]
    fn a_long_game_stays_under_the_cap_with_a_jump_back_that_is_a_few_thousand_cycles_at_worst() {
        let mut k = Keyframes::new(KEYFRAME_EVERY, KEYFRAME_CAP);
        for t in (KEYFRAME_EVERY..=200_000).step_by(KEYFRAME_EVERY as usize) {
            k.offer(t, || t);
        }
        assert!(k.len() <= KEYFRAME_CAP);
        let worst = (0..=200_000).map(|t| t - k.at_or_before(t).map_or(0, |f| f.0)).max().unwrap();
        assert!(worst <= 8_000, "{worst}");
    }

    #[test]
    fn the_clock_counts_each_run_of_cycles_at_the_speed_it_was_played_at() {
        let mut c = Clock {
            enhanced: true,
            runs: vec![(0, 0.0, 4)],
        };
        let slow = cycle_ms(true, 4);
        let fast = cycle_ms(true, 6);
        c.runs.push((100, 100.0 * slow, 6));
        assert_eq!(c.ms_at(0), 0);
        assert_eq!(c.ms_at(100), (100.0 * slow) as u64);
        assert_eq!(c.ms_at(150), (100.0 * slow + 50.0 * fast) as u64);
        assert!(c.ms_at(150) > c.ms_at(149));
    }

    #[test]
    fn a_time_maps_back_to_the_last_cycle_at_or_before_it() {
        let clock = Clock {
            enhanced: true,
            runs: vec![(0, 0.0, 4)],
        };
        let find = |ms| last_at_or_before(1000, |t| clock.ms_at(t), ms);
        assert_eq!(find(0), 0);
        assert_eq!(find(clock.ms_at(10)), 10);
        assert_eq!(find((10.5 * cycle_ms(true, 4)) as u64), 10);
        assert_eq!(find(u64::MAX), 1000);
    }

    #[test]
    fn a_jump_plays_on_from_the_nearest_place() {
        // Ahead of where we are, with no nearer copy: just play.
        assert_eq!(plan(100, 400, None), Source::Continue);
        assert_eq!(plan(100, 400, Some(50)), Source::Continue);
        assert_eq!(plan(100, 100, Some(100)), Source::Continue);
        // Ahead, with a copy nearer than where we are (we were further on before): jump to it.
        assert_eq!(plan(100, 900, Some(500)), Source::Keyframe(500));
        // Back: the latest copy before the target, or the origin when there is none.
        assert_eq!(plan(900, 700, Some(500)), Source::Keyframe(500));
        assert_eq!(plan(900, 700, Some(700)), Source::Keyframe(700));
        assert_eq!(plan(900, 300, None), Source::Origin);
        assert_eq!(plan(900, 0, None), Source::Origin);
    }
}
