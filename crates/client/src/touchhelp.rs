//! The Touch Controls help (ours, `docs/design/touch.md`, "Help"): two pages, "On the map" for what the fingers do and
//! "Buttons" for the side bars, each button drawn as the bars draw it (`chrome::button_picture`). The game menu shows
//! it over the game (`gamemenu`, `Screen::TouchHelp`, from the Help menu), the main menu over its picture (`frontend`,
//! the Touch controls link), and a phone or tablet shows it by itself the first time a game starts (the
//! `touch_help_seen` line of the stored settings, `prefs`). [`Pager`] holds the page and the buttons' input, the same on
//! both. SDL-free.

use tidewater_formats::fnt::Font;

use crate::chrome::{self, Control, Look};
use crate::hud::{self, Rect, TEXT, TEXT_ALT, TextStyle};
use crate::scene::Picture;

/// The pages' title.
pub const TITLE: &str = "Touch Controls";

/// A row of a page: the buttons drawn at its left (none on the first page), the gesture or button's name (in yellow)
/// and what it does (in white). A row with an empty name continues the one above it.
pub(crate) struct Row {
    pub icons: &'static [(Control, Look)],
    pub name: &'static str,
    pub does: &'static str,
}

/// A page: its heading under the title, its rows and a tip under them across the page, each line in two parts (the
/// first in yellow).
pub(crate) struct Page {
    pub heading: &'static str,
    pub rows: &'static [Row],
    pub tip: &'static [(&'static str, &'static str)],
}

const fn row(name: &'static str, does: &'static str) -> Row {
    Row { icons: &[], name, does }
}

const fn icons(icons: &'static [(Control, Look)], name: &'static str, does: &'static str) -> Row {
    Row { icons, name, does }
}

/// The two pages. Each line must fit its column in `font12x` (the test measures them with the real font).
pub(crate) const PAGES: [Page; 2] = [
    Page {
        heading: "On the map",
        rows: &[
            row("Drag", "Scroll the map"),
            row("Hold, then drag", "Select with a box"),
            row("Tap", "Select, or order the selected units"),
            row("Two taps", "Select all units of that type"),
            row("Hold, then lift", "Right click"),
            row("Two fingers", "Scroll; pinch to zoom on a phone"),
            row("Three fingers", "Open the game menu"),
            row("Minimap", "Tap or drag to look, hold to order"),
            row("Building", "Drag the outline, tap it to place"),
            row("Settings", "The Touch page changes all this"),
            row("Help menu", "Shows these pages again"),
        ],
        tip: &[],
    },
    Page {
        heading: "Buttons",
        rows: &[
            icons(&[(Control::Menu, Look::Off)], "Menu", "Open the game menu"),
            icons(&[(Control::Pause, Look::Off)], "Pause", "Pause or resume the game"),
            icons(
                &[
                    (Control::Shift, Look::Off),
                    (Control::Ctrl, Look::Off),
                    (Control::Alt, Look::Off),
                ],
                "Shift, Ctrl, Alt",
                "Held for your next tap",
            ),
            icons(
                &[(Control::Shift, Look::Once), (Control::Shift, Look::Locked)],
                "Double tap",
                "Locks it on; tap again to let go",
            ),
            icons(&[(Control::Plain, Look::Off)], "Pointer", "Next tap is a plain click"),
            icons(
                &[(Control::Keyboard, Look::Off)],
                "Keyboard",
                "Show or hide the keyboard",
            ),
            icons(
                &[
                    (Control::Group(1), Look::Off),
                    (Control::Group(2), Look::Off),
                    (Control::Group(3), Look::Off),
                ],
                "Groups",
                "Tap: recall. Hold: make the group",
            ),
            icons(
                &[(Control::IdleWorker, Look::Off)],
                "Idle worker",
                "Select an idle worker",
            ),
        ],
        tip: &[
            ("Pick units one by one: ", "double-tap Shift to lock it,"),
            ("", "then tap units to add or remove them."),
        ],
    },
];

/// The panel: most of the 640 × 480 frame.
pub(crate) const PANEL: Rect = Rect::new(16, 8, 608, 464);

/// The text's left edge from the panel's.
const INSET: i32 = 28;
/// The first page's name column; its meanings start after it.
const NAME_W: u32 = 168;
/// The second page's button column: three buttons of [`ICON`] pixels with [`ICON_GAP`] between.
const ICON: u32 = 28;
const ICON_GAP: i32 = 4;
const ICONS_W: u32 = 3 * ICON + 2 * ICON_GAP as u32 + 12;
/// The second page's name column.
const ICON_NAME_W: u32 = 136;
/// The rows' pitch: the first page's, the second's (a button is [`ICON`] high).
const PITCH: [i32; 2] = [26, 31];

/// The buttons under the pages: [`BUTTON_H`] high, which with [`HIT_PAD`] round them makes a target of 72 pixels,
/// 9 mm on a phone in landscape (`docs/design/touch.md`, "Help").
const BUTTON_W: u32 = 150;
const BUTTON_H: u32 = 44;
const HIT_PAD: i32 = 14;
const BUTTONS_Y: i32 = PANEL.y + PANEL.h as i32 - 14 - BUTTON_H as i32;

/// A button under the pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Back,
    Next,
    Ok,
}

impl Target {
    fn label(self) -> &'static str {
        match self {
            Target::Back => "Back",
            Target::Next => "Next",
            Target::Ok => "OK",
        }
    }
}

/// The buttons of page `page` and where they are: Next on the first, Back and OK on the last.
pub(crate) fn buttons(page: usize) -> Vec<(Target, Rect)> {
    let left = Rect::new(PANEL.x + INSET, BUTTONS_Y, BUTTON_W, BUTTON_H);
    let right = Rect::new(
        PANEL.x + PANEL.w as i32 - INSET - BUTTON_W as i32,
        BUTTONS_Y,
        BUTTON_W,
        BUTTON_H,
    );
    if page + 1 < PAGES.len() {
        let mut v = Vec::new();
        if page > 0 {
            v.push((Target::Back, left));
        }
        v.push((Target::Next, right));
        v
    } else {
        vec![(Target::Back, left), (Target::Ok, right)]
    }
}

/// A button's tap target: its rectangle and [`HIT_PAD`] round it.
fn hit(r: Rect) -> Rect {
    Rect::new(
        r.x - HIT_PAD,
        r.y - HIT_PAD,
        r.w + 2 * HIT_PAD as u32,
        r.h + 2 * HIT_PAD as u32,
    )
}

/// The key a frame brings the pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageKey {
    /// Esc: closes them.
    Close,
    /// Enter: the default button, Next or OK.
    Enter,
    /// The right arrow.
    Next,
    /// The left arrow.
    Back,
}

/// A frame's input for the pages: the pointer in the 640 × 480 frame, the left button's press and release, a key.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Feed {
    pub at: Option<(i32, i32)>,
    pub down: bool,
    pub up: bool,
    pub key: Option<PageKey>,
}

/// What a frame's input did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Same,
    /// The pages look different: draw them again.
    Redraw,
    Close,
}

/// The open pages: which one shows, the button the pointer is over and the one the left button went down on (a button
/// acts when the button comes up on the one it went down on, as menu buttons do).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pager {
    pub page: usize,
    hot: Option<Target>,
    held: Option<Target>,
}

impl Pager {
    fn target(&self, at: Option<(i32, i32)>) -> Option<Target> {
        let (x, y) = at?;
        buttons(self.page)
            .into_iter()
            .find(|&(_, r)| hit(r).contains(x as f32, y as f32))
            .map(|(t, _)| t)
    }

    /// Runs a frame's input.
    pub(crate) fn step(&mut self, feed: Feed) -> Outcome {
        let before = *self;
        let over = self.target(feed.at);
        self.hot = over;
        if feed.down {
            self.held = over;
        }
        let mut act = None;
        if feed.up {
            if self.held.is_some() && self.held == over {
                act = over;
            }
            self.held = None;
        }
        let last = PAGES.len() - 1;
        act = match feed.key {
            Some(PageKey::Close) => Some(Target::Ok),
            Some(PageKey::Enter) => Some(if self.page == last { Target::Ok } else { Target::Next }),
            Some(PageKey::Next) if self.page < last => Some(Target::Next),
            Some(PageKey::Back) if self.page > 0 => Some(Target::Back),
            _ => act,
        };
        match act {
            Some(Target::Ok) => return Outcome::Close,
            Some(Target::Next) => self.turn(self.page + 1),
            Some(Target::Back) => self.turn(self.page.saturating_sub(1)),
            None => {}
        }
        if *self == before {
            Outcome::Same
        } else {
            Outcome::Redraw
        }
    }

    fn turn(&mut self, page: usize) {
        self.page = page.min(PAGES.len() - 1);
        self.held = None;
        self.hot = None;
    }

    /// Draws the page on `dst` (640 × 480): the panel with a gold rule, the title in `big`, the rest in `font`.
    pub(crate) fn draw(&self, dst: &mut Picture, big: &Font, font: &Font) {
        let gold = TEXT_ALT[1];
        let b = PANEL;
        hud::fill(dst, b, [16, 12, 8]);
        hud::outline(dst, b, gold);
        hud::outline(dst, Rect::new(b.x + 1, b.y + 1, b.w - 2, b.h - 2), [120, 96, 40]);
        let l = Layout::of(big, font, self.page);
        let tw = big.width(TITLE.as_bytes()) as i32;
        hud::text(dst, big, b.x + (b.w as i32 - tw) / 2, l.title_y, TITLE);
        let page = &PAGES[self.page];
        let heading = format!("{} ({} of {})", page.heading, self.page + 1, PAGES.len());
        let hw = font.width(heading.as_bytes()) as i32;
        put(
            dst,
            font,
            (b.x + (b.w as i32 - hw) / 2, l.heading_y),
            &heading,
            &TEXT_ALT,
        );
        hud::fill(
            dst,
            Rect::new(b.x + INSET, l.rule_y, b.w - 2 * INSET as u32, 1),
            [120, 96, 40],
        );
        let cols = columns(self.page);
        for (i, r) in page.rows.iter().enumerate() {
            let y = l.rows_y + i as i32 * PITCH[self.page];
            let text_y = y + (l.row_h - font.height as i32) / 2;
            let mut x = cols.icons;
            for &(c, look) in r.icons {
                blend(
                    dst,
                    &chrome::button_picture(c, look, ICON),
                    (x, y + (l.row_h - ICON as i32) / 2),
                );
                x += (ICON as i32) + ICON_GAP;
            }
            put(dst, font, (cols.name, text_y), r.name, &TEXT_ALT);
            put(dst, font, (cols.does, text_y), r.does, &TEXT);
        }
        let tip_y = l.tip_y(page.rows.len(), PITCH[self.page]);
        for (i, &(lead, rest)) in page.tip.iter().enumerate() {
            let y = tip_y + i as i32 * l.tip_pitch;
            let x = cols.icons;
            put(dst, font, (x, y), lead, &TEXT_ALT);
            put(dst, font, (x + font.width(lead.as_bytes()) as i32, y), rest, &TEXT);
        }
        for (t, r) in buttons(self.page) {
            crate::frontend::button_pressed(dst, font, r, t.label(), self.hot == Some(t), self.held == Some(t));
            let default = if self.page + 1 == PAGES.len() {
                Target::Ok
            } else {
                Target::Next
            };
            if t == default {
                hud::outline(dst, r, [255, 255, 0]);
            }
        }
    }
}

fn put(dst: &mut Picture, font: &Font, at: (i32, i32), s: &str, colours: &[[u8; 3]; 8]) {
    let style = TextStyle {
        colours,
        space: None,
        rows: (i32::MIN, i32::MAX),
    };
    hud::text_styled(dst, font, at, s, &style);
}

/// Blends a button's picture (`chrome::Overlay`, RGBA without premultiplication) onto `dst` at `at`.
fn blend(dst: &mut Picture, o: &chrome::Overlay, at: (i32, i32)) {
    let (w, h) = (o.size.0 as i32, o.size.1 as i32);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (at.0 + x, at.1 + y);
            if dx < 0 || dy < 0 || dx >= dst.width as i32 || dy >= dst.height as i32 {
                continue;
            }
            let s = &o.rgba[((y * w + x) * 4) as usize..][..4];
            let a = u32::from(s[3]);
            if a == 0 {
                continue;
            }
            let d = &mut dst.rgba[((dy * dst.width as i32 + dx) * 4) as usize..][..4];
            for ch in 0..3 {
                d[ch] = ((u32::from(d[ch]) * (255 - a) + u32::from(s[ch]) * a + 127) / 255) as u8;
            }
            d[3] = 255;
        }
    }
}

/// The x of a page's columns: its buttons (the second page's), the names and what they do.
struct Columns {
    icons: i32,
    name: i32,
    does: i32,
}

/// Whether page `page` draws buttons in its rows.
fn has_icons(page: usize) -> bool {
    PAGES[page].rows.iter().any(|r| !r.icons.is_empty())
}

fn columns(page: usize) -> Columns {
    let left = PANEL.x + INSET;
    if has_icons(page) {
        Columns {
            icons: left,
            name: left + ICONS_W as i32,
            does: left + (ICONS_W + ICON_NAME_W) as i32,
        }
    } else {
        Columns {
            icons: left,
            name: left,
            does: left + NAME_W as i32,
        }
    }
}

/// Where the parts sit from top to bottom, from the fonts' heights.
struct Layout {
    title_y: i32,
    heading_y: i32,
    rule_y: i32,
    rows_y: i32,
    /// A row's height: the taller of a button and the text.
    row_h: i32,
    /// The tip's line pitch.
    tip_pitch: i32,
}

impl Layout {
    fn of(big: &Font, font: &Font, page: usize) -> Self {
        let title_y = PANEL.y + 10;
        let heading_y = title_y + big.height as i32 + 4;
        let rule_y = heading_y + font.height as i32 + 6;
        Layout {
            title_y,
            heading_y,
            rule_y,
            rows_y: rule_y + 10,
            row_h: if has_icons(page) {
                (ICON as i32).max(font.height as i32)
            } else {
                font.height as i32
            },
            tip_pitch: font.height as i32 + 6,
        }
    }

    /// The tip's top under `rows` rows at `pitch`.
    fn tip_y(&self, rows: usize, pitch: i32) -> i32 {
        self.rows_y + (rows as i32 - 1) * pitch + self.row_h + 14
    }

    /// The bottom of a page's text.
    #[cfg(test)]
    fn bottom(&self, page: usize) -> i32 {
        let p = &PAGES[page];
        let rows = self.rows_y + (p.rows.len() as i32 - 1) * PITCH[page] + self.row_h;
        if p.tip.is_empty() {
            rows
        } else {
            self.tip_y(p.rows.len(), PITCH[page]) + p.tip.len() as i32 * self.tip_pitch - 6
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(p: &mut Pager, at: (i32, i32)) -> Outcome {
        p.step(Feed {
            at: Some(at),
            down: true,
            ..Feed::default()
        });
        p.step(Feed {
            at: Some(at),
            up: true,
            ..Feed::default()
        })
    }

    fn centre(r: Rect) -> (i32, i32) {
        (r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
    }

    fn of(page: usize, t: Target) -> Rect {
        buttons(page).into_iter().find(|&(b, _)| b == t).expect("the button").1
    }

    #[test]
    fn next_back_and_ok_turn_the_pages_and_close_them() {
        let mut p = Pager::default();
        assert_eq!(press(&mut p, centre(of(0, Target::Next))), Outcome::Redraw);
        assert_eq!(p.page, 1);
        assert_eq!(press(&mut p, centre(of(1, Target::Back))), Outcome::Redraw);
        assert_eq!(p.page, 0);
        // A tap just outside a button still hits it; one in the middle of the page does nothing.
        let next = of(0, Target::Next);
        assert_eq!(
            press(&mut p, (next.x - HIT_PAD + 1, next.y - HIT_PAD + 1)),
            Outcome::Redraw
        );
        assert_eq!(press(&mut p, (320, 200)), Outcome::Same);
        assert_eq!(press(&mut p, centre(of(1, Target::Ok))), Outcome::Close);
    }

    #[test]
    fn a_button_acts_only_when_released_on_the_one_it_went_down_on() {
        let mut p = Pager::default();
        p.step(Feed {
            at: Some(centre(of(0, Target::Next))),
            down: true,
            ..Feed::default()
        });
        p.step(Feed {
            at: Some((320, 200)),
            up: true,
            ..Feed::default()
        });
        assert_eq!(p.page, 0);
    }

    #[test]
    fn keys_turn_the_pages_enter_takes_the_default_and_esc_closes() {
        let key = |p: &mut Pager, k| {
            p.step(Feed {
                key: Some(k),
                ..Feed::default()
            })
        };
        let mut p = Pager::default();
        assert_eq!(key(&mut p, PageKey::Back), Outcome::Same);
        assert_eq!(key(&mut p, PageKey::Enter), Outcome::Redraw);
        assert_eq!(p.page, 1);
        assert_eq!(key(&mut p, PageKey::Next), Outcome::Same);
        assert_eq!(key(&mut p, PageKey::Back), Outcome::Redraw);
        assert_eq!(key(&mut p, PageKey::Next), Outcome::Redraw);
        assert_eq!(key(&mut p, PageKey::Enter), Outcome::Close);
        assert_eq!(key(&mut Pager::default(), PageKey::Close), Outcome::Close);
    }

    #[test]
    fn the_buttons_tap_targets_are_big_and_inside_the_frame() {
        for page in 0..PAGES.len() {
            let bs = buttons(page);
            for (i, &(_, r)) in bs.iter().enumerate() {
                let h = hit(r);
                assert!(h.h >= 72 && h.w >= 72, "{h:?}");
                assert!(
                    h.x >= 0 && h.y >= 0 && h.x + h.w as i32 <= 640 && h.y + h.h as i32 <= 480,
                    "{h:?}"
                );
                for &(_, s) in &bs[i + 1..] {
                    assert!(h.x + h.w as i32 <= hit(s).x || hit(s).x + hit(s).w as i32 <= h.x);
                }
            }
        }
        assert!(PANEL.x >= 0 && PANEL.y >= 0 && PANEL.x + PANEL.w as i32 <= 640 && PANEL.y + PANEL.h as i32 <= 480);
    }

    #[test]
    fn every_row_has_text_and_at_most_three_buttons() {
        for page in &PAGES {
            assert!(!page.rows[0].name.is_empty());
            for r in page.rows {
                assert!(!r.does.is_empty() && r.icons.len() <= 3, "{}", r.does);
            }
        }
    }

    /// Measures every line with the install's own fonts (`TIDEWATER_INSTALL`; skipped without one): each name and
    /// meaning fits its column, and the rows end above the buttons' tap targets.
    #[test]
    fn every_line_fits_its_column_in_the_real_font() {
        let Some(root) = std::env::var_os("TIDEWATER_INSTALL") else {
            eprintln!("skipped: set TIDEWATER_INSTALL to run");
            return;
        };
        let install = tidewater_formats::install::Install::open(&root).expect("the install opens");
        let front = crate::frontend::Front::load(install).expect("the fonts load");
        let (big, font) = (&front.big, &front.font);
        assert!(big.width(TITLE.as_bytes()) <= PANEL.w - 2 * INSET as u32);
        for (n, page) in PAGES.iter().enumerate() {
            let l = Layout::of(big, font, n);
            let cols = columns(n);
            let right = PANEL.x + PANEL.w as i32 - INSET;
            for r in page.rows {
                let icons_end = cols.icons + r.icons.len() as i32 * (ICON as i32 + ICON_GAP);
                assert!(icons_end <= cols.name, "{}: the buttons run into the names", r.name);
                let name_end = cols.name + font.width(r.name.as_bytes()) as i32;
                assert!(
                    name_end + 8 <= cols.does,
                    "{:?} is {} too wide",
                    r.name,
                    name_end + 8 - cols.does
                );
                let does_end = cols.does + font.width(r.does.as_bytes()) as i32;
                assert!(does_end <= right, "{:?} is {} too wide", r.does, does_end - right);
            }
            assert!(PITCH[n] >= l.row_h, "page {n}: the rows overlap");
            for &(lead, rest) in page.tip {
                let end = cols.icons + font.width(lead.as_bytes()) as i32 + font.width(rest.as_bytes()) as i32;
                assert!(end <= right, "{lead:?}{rest:?} is {} too wide", end - right);
            }
            let bottom = l.bottom(n);
            let buttons_top = BUTTONS_Y - HIT_PAD;
            assert!(
                bottom <= buttons_top,
                "page {n} ends at {bottom}, under the buttons at {buttons_top}"
            );
            for (t, b) in buttons(n) {
                assert!(font.width(t.label().as_bytes()) + 16 <= b.w);
            }
        }
    }
}
