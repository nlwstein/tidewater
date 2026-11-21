//! The income rates (option `progress-income`, `extras`, `docs/design/extras.md`): the gold, lumber and oil the local
//! player delivered over the last game minute, shown beside the resource counters. Our own design, not the original's:
//! the original shows only the amounts you hold, and the totals gathered only on the score screen.
//!
//! **What is counted.** The player's own "total gathered" statistic (`Player::gathered`, the figures the score screen
//! shows), which only rises when a worker delivers. Spending never lowers it, so the rate is what the economy brings
//! in, not the change in the balance. Nothing of any other player is read.
//!
//! **The window.** A game minute at the game's current speed, counted in cycles so a pause (no cycles) leaves the rate
//! alone and a faster game speed shortens the window in real time as it should. The [`Meter`] keeps one reading about
//! every second of that window and compares the newest with the oldest still inside it. Deliveries come in lumps (a
//! worker brings 100 gold at a time), so the figure steps; the full minute smooths it.
//!
//! **Warm-up.** Until a sixth of the window has passed (ten seconds) there is too little to divide by and the rate
//! shows as `-`.

use std::collections::VecDeque;

/// Gold, lumber and oil.
pub type Amounts = [u32; 3];

/// The window shows a rate once it spans at least this share (1 in 6) of a minute.
const WARM_UP_DIVISOR: u32 = 6;
/// Rows from the map view's top edge to the income row, and, when the control-group bar is on, to the row under its
/// buttons (`groupbar`: 4 rows in, [`crate::groupbar::CELL_H`] high, then 2 clear).
pub fn row_offset(below_groups: bool) -> i32 {
    if below_groups {
        crate::groupbar::MARGIN + crate::groupbar::CELL_H + 2
    } else {
        2
    }
}

/// Readings kept at most (a safety bound: one a second over a minute is 60).
const MAX_READINGS: usize = 256;

/// A rolling record of what was gathered, and the rates worked out from it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meter {
    /// (cycle, gathered so far), oldest first.
    readings: VecDeque<(u32, Amounts)>,
    /// The rates per game minute from the last reading, `None` while warming up.
    rates: Option<Amounts>,
}

impl Meter {
    /// Takes the reading for `cycle` (`gathered` so far) with `minute` the cycles in a game minute now.
    pub fn update(&mut self, cycle: u32, gathered: Amounts, minute: u32) {
        let minute = minute.max(1);
        // A cycle number that went back (a game loaded over this one) starts the record again.
        if self.readings.back().is_some_and(|&(c, _)| cycle < c) {
            self.readings.clear();
        }
        // About one reading a second.
        let gap = (minute / 60).max(1);
        match self.readings.back_mut() {
            Some(&mut (c, _)) if cycle == c => return,
            Some(&mut (c, _)) if cycle - c < gap => return,
            _ => {}
        }
        self.readings.push_back((cycle, gathered));
        // Keep one reading at or before the window's start; drop the older ones.
        while self.readings.len() > 2 && self.readings[1].0 + minute <= cycle {
            self.readings.pop_front();
        }
        while self.readings.len() > MAX_READINGS {
            self.readings.pop_front();
        }
        self.rates = self.work_out(minute);
    }

    fn work_out(&self, minute: u32) -> Option<Amounts> {
        let (&(old, was), &(new, now)) = (self.readings.front()?, self.readings.back()?);
        let span = new - old;
        if span == 0 || span < minute / WARM_UP_DIVISOR {
            return None;
        }
        let per_minute = |i: usize| {
            let got = u64::from(now[i].saturating_sub(was[i]));
            ((got * u64::from(minute) + u64::from(span) / 2) / u64::from(span)).min(u64::from(u32::MAX)) as u32
        };
        Some([per_minute(0), per_minute(1), per_minute(2)])
    }

    /// The rates per game minute as of the last reading: gold, lumber, oil. `None` while the record is too short.
    pub fn rates(&self) -> Option<Amounts> {
        self.rates
    }
}

/// A rate as the resource bar writes it: `+85`, `+1.2k` from 1,000, `+12k` from 10,000, `-` with no rate yet.
pub fn rate_text(rate: Option<u32>) -> String {
    match rate {
        None => "-".to_string(),
        Some(r) if r < 1000 => format!("+{r}"),
        Some(r) if r < 10_000 => format!("+{}.{}k", r / 1000, r % 1000 / 100),
        Some(r) => format!("+{}k", r / 1000),
    }
}

/// A rate as the income row writes it under a counter: `+85/min`, `+1.2k/min`, or `-` with no rate yet.
pub fn rate_line(rate: Option<u32>) -> String {
    match rate {
        None => "-".to_string(),
        Some(_) => format!("{}/min", rate_text(rate)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1,200 cycles to the minute: a reading every 20.
    const MINUTE: u32 = 1200;

    /// Feeds one reading per 20 cycles to `cycles`, gathering `per_reading` at each.
    fn run(m: &mut Meter, cycles: u32, per_reading: Amounts, from: u32) -> Amounts {
        let mut total = [0; 3];
        let mut cycle = from;
        while cycle < from + cycles {
            cycle += 20;
            for i in 0..3 {
                total[i] += per_reading[i];
            }
            m.update(cycle, total, MINUTE);
        }
        total
    }

    #[test]
    fn nothing_shows_until_a_sixth_of_a_minute_has_passed() {
        let mut m = Meter::default();
        m.update(0, [0; 3], MINUTE);
        assert_eq!(m.rates(), None);
        m.update(20, [10, 0, 0], MINUTE);
        assert_eq!(m.rates(), None, "one second is too little");
        m.update(200, [100, 0, 0], MINUTE);
        assert_eq!(m.rates(), Some([600, 0, 0]), "ten seconds: 100 gold is 600 a minute");
    }

    #[test]
    fn a_steady_income_reads_as_its_per_minute_figure() {
        let mut m = Meter::default();
        // 5 gold, 2 lumber and 1 oil every second (20 cycles): 300, 120 and 60 a minute.
        run(&mut m, 3 * MINUTE, [5, 2, 1], 0);
        assert_eq!(m.rates(), Some([300, 120, 60]));
    }

    #[test]
    fn the_window_forgets_what_is_older_than_a_minute() {
        let mut m = Meter::default();
        let total = run(&mut m, 2 * MINUTE, [10, 0, 0], 0);
        assert_eq!(m.rates().map(|r| r[0]), Some(600));
        // Then the income stops: after a full minute the rate is 0.
        let mut cycle = 2 * MINUTE;
        for _ in 0..(MINUTE / 20) + 1 {
            cycle += 20;
            m.update(cycle, total, MINUTE);
        }
        assert_eq!(m.rates(), Some([0, 0, 0]));
        // Half way through that minute it was half the old rate.
        let mut m = Meter::default();
        let total = run(&mut m, 2 * MINUTE, [10, 0, 0], 0);
        let mut cycle = 2 * MINUTE;
        for _ in 0..(MINUTE / 40) {
            cycle += 20;
            m.update(cycle, total, MINUTE);
        }
        assert_eq!(m.rates().map(|r| r[0]), Some(300));
    }

    #[test]
    fn lumps_of_gold_step_the_rate_and_the_minute_smooths_them() {
        let mut m = Meter::default();
        // 100 gold every 10 seconds: 600 a minute, once a minute has been seen.
        for n in 1..=12u32 {
            m.update(n * 200, [n * 100, 0, 0], MINUTE);
        }
        assert_eq!(m.rates().map(|r| r[0]), Some(600));
    }

    #[test]
    fn readings_are_kept_about_once_a_second_and_bounded() {
        let mut m = Meter::default();
        for cycle in 0..10 * MINUTE {
            m.update(cycle, [cycle, 0, 0], MINUTE);
        }
        assert!(m.readings.len() <= 62, "{} readings", m.readings.len());
        assert_eq!(m.rates().map(|r| r[0]), Some(MINUTE));
    }

    #[test]
    fn a_pause_adds_no_cycles_so_the_rate_holds() {
        let mut m = Meter::default();
        run(&mut m, 2 * MINUTE, [5, 0, 0], 0);
        let before = m.rates();
        // The same cycle again and again, as a paused game's frames would.
        for _ in 0..1000 {
            m.update(2 * MINUTE, [600, 0, 0], MINUTE);
        }
        assert_eq!(m.rates(), before);
    }

    #[test]
    fn a_game_loaded_over_this_one_starts_the_record_again() {
        let mut m = Meter::default();
        run(&mut m, MINUTE, [5, 0, 0], 0);
        assert!(m.rates().is_some());
        m.update(40, [0; 3], MINUTE);
        assert_eq!(m.rates(), None);
        assert_eq!(m.readings.len(), 1);
    }

    #[test]
    fn a_faster_speed_shortens_the_window_in_cycles() {
        let mut m = Meter::default();
        // 60 cycles to a game minute (a made-up speed): gap of 1, 5 gold a cycle is 300 a minute.
        for cycle in 1..=200u32 {
            m.update(cycle, [cycle * 5, 0, 0], 60);
        }
        assert_eq!(m.rates().map(|r| r[0]), Some(300));
    }

    #[test]
    fn the_row_clears_the_group_bar() {
        // The widest line, `+9.9k/min`, is 9 characters of about 7 pixels: under 73, a counter's width.
        assert_eq!(rate_line(Some(9999)), "+9.9k/min");
        assert_eq!(rate_line(None), "-");
        assert_eq!(rate_line(Some(85)), "+85/min");
        assert_eq!(row_offset(false), 2);
        let bar_bottom = crate::groupbar::MARGIN + crate::groupbar::CELL_H;
        assert!(row_offset(true) > bar_bottom);
    }

    #[test]
    fn rates_read_as_short_text() {
        assert_eq!(rate_text(None), "-");
        assert_eq!(rate_text(Some(0)), "+0");
        assert_eq!(rate_text(Some(85)), "+85");
        assert_eq!(rate_text(Some(999)), "+999");
        assert_eq!(rate_text(Some(1000)), "+1.0k");
        assert_eq!(rate_text(Some(1234)), "+1.2k");
        assert_eq!(rate_text(Some(9999)), "+9.9k");
        assert_eq!(rate_text(Some(12_345)), "+12k");
    }
}
