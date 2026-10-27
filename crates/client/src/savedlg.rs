//! The save and load dialogs' layout, input and drawing (spec `docs/specs/game/save-load.md`, "Save dialog" and "Load
//! dialog"); the rules are in [`crate::saves`]. SDL-free: `gamemenu.rs` feeds it the frame's input and shows the
//! picture it draws.
//!
//! The dialog files (`savegame.bin`, `loadgame.bin`, `loadgamt.bin`) have no layout spec, so ours is our own, drawn like
//! the game menu's dialogs; the race picture (`Art\h192x128.bmp`, `Art\o192x128.bmp`) isn't drawn yet.
//!
//! **Keyboard focus** (`ui/screen-and-cursors.md`, "Keyboard focus", round 75): Tab and Shift+Tab move the focus over
//! the enabled buttons, then the list, then (saving) the name field ([`Stop`]); the load-game screen's order, Load,
//! Cancel, the list, is the observed one, the save dialog's is ours. The dialog opens focused on its default button
//! (Save or Load) when that is enabled and on Cancel otherwise; typing a name moves the focus to the field; Enter
//! activates the focused button, and with the list or the field focused it presses the default button, as before. The
//! default button has the entry-247 outline, enabled or disabled; the focus draws entry 251.

use tidewater_formats::fnt::Font;

use crate::hud::{self, Rect};
use crate::menuscreen::FocusKey;
use crate::saves::{Dialog, Mode, Stage, Step};
use crate::scene::Picture;

const FRAME: Rect = Rect {
    x: 120,
    y: 50,
    w: 400,
    h: 380,
};
const LIST: Rect = Rect {
    x: 140,
    y: 96,
    w: 360,
    h: 176,
};
pub const ROW_H: i32 = 22;
/// Rows the list shows at once.
pub const ROWS: usize = (LIST.h as i32 / ROW_H) as usize;
/// The name field, where a save dialog takes typing.
pub(crate) const FIELD: Rect = Rect {
    x: 140,
    y: 284,
    w: 360,
    h: 24,
};
const BUTTON_Y: i32 = 340;
const BUTTON_W: u32 = 112;
const BUTTON_H: u32 = 28;

/// What the pointer can hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Row(usize),
    Save,
    Delete,
    Load,
    Cancel,
    /// OK on a question or a message.
    Ok,
    /// Cancel on a question.
    No,
}

/// What Tab can stop on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Button(Hit),
    /// The saved games' list.
    List,
    /// The save dialog's name field.
    Field,
}

/// The focus ring's colour, entry 251, and the default button's outline, entry 247 in a game's palette (screen spec,
/// "Buttons").
const FOCUS_RGB: [u8; 3] = [255, 255, 0];
const DEFAULT_RGB: [u8; 3] = [160, 160, 164];

/// One frame's input.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// The pointer in 640x480 pixels.
    pub pointer: Option<(f32, f32)>,
    /// The left button went down this frame.
    pub press: bool,
    /// The left button was released this frame.
    pub click: bool,
    /// Text typed this frame.
    pub text: String,
    pub backspace: bool,
    pub enter: bool,
    pub escape: bool,
    pub up: bool,
    pub down: bool,
    /// Mouse wheel lines, positive up.
    pub wheel: i32,
    /// Tab or Shift+Tab.
    pub tab: Option<FocusKey>,
}

/// An open save or load dialog with its view state.
#[derive(Clone, Debug)]
pub struct SaveUi {
    pub dialog: Dialog,
    /// The first list row shown.
    pub scroll: usize,
    pub hot: Option<Hit>,
    /// What the left button went down on, while it is held (screen spec, "Pressing").
    pub held: crate::menuscreen::Hold<Hit>,
    /// The focus.
    pub focus: Stop,
}

/// The focused list's mark (round 75): a rectangle one pixel inside the list's left, top and right edges, round its
/// rows (ours: one pixel inside the bottom too, as our list is all rows).
fn list_mark() -> Rect {
    Rect {
        x: LIST.x + 1,
        y: LIST.y + 1,
        w: LIST.w - 2,
        h: LIST.h - 2,
    }
}

fn three(i: i32, n: i32) -> Rect {
    let gap = 12;
    let total = n * BUTTON_W as i32 + (n - 1) * gap;
    Rect {
        x: FRAME.x + (FRAME.w as i32 - total) / 2 + i * (BUTTON_W as i32 + gap),
        y: BUTTON_Y,
        w: BUTTON_W,
        h: BUTTON_H,
    }
}

impl SaveUi {
    pub fn new(dialog: Dialog) -> Self {
        let mut s = SaveUi {
            dialog,
            scroll: 0,
            hot: None,
            held: Default::default(),
            focus: Stop::List,
        };
        s.focus = s.default_stop();
        s.show_chosen();
        s
    }

    /// The default button: Save or Load on the list, OK on a question or a message.
    fn default_button(&self) -> Hit {
        match (&self.dialog.stage, self.dialog.mode) {
            (Stage::List, Mode::Save) => Hit::Save,
            (Stage::List, Mode::Load) => Hit::Load,
            _ => Hit::Ok,
        }
    }

    /// Where the focus goes when the dialog or a stage of it opens (screen spec, "Focus", round 75): the default
    /// button, or Cancel while the default button is disabled.
    fn default_stop(&self) -> Stop {
        let d = self.default_button();
        if self.on_offer(d) {
            Stop::Button(d)
        } else {
            Stop::Button(Hit::Cancel)
        }
    }

    fn on_offer(&self, h: Hit) -> bool {
        self.hits().iter().any(|(x, _)| *x == h)
    }

    /// The Tab stops in their order: the enabled buttons, then the list, then the name field (saving).
    pub fn stops(&self) -> Vec<Stop> {
        let mut v: Vec<Stop> = self
            .hits()
            .into_iter()
            .filter(|(h, _)| !matches!(h, Hit::Row(_)))
            .map(|(h, _)| Stop::Button(h))
            .collect();
        if self.dialog.stage == Stage::List {
            v.push(Stop::List);
            if self.dialog.mode == Mode::Save {
                v.push(Stop::Field);
            }
        }
        v
    }

    fn show_chosen(&mut self) {
        if let Some(i) = self.dialog.selected {
            if i < self.scroll {
                self.scroll = i;
            } else if i >= self.scroll + ROWS {
                self.scroll = i + 1 - ROWS;
            }
        }
        let max = self.dialog.entries.len().saturating_sub(ROWS);
        self.scroll = self.scroll.min(max);
    }

    /// The things the pointer can hit now, with their places.
    pub fn hits(&self) -> Vec<(Hit, Rect)> {
        let d = &self.dialog;
        let mut v = Vec::new();
        match &d.stage {
            Stage::List => {
                for (n, i) in (self.scroll..d.entries.len()).take(ROWS).enumerate() {
                    v.push((
                        Hit::Row(i),
                        Rect {
                            x: LIST.x,
                            y: LIST.y + n as i32 * ROW_H,
                            w: LIST.w,
                            h: ROW_H as u32,
                        },
                    ));
                }
                match d.mode {
                    Mode::Save => {
                        if d.save_enabled() {
                            v.push((Hit::Save, three(0, 3)));
                        }
                        if d.delete_enabled() {
                            v.push((Hit::Delete, three(1, 3)));
                        }
                        v.push((Hit::Cancel, three(2, 3)));
                    }
                    Mode::Load => {
                        if d.load_enabled() {
                            v.push((Hit::Load, three(0, 2)));
                        }
                        v.push((Hit::Cancel, three(1, 2)));
                    }
                }
            }
            Stage::Confirm(_) => {
                v.push((Hit::Ok, three(0, 2)));
                v.push((Hit::No, three(1, 2)));
            }
            Stage::Notice(_) => v.push((Hit::Ok, three(0, 1))),
        }
        v
    }

    /// Runs one frame of input and returns what the caller must do.
    pub fn step(&mut self, input: &Input) -> Step {
        let hits = self.hits();
        self.hot = input
            .pointer
            .and_then(|(x, y)| hits.iter().find(|(_, r)| r.contains(x, y)).map(|(h, _)| *h));
        // Screen spec, "Pressing": only a release on what the button went down on counts; pressing a button gives it
        // the focus.
        if input.press {
            self.held.down(self.hot);
            if let Some(h) = self.hot.filter(|h| !matches!(h, Hit::Row(_))) {
                self.focus = Stop::Button(h);
            }
        }
        if let Some(k) = input.tab {
            self.focus = crate::menuscreen::step_focus(&self.stops(), self.focus, k);
        }
        let stage = self.dialog.stage.clone();
        let mut hit = if input.click { self.held.up(self.hot) } else { None };
        let d = &mut self.dialog;
        if d.stage == Stage::List {
            input.text.chars().for_each(|c| d.type_char(c));
            if input.backspace {
                d.backspace();
            }
            // Typing into the name field moves the focus to it (round 75).
            if d.mode == Mode::Save && (!input.text.is_empty() || input.backspace) {
                self.focus = Stop::Field;
            }
            if input.wheel != 0 {
                let max = d.entries.len().saturating_sub(ROWS) as i64;
                self.scroll = (self.scroll as i64 - input.wheel as i64).clamp(0, max) as usize;
            }
            if (input.up || input.down) && !d.entries.is_empty() {
                let at = d.selected;
                let to = match (at, input.up) {
                    (None, true) => d.entries.len() - 1,
                    (None, false) => 0,
                    (Some(i), true) => i.saturating_sub(1),
                    (Some(i), false) => (i + 1).min(d.entries.len() - 1),
                };
                d.choose(to);
                self.show_chosen();
            }
        }
        // Enter activates the focused button; with the list or the field focused, the default button.
        if input.enter && hit.is_none() {
            hit = Some(match self.focus {
                Stop::Button(h) if self.on_offer(h) => h,
                _ => self.default_button(),
            });
        }
        let d = &mut self.dialog;
        if input.escape {
            hit = Some(match d.stage {
                Stage::Confirm(_) => Hit::No,
                Stage::Notice(_) => Hit::Ok,
                Stage::List => Hit::Cancel,
            });
        }
        let Some(hit) = hit else {
            self.settle_focus(&stage);
            return Step::Nothing;
        };
        // A hit only counts if it is one of the things now on offer.
        if !matches!(hit, Hit::Row(_)) && !hits.iter().any(|(h, _)| *h == hit) && !input.enter && !input.escape {
            return Step::Nothing;
        }
        let step = match hit {
            Hit::Row(i) => {
                d.choose(i);
                Step::Nothing
            }
            Hit::Save => d.press_save(),
            Hit::Delete => d.press_delete(),
            Hit::Load => d.press_load(),
            Hit::Cancel => d.press_cancel(),
            Hit::Ok => match d.stage {
                Stage::Confirm(_) => d.confirm_ok(),
                _ => {
                    d.notice_ok();
                    Step::Nothing
                }
            },
            Hit::No => {
                d.confirm_cancel();
                Step::Nothing
            }
        };
        self.settle_focus(&stage);
        step
    }

    /// After a frame: a stage that changed opens on its own default focus, and a focus no longer on offer (a button
    /// that went disabled) goes to the default too.
    fn settle_focus(&mut self, before: &Stage) {
        if self.dialog.stage != *before || !self.stops().contains(&self.focus) {
            self.focus = self.default_stop();
        }
    }

    /// Draws the dialog. `net` gives a `network.tbl` entry's text (spec entries, numbered from 1).
    pub fn draw(&self, font: &Font, big: &Font, net: &dyn Fn(u16) -> String) -> Picture {
        let d = &self.dialog;
        let mut p = Picture {
            width: 640,
            height: 480,
            rgba: vec![0; 640 * 480 * 4],
        };
        hud::fill(&mut p, FRAME, [24, 20, 16]);
        hud::outline(&mut p, FRAME, [180, 160, 100]);
        let title = if d.mode == Mode::Save { "Save Game" } else { "Load Game" };
        let tw = big.width(title.as_bytes()) as i32;
        hud::text(&mut p, big, FRAME.x + (FRAME.w as i32 - tw) / 2, FRAME.y + 14, title);
        let pressed = self.held.pressed(self.hot);
        let default = self.default_button();
        // Screen spec, "Buttons": the default button's 247 outline, the focus's 251 one over it.
        let marks = |p: &mut Picture, h: Hit, r: Rect| {
            if self.focus == Stop::Button(h) {
                hud::outline(p, r, FOCUS_RGB);
            } else if h == default {
                hud::outline(p, r, DEFAULT_RGB);
            }
        };
        let button = |p: &mut Picture, h: Hit, r: Rect, label: &str| {
            crate::frontend::button_pressed(p, font, r, label, self.hot == Some(h), pressed == Some(h));
            marks(p, h, r);
        };
        match &d.stage {
            Stage::List => {
                hud::fill(&mut p, LIST, [12, 10, 8]);
                hud::outline(&mut p, LIST, [120, 106, 66]);
                if self.focus == Stop::List {
                    hud::outline(&mut p, list_mark(), FOCUS_RGB);
                }
                for (h, r) in self.hits() {
                    if let Hit::Row(i) = h {
                        let on = d.selected == Some(i);
                        if on {
                            hud::fill(&mut p, r, [90, 76, 36]);
                        } else if self.hot == Some(h) {
                            hud::fill(&mut p, r, [50, 42, 22]);
                        }
                        hud::text(
                            &mut p,
                            font,
                            r.x + 6,
                            r.y + (ROW_H - font.height as i32) / 2,
                            &d.entries[i].stem,
                        );
                    }
                }
                if d.mode == Mode::Save {
                    hud::fill(&mut p, FIELD, [12, 10, 8]);
                    hud::outline(&mut p, FIELD, [180, 160, 100]);
                    if self.focus == Stop::Field {
                        hud::outline(&mut p, FIELD, FOCUS_RGB);
                    }
                    let shown = format!("{}_", d.name);
                    hud::text(
                        &mut p,
                        font,
                        FIELD.x + 6,
                        FIELD.y + (FIELD.h as i32 - font.height as i32) / 2,
                        &shown,
                    );
                }
                let (labels, n): (&[(Hit, &str)], i32) = match d.mode {
                    Mode::Save => (
                        &[(Hit::Save, "Save"), (Hit::Delete, "Delete"), (Hit::Cancel, "Cancel")],
                        3,
                    ),
                    Mode::Load => (&[(Hit::Load, "Load"), (Hit::Cancel, "Cancel")], 2),
                };
                let on = self.hits();
                for (i, (h, label)) in labels.iter().enumerate() {
                    let r = three(i as i32, n);
                    if on.iter().any(|(x, _)| x == h) {
                        button(&mut p, *h, r, label);
                    } else {
                        // Not enabled: dim, no label colour change available, so just the dim box, with its own ring
                        // unless it is the default button (round 75).
                        hud::fill(&mut p, r, [20, 18, 14]);
                        hud::outline(&mut p, r, [80, 72, 48]);
                        marks(&mut p, *h, r);
                    }
                }
            }
            Stage::Confirm(c) => {
                let text = crate::saves::fill(&net(c.entry()), c.name());
                self.centred_lines(&mut p, font, &[text]);
                button(&mut p, Hit::Ok, three(0, 2), "OK");
                button(&mut p, Hit::No, three(1, 2), "Cancel");
            }
            Stage::Notice(n) => {
                self.centred_lines(&mut p, font, &n.lines(net));
                button(&mut p, Hit::Ok, three(0, 1), "OK");
            }
        }
        p
    }

    fn centred_lines(&self, p: &mut Picture, font: &Font, lines: &[String]) {
        let wrap = |s: &str| {
            crate::campaign::wrap(&s.replace("\\n", " "), FRAME.w - 40, &|t: &str| {
                font.width(t.as_bytes())
            })
        };
        let mut y = FRAME.y + 120;
        for l in lines.iter().flat_map(|l| wrap(l)) {
            let w = font.width(l.as_bytes()) as i32;
            hud::text(p, font, FRAME.x + (FRAME.w as i32 - w) / 2, y, &l);
            y += font.height as i32 + 6;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saves::{Confirm, Entry, FORBIDDEN};

    fn entries(n: usize) -> Vec<Entry> {
        (0..n)
            .map(|i| Entry {
                stem: format!("g{i}"),
                file: format!("g{i}.sav"),
                stored_name: format!("g{i}"),
                saved_at: 1000 - i as u64,
                race: 0,
            })
            .collect()
    }

    fn ui(mode: Mode, n: usize) -> SaveUi {
        SaveUi::new(Dialog::open(mode, entries(n), FORBIDDEN))
    }

    fn click_on(ui: &mut SaveUi, h: Hit) -> Step {
        let r = ui.hits().into_iter().find(|(x, _)| *x == h).expect("on offer").1;
        let (x, y) = (r.x as f32 + 2.0, r.y as f32 + 2.0);
        ui.step(&Input {
            pointer: Some((x, y)),
            press: true,
            click: true,
            ..Default::default()
        })
    }

    fn tab(u: &mut SaveUi) -> Step {
        u.step(&Input {
            tab: Some(FocusKey::Next),
            ..Default::default()
        })
    }

    #[test]
    fn the_load_dialog_opens_on_load_and_tabs_to_cancel_then_the_list() {
        // Screen spec, "Keyboard focus" and the round-75 vector: one saved game listed.
        let mut u = ui(Mode::Load, 1);
        assert_eq!(u.focus, Stop::Button(Hit::Load));
        tab(&mut u);
        assert_eq!(u.focus, Stop::Button(Hit::Cancel));
        tab(&mut u);
        assert_eq!(u.focus, Stop::List);
        tab(&mut u);
        assert_eq!(u.focus, Stop::Button(Hit::Load), "and round to Load");
        // No saved games: Load is disabled, so the focus starts on Cancel.
        assert_eq!(ui(Mode::Load, 0).focus, Stop::Button(Hit::Cancel));
    }

    #[test]
    fn the_save_dialog_opens_on_cancel_and_typing_moves_the_focus_to_the_field() {
        // Round 75: no name typed, Save (the default) disabled: the focus on Cancel; typing: on the name field.
        let mut u = ui(Mode::Save, 2);
        assert_eq!(u.focus, Stop::Button(Hit::Cancel));
        assert_eq!(
            u.stops(),
            [Stop::Button(Hit::Cancel), Stop::List, Stop::Field],
            "disabled buttons are passed over"
        );
        u.step(&Input {
            text: "a".into(),
            ..Default::default()
        });
        assert_eq!(u.focus, Stop::Field);
        // Tab never changes anything: the name and the choice stay.
        for _ in 0..4 {
            tab(&mut u);
        }
        assert_eq!((u.dialog.name.as_str(), u.dialog.selected), ("a", None));
    }

    #[test]
    fn enter_activates_the_focused_button() {
        // Load with a saved game: Tab to Cancel, Enter closes rather than loads.
        let mut u = ui(Mode::Load, 1);
        tab(&mut u);
        let s = u.step(&Input {
            enter: true,
            ..Default::default()
        });
        assert_eq!(s, Step::Close);
        // With the list focused, Enter presses the default button.
        let mut u = ui(Mode::Load, 1);
        tab(&mut u);
        tab(&mut u);
        let s = u.step(&Input {
            enter: true,
            ..Default::default()
        });
        assert!(matches!(s, Step::Load(_)));
    }

    #[test]
    fn only_a_release_on_the_button_pressed_chooses_it() {
        // Screen spec, "Pressing": pressed on Cancel, released on Load: nothing; pressed and released on Cancel: closes.
        let mut u = ui(Mode::Load, 2);
        let at = |u: &SaveUi, h: Hit| {
            let r = u.hits().into_iter().find(|(x, _)| *x == h).expect("on offer").1;
            Some((r.x as f32 + 2.0, r.y as f32 + 2.0))
        };
        let (cancel, load) = (at(&u, Hit::Cancel), at(&u, Hit::Load));
        u.step(&Input {
            pointer: cancel,
            press: true,
            ..Default::default()
        });
        let s = u.step(&Input {
            pointer: load,
            click: true,
            ..Default::default()
        });
        assert_eq!(s, Step::Nothing);
        u.step(&Input {
            pointer: cancel,
            press: true,
            ..Default::default()
        });
        u.step(&Input {
            pointer: None,
            ..Default::default()
        });
        let s = u.step(&Input {
            pointer: cancel,
            click: true,
            ..Default::default()
        });
        assert_eq!(s, Step::Close, "off the button and back on, then released there");
    }

    #[test]
    fn nothing_overlaps_and_it_all_fits() {
        for mode in [Mode::Save, Mode::Load] {
            let mut u = ui(mode, 30);
            u.dialog.choose(0);
            for stage in 0..3 {
                match stage {
                    1 => u.dialog.stage = Stage::Confirm(Confirm::Delete("x".into())),
                    2 => u.dialog.stage = Stage::Notice(crate::saves::Notice::TooMany),
                    _ => {}
                }
                let hits = u.hits();
                for (i, (_, a)) in hits.iter().enumerate() {
                    assert!(a.x >= FRAME.x && a.x + a.w as i32 <= FRAME.x + FRAME.w as i32);
                    assert!(a.y >= FRAME.y && a.y + a.h as i32 <= FRAME.y + FRAME.h as i32);
                    for (_, c) in &hits[i + 1..] {
                        let apart = a.x + a.w as i32 <= c.x
                            || c.x + c.w as i32 <= a.x
                            || a.y + a.h as i32 <= c.y
                            || c.y + c.h as i32 <= a.y;
                        assert!(apart);
                    }
                }
            }
        }
        assert!(LIST.y + LIST.h as i32 <= FIELD.y);
        assert!(FIELD.y + FIELD.h as i32 <= BUTTON_Y);
    }

    #[test]
    fn typing_choosing_and_saving() {
        let mut u = ui(Mode::Save, 3);
        assert!(!u.hits().iter().any(|(h, _)| *h == Hit::Save), "Save needs a name");
        u.step(&Input {
            text: "Si?ege".into(),
            ..Default::default()
        });
        assert_eq!(u.dialog.name, "Siege");
        assert_eq!(
            click_on(&mut u, Hit::Save),
            Step::Write {
                stem: "Siege".into(),
                name: "Siege".into()
            }
        );
        click_on(&mut u, Hit::Row(1));
        assert_eq!(u.dialog.name, "g1");
        assert!(u.hits().iter().any(|(h, _)| *h == Hit::Delete));
        // Enter saves over g1 after the question.
        u.step(&Input {
            enter: true,
            ..Default::default()
        });
        assert!(matches!(u.dialog.stage, Stage::Confirm(Confirm::Overwrite(_))));
        let s = u.step(&Input {
            enter: true,
            ..Default::default()
        });
        assert!(matches!(s, Step::Write { .. }));
    }

    #[test]
    fn escape_backs_out_one_level() {
        let mut u = ui(Mode::Save, 2);
        u.dialog.choose(0);
        click_on(&mut u, Hit::Delete);
        assert!(matches!(u.dialog.stage, Stage::Confirm(Confirm::Delete(_))));
        let esc = Input {
            escape: true,
            ..Default::default()
        };
        assert_eq!(u.step(&esc), Step::Nothing);
        assert_eq!(u.dialog.stage, Stage::List);
        assert_eq!(u.step(&esc), Step::Close);
    }

    #[test]
    fn delete_goes_through_the_question() {
        let mut u = ui(Mode::Save, 2);
        click_on(&mut u, Hit::Row(1));
        click_on(&mut u, Hit::Delete);
        let s = click_on(&mut u, Hit::Ok);
        assert!(matches!(s, Step::Delete(e) if e.stem == "g1"));
    }

    #[test]
    fn the_list_scrolls_with_the_wheel_and_the_arrows() {
        let mut u = ui(Mode::Load, 30);
        assert_eq!(u.hits().iter().filter(|(h, _)| matches!(h, Hit::Row(_))).count(), ROWS);
        u.step(&Input {
            wheel: -3,
            ..Default::default()
        });
        assert_eq!(u.scroll, 3);
        for _ in 0..12 {
            u.step(&Input {
                down: true,
                ..Default::default()
            });
        }
        assert_eq!(u.dialog.selected, Some(12));
        assert!(u.scroll + ROWS > 12 && u.scroll <= 12);
        assert!(matches!(click_on(&mut u, Hit::Load), Step::Load(e) if e.stem == "g12"));
    }

    #[test]
    fn load_dialog_has_no_field_and_an_empty_list_cannot_load() {
        let mut u = ui(Mode::Load, 0);
        assert!(!u.hits().iter().any(|(h, _)| *h == Hit::Load));
        assert!(!u.stops().contains(&Stop::Field));
        // Tab to the list, where Enter would press Load: nothing, as Load is disabled.
        tab(&mut u);
        assert_eq!(u.focus, Stop::List);
        assert_eq!(
            u.step(&Input {
                enter: true,
                ..Default::default()
            }),
            Step::Nothing
        );
        // On Cancel, where the dialog opens, Enter closes it (round 75: Enter activates the focused button).
        tab(&mut u);
        assert_eq!(
            u.step(&Input {
                enter: true,
                ..Default::default()
            }),
            Step::Close
        );
    }
}
