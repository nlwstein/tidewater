//! The menu screens' rules (spec `docs/specs/ui/screen-and-cursors.md`, "Menu screens"; the fades in
//! `docs/specs/ui/palette-effects.md`, "Screen fades"): which screens share the menu picture
//! (`campaign::MENU_PICTURE`), when it is loaded again, which switches fade, and where the version line shows; and
//! how their buttons and text are drawn in the picture's palette entries ("Text colours", "Buttons"). SDL-free and
//! engine-free; `frontend` opens the screens by these rules.
//!
//! The main menu and every menu reached from it without starting a game are drawn on one picture in its own palette.
//! A switch between them replaces the buttons and text only: the picture isn't loaded again and nothing fades. The
//! custom-scenario setup loads the picture again, fades out and in over 200 ms and has no version line; the main
//! menu, opened from any screen not on the picture, fades in over 200 ms. Tidewater has only the main menu and the
//! campaign menu yet; the other rows are kept whole for the screens to come.

use tidewater_formats::fnt::Font;
use tidewater_formats::tileset::Palette;

use crate::hud::{self, Rect, TextStyle};
use crate::input::Key;
use crate::palette::fades;
use crate::scene::Picture;

/// A screen drawn on the menu picture ("Menu screens", "Which screens").
#[allow(dead_code)] // the spec's table is kept whole; Tidewater has only the main menu and the campaign menu yet
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuScreen {
    Main,
    SinglePlayer,
    /// The campaign menu (Tides of Darkness or Beyond the Dark Portal, orc or human); ours is `AppState::NewCampaign`.
    Campaign,
    LoadGame,
    /// The multiplayer menus up to the direct-connection setup (Enhanced or Classic choice, connection method).
    Multiplayer,
    /// A small dialog over these menus (name entry, "Unable to locate IPX network.").
    SmallDialog,
    /// The custom-scenario setup ("Custom Game Setup").
    CustomSetup,
    /// Tidewater's own Settings screen (`settings`): not in the original, opened like the custom-scenario setup (it
    /// reloads the picture and fades, with no version line).
    Settings,
}

/// How a menu screen opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Opening {
    /// The menu picture is loaded (again), with its full palette.
    pub reload: bool,
    /// The fade-in's length in ms; 0 for none.
    pub fade_in_ms: u32,
    /// The version line is drawn over the picture.
    pub version_line: bool,
}

impl MenuScreen {
    /// Whether the screen shows on the shared picture without loading it again: every menu screen but the
    /// custom-scenario setup.
    pub(crate) fn shares_picture(self) -> bool {
        !matches!(self, MenuScreen::CustomSetup | MenuScreen::Settings)
    }

    /// How the screen opens after `from`, the menu screen before it (`None`: a screen not on the menu picture, such
    /// as the title, the credits or a game).
    pub(crate) fn opening(self, from: Option<MenuScreen>) -> Opening {
        let switch = from.is_some_and(|f| !switch_fades(f, self));
        Opening {
            reload: !switch,
            fade_in_ms: if switch { 0 } else { fades::MENUS },
            version_line: self.shares_picture(),
        }
    }
}

/// Whether a switch from one menu screen to another fades out and in (200 ms each): only to or from the
/// custom-scenario setup ("Which screens"; `ui/palette-effects.md`, "Screen fades").
pub(crate) fn switch_fades(from: MenuScreen, to: MenuScreen) -> bool {
    !(from.shares_picture() && to.shares_picture())
}

/// Whether the pointer stays shown on the frame a screen asks to switch from `from` to `to`: on a switch between menus
/// on the shared picture, which has no fade (spec "Open questions", "Pointer during menu switches without a fade":
/// the default, the orc arrow stays shown). Every other switch hides it from that frame ("Pointer and fades").
pub(crate) fn pointer_stays(from: Option<MenuScreen>, to: Option<MenuScreen>) -> bool {
    matches!((from, to), (Some(f), Some(t)) if !switch_fades(f, t))
}

// ---------------------------------------------------------------------------------------------------------------
// Buttons and text ("Menu screens", "Text colours" and "Buttons")

/// A font's shades 1 to 4 as entries of the menu palette; shade 5, the shadow, is always entry 0 ("Text colours").
pub(crate) type ShadeMap = [u8; 4];

/// Button labels and dialog text: gold.
pub(crate) const GOLD: ShadeMap = [200, 199, 197, 192];
/// The label's hotkey letter, the whole label while the pointer is over the button, and the screen headings: white.
pub(crate) const WHITE: ShadeMap = [246, 246, 108, 104];
/// A disabled button's label: grey.
pub(crate) const GREY: ShadeMap = [108, 108, 105, 102];
/// The shadow's entry (shade 5), and the button's inner ring: black.
pub(crate) const BLACK: u8 = 0;

/// The focused button's outline, drawn over the face's outer ring ("Buttons"), a dialog's default button's too.
pub(crate) const FOCUS_RING: u8 = 251;
/// A dialog's default button's outline, drawn over the face's outer ring, enabled or disabled ("Buttons", "Default
/// button", round 75). Every other button keeps its frame's own ring, disabled ones included.
pub(crate) const DEFAULT_RING: u8 = 247;
/// The entries of a face's own outer ring (game data, "Buttons"): the 1-pixel border of frames 15 to 17.
pub(crate) const OUTER_RING: std::ops::RangeInclusive<u8> = 128..=136;
/// The entry of a face's second ring, 1 pixel inside the outer one.
pub(crate) const SECOND_RING: u8 = 239;
/// Our stand-in face's entries where the install lacks the interface file: enabled, disabled.
const STAND_IN_FACE: (u8, u8) = (162, 102);

/// The interface file whose frames are the menu buttons' faces ("Buttons"); its frames are stored uncompressed
/// (`tidewater_formats::grp::Grp::parse_uncompressed`).
pub(crate) const FACES_FILE: &str = "Art\\orc.grp";

/// One button size's three faces in the interface file ("Buttons", the frame table): from `first` on, the disabled,
/// normal and pressed faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FaceSet {
    pub first: usize,
    pub w: u32,
    pub h: u32,
}

/// The frame table's sizes ours draws faces for ("Buttons"). Which set a dialog uses comes from its dialog file,
/// which isn't read; frames 0 to 2 are also 106 × 28 but have no rings, so ours takes 9 to 11 for that size.
pub(crate) const FACE_SETS: [FaceSet; 6] = [
    FaceSet {
        first: 3,
        w: 128,
        h: 20,
    },
    FaceSet { first: 6, w: 80, h: 20 },
    FaceSet {
        first: 9,
        w: 106,
        h: 28,
    },
    FaceSet {
        first: 12,
        w: 164,
        h: 28,
    },
    // The main, single-player and campaign menus.
    FaceSet {
        first: 15,
        w: 224,
        h: 28,
    },
    FaceSet {
        first: 48,
        w: 80,
        h: 15,
    },
];

/// The face set for a button of `w` × `h`, if the table has one.
pub(crate) fn face_set(w: u32, h: u32) -> Option<FaceSet> {
    FACE_SETS.into_iter().find(|f| (f.w, f.h) == (w, h))
}

/// Which of a set's three faces a state shows: 0 disabled, 2 pressed (the mouse button held on it), 1 otherwise.
pub(crate) fn face_index(s: ButtonState) -> usize {
    match s {
        ButtonState { enabled: false, .. } => 0,
        ButtonState { pressed: true, .. } => 2,
        _ => 1,
    }
}

/// A button face: `w` × `h` entries of the menu palette, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Face {
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u8>,
}

/// The interface file's frames as button faces, by frame index.
#[derive(Clone, Debug, Default)]
pub(crate) struct ButtonFaces(pub Vec<Face>);

impl ButtonFaces {
    pub(crate) fn from_grp(g: &tidewater_formats::grp::Grp) -> Self {
        ButtonFaces(
            g.frames
                .iter()
                .map(|f| Face {
                    w: f.width as u32,
                    h: f.height as u32,
                    pixels: f.pixels.clone(),
                })
                .collect(),
        )
    }

    /// The face a button of `w` × `h` shows in state `s`; `None` if the size has no faces or the file lacks the frame.
    pub(crate) fn face(&self, w: u32, h: u32, s: ButtonState) -> Option<&Face> {
        let set = face_set(w, h)?;
        self.0
            .get(set.first + face_index(s))
            .filter(|f| (f.w, f.h) == (w, h) && f.pixels.len() == (w * h) as usize)
    }
}

/// Draws a face with its top-left at `at`, each pixel its menu palette entry.
fn draw_face(dst: &mut Picture, pal: &Palette, face: &Face, at: (i32, i32)) {
    for y in 0..face.h as i32 {
        let dy = at.1 + y;
        if dy < 0 || dy >= dst.height as i32 {
            continue;
        }
        for x in 0..face.w as i32 {
            let dx = at.0 + x;
            if dx < 0 || dx >= dst.width as i32 {
                continue;
            }
            let c = pal.0[face.pixels[(y * face.w as i32 + x) as usize] as usize];
            let d = ((dy as u32 * dst.width + dx as u32) * 4) as usize;
            dst.rgba[d..d + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
}

/// The pixels a space leaves between the glyphs either side of it ("Text colours"): `font12x` (what [`label_gap`]
/// gives it), and `font10x`.
#[cfg(test)]
pub(crate) const SPACE_12X: u32 = 10;
pub(crate) const SPACE_10X: u32 = 8;

/// The version line's shades ("Text colours"): the white map, so its face is entry 246 (255, 251, 243) with grey
/// (111, 111, 111) edges in entry 104 at shade 4 (the original's text has no shade-3 pixels; ours takes the map's 108).
pub(crate) const VERSION: ShadeMap = WHITE;
/// The version line's glyph cell top-left on the 640 × 480 menu picture, in `font10x` ("Pointer and fades", round 73).
pub(crate) const VERSION_CELL: (i32, i32) = (60, 460);

/// The main menu's buttons ("Buttons", from `rez\MainMenu.bin`): 224 × 28, left edge 208, tops 36 apart from 240.
pub(crate) const BUTTON_W: u32 = 224;
pub(crate) const BUTTON_H: u32 = 28;
pub(crate) const BUTTON_X: i32 = 208;
pub(crate) const BUTTON_TOP: i32 = 240;
pub(crate) const BUTTON_PITCH: i32 = 36;

/// The advance of a space that leaves `gap` pixels: a glyph already leaves 1 pixel after itself.
pub(crate) fn space_advance(gap: u32) -> u32 {
    gap.saturating_sub(1)
}

/// A shade map's eight colours by font shade in `pal`: shades 1 to 4 from the map, the shadow (5) and the unused shades
/// black (entry 0).
pub(crate) fn colours(pal: &Palette, map: ShadeMap) -> [[u8; 3]; 8] {
    let mut c = [pal.0[BLACK as usize]; 8];
    for (i, &e) in map.iter().enumerate() {
        c[i + 1] = pal.0[e as usize];
    }
    c
}

/// The rectangles of the main menu's `n` buttons: 224 × 28 at x 208, tops 36 apart from 240.
pub(crate) fn button_rects(n: usize) -> Vec<Rect> {
    (0..n)
        .map(|i| Rect::new(BUTTON_X, BUTTON_TOP + i as i32 * BUTTON_PITCH, BUTTON_W, BUTTON_H))
        .collect()
}

/// A 224 × 28 menu button at x 208 with its top at `y`.
const fn column(y: i32) -> Rect {
    Rect::new(BUTTON_X, y, BUTTON_W, BUTTON_H)
}

/// The campaign menu's headings ("Main menu and campaign menu", from `rez\newcmpgn.bin`): white, on 640 × 28
/// rectangles.
pub(crate) const CAMPAIGN_HEADINGS: [(Rect, &str); 2] = [
    (Rect::new(0, 216, 640, 28), "Tides of Darkness"),
    (Rect::new(0, 324, 640, 28), "Beyond the Dark Portal"),
];

/// The campaign menu's buttons with their hotkeys: Tides of Darkness orc and human, Beyond the Dark Portal orc and
/// human (`campaign::Choice::ALL`'s order), then Previous Menu.
pub(crate) const CAMPAIGN_BUTTONS: [(Rect, &str, char); 5] = [
    (column(240), "Orc Campaign", 'O'),
    (column(276), "Human Campaign", 'H'),
    (column(348), "Orc Campaign", 'r'),
    (column(384), "Human Campaign", 'u'),
    (column(420), "Previous Menu", 'P'),
];

/// The single-player menu's buttons with their hotkeys (`rez\snglplay.bin`: New Campaign, Load Game, Custom Scenario,
/// Previous Menu). Load Game's rectangle is observed (x 208, y 276); the others follow the main menu's 36-pixel pitch.
// spec gap: the dialog file's rectangles and hotkeys for the other three buttons aren't in the spec; N, C and P are ours
// (the campaign menu's Previous Menu is P) and Previous Menu sits in the fourth slot.
pub(crate) const SINGLE_PLAYER_BUTTONS: [(Rect, &str, char); 4] = [
    (column(240), "New Campaign", 'N'),
    (column(276), "Load Game", 'L'),
    (column(312), "Custom Scenario", 'C'),
    (column(348), "Previous Menu", 'P'),
];

/// The pixels a space leaves in a label or heading ("Label placement"): its advance is floor(the font's widest glyph /
/// 2) + 1 (9 in `font12x`, leaving the 10 of [`SPACE_12X`]).
pub(crate) fn label_gap(font: &Font) -> u32 {
    font.max_width as u32 / 2 + 2
}

/// A label's or heading's text width W ("Label placement"): the sum of its characters' advances (the glyph's x
/// offset, its width and 1), a space's being [`label_gap`]'s. The pixel after the last glyph counts.
pub(crate) fn label_width(font: &Font, s: &str) -> u32 {
    style(&[[0; 3]; 8], label_gap(font)).width(font, s)
}

/// Where a button label's text cell has its top-left ("Label placement"): x = left + floor((width + 1) / 2) + 1 −
/// floor(W / 2), y = top + floor((height + 1) / 2) + 1 − floor(font height / 2), 2 right and 2 down while pressed. For
/// a 224 × 28 button in `font12x` (17 high) that is (left + 113 − floor(W / 2), top + 7).
pub(crate) fn label_cell(r: Rect, text_w: u32, font_h: u32, pressed: bool) -> (i32, i32) {
    let p = if pressed { 2 } else { 0 };
    (
        r.x + (r.w as i32 + 1) / 2 + 1 - (text_w / 2) as i32 + p,
        r.y + (r.h as i32 + 1) / 2 + 1 - (font_h / 2) as i32 + p,
    )
}

/// Where a heading's text cell has its top-left ("Label placement", headings): x = left + floor((width − 1) / 2) −
/// floor(W / 2), y = top.
pub(crate) fn heading_cell(r: Rect, text_w: u32) -> (i32, i32) {
    (r.x + (r.w as i32 - 1) / 2 - (text_w / 2) as i32, r.y)
}

/// The mouse button held on a button ("Pressing"): the one it went down on, while it stays down. While held, the
/// button is drawn pressed with the pointer on it and normal with the pointer off it; releasing on it chooses it,
/// releasing anywhere else chooses nothing. Passing over another button while held doesn't press that one (round 75).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hold<T>(Option<T>);

impl<T> Default for Hold<T> {
    fn default() -> Self {
        Hold(None)
    }
}

impl<T: Copy + PartialEq> Hold<T> {
    /// The left button went down with the pointer over `hot` (an enabled button, or nothing).
    pub(crate) fn down(&mut self, hot: Option<T>) {
        self.0 = hot;
    }

    /// The button drawn pressed, with the pointer over `hot`: the held one, while the pointer is on it.
    pub(crate) fn pressed(self, hot: Option<T>) -> Option<T> {
        self.0.filter(|&h| hot == Some(h))
    }

    /// The left button came up with the pointer over `hot`: the held button, chosen if the pointer is on it.
    pub(crate) fn up(&mut self, hot: Option<T>) -> Option<T> {
        self.0.take().filter(|&h| hot == Some(h))
    }
}

/// A keypress that moves the focus ring ("Keyboard focus").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusKey {
    /// Tab: the next button in the dialog's order, from the last back to the first.
    Next,
    /// Shift+Tab: the one before, from the first to the last.
    Back,
}

/// The pad's focus step for the arrow key `k`, taken once. A pad's d-pad up or down in a menu arrives both as an arrow
/// key and as `pad` (`Input::pad_focus`); a screen that gives the arrows a meaning of its own calls this where it wants
/// the pad to move the focus ring instead. A keyboard's arrows leave `pad` empty, so they keep their meaning.
pub(crate) fn pad_step(pad: &mut Option<FocusKey>, k: Key) -> Option<FocusKey> {
    match (k, *pad) {
        (Key::Up, Some(FocusKey::Back)) | (Key::Down, Some(FocusKey::Next)) => pad.take(),
        _ => None,
    }
}

/// Where the focus goes from `focus` on `key` among the dialog's Tab stops, `stops` in its Tab order (screen spec,
/// "Keyboard focus", round 75: buttons, sliders, checkboxes, each radio button, lists and text fields, hidden controls
/// left out), from the last back to the first and the other way. A focus not among them goes to the first stop (or,
/// on Shift+Tab, the last).
pub(crate) fn step_focus<T: Copy + PartialEq>(stops: &[T], focus: T, key: FocusKey) -> T {
    let n = stops.len();
    if n == 0 {
        return focus;
    }
    let i = match (stops.iter().position(|&s| s == focus), key) {
        (Some(i), FocusKey::Next) => (i + 1) % n,
        (Some(i), FocusKey::Back) => (i + n - 1) % n,
        (None, FocusKey::Next) => 0,
        (None, FocusKey::Back) => n - 1,
    };
    stops[i]
}

/// Where the focus goes from `focus` on `key`, among buttons that are `enabled` (the dialog's order). A disabled
/// button is passed over (the spec saw none on the main menu; ours, as a disabled button can't be chosen). The arrow
/// keys and the pointer don't move the focus, so they have no `FocusKey`.
pub(crate) fn move_focus(enabled: &[bool], focus: usize, key: FocusKey) -> usize {
    let n = enabled.len();
    if n == 0 {
        return focus;
    }
    let step = |i: usize| match key {
        FocusKey::Next => (i + 1) % n,
        FocusKey::Back => (i + n - 1) % n,
    };
    let mut i = step(focus.min(n - 1));
    for _ in 0..n {
        if enabled[i] {
            return i;
        }
        i = step(i);
    }
    focus
}

/// The byte index of a label's hotkey letter: its first letter equal to `key`, ignoring case.
pub(crate) fn hotkey_at(label: &str, key: char) -> Option<usize> {
    label
        .char_indices()
        .find(|(_, c)| c.eq_ignore_ascii_case(&key))
        .map(|(i, _)| i)
}

/// A menu button's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ButtonState {
    pub enabled: bool,
    /// The pointer is over it.
    pub hover: bool,
    /// It has the focus: the yellow ring, which stays when the pointer is over another button.
    pub focused: bool,
    /// The mouse button is held on it: the pressed face, the label 2 right and 2 down.
    pub pressed: bool,
    /// It is the dialog's default button (the dialog file marks it): the 247 outline.
    pub default: bool,
}

/// The label's shade map ("Text colours"): grey when disabled, white while the pointer is over it, gold otherwise.
pub(crate) fn label_map(s: ButtonState) -> ShadeMap {
    match s {
        ButtonState { enabled: false, .. } => GREY,
        ButtonState { hover: true, .. } => WHITE,
        _ => GOLD,
    }
}

/// The outline drawn over the face's outer ring ("Buttons"): yellow when focused, light grey on the dialog's default
/// button, enabled or disabled, none otherwise, the face's own ring showing (a disabled button's too).
pub(crate) fn ring(s: ButtonState) -> Option<u8> {
    match s {
        ButtonState { focused: true, .. } => Some(FOCUS_RING),
        ButtonState { default: true, .. } => Some(DEFAULT_RING),
        _ => None,
    }
}

/// One line of `font12x` text in a shade map.
fn style<'a>(colours: &'a [[u8; 3]; 8], gap: u32) -> TextStyle<'a> {
    TextStyle {
        colours,
        space: Some(space_advance(gap)),
        rows: (i32::MIN, i32::MAX),
    }
}

/// Draws `s` with its cell's top-left at `at` in the shade map, spaces leaving `gap` pixels.
pub(crate) fn draw_text(
    dst: &mut Picture,
    font: &Font,
    pal: &Palette,
    at: (i32, i32),
    s: &str,
    map: ShadeMap,
    gap: u32,
) {
    let c = colours(pal, map);
    hud::text_styled(dst, font, at, s, &style(&c, gap));
}

/// Draws a heading in the white map on its rectangle `r` ("Label placement", headings).
pub(crate) fn draw_heading(dst: &mut Picture, font: &Font, pal: &Palette, r: Rect, s: &str) {
    let at = heading_cell(r, label_width(font, s));
    draw_text(dst, font, pal, at, s, WHITE, label_gap(font));
}

/// The Settings link on the main menu's bottom row, right of the version line: a tap target (not part of the original,
/// like the version line). 120 × 28 so a finger hits it; the text is right-aligned inside, ending at
/// [`SETTINGS_LINK_RIGHT`]. It clears the update offer's buttons (`frontend::UPDATE_BUTTONS`, which end at x 490).
pub(crate) const SETTINGS_LINK: Rect = Rect::new(504, 448, 120, 28);
/// Where the link's text ends.
pub(crate) const SETTINGS_LINK_RIGHT: i32 = 612;
/// The link's text.
pub(crate) const SETTINGS_LINK_TEXT: &str = "Settings (F2)";

/// The Touch controls link (ours, like the Settings link): above it, right-aligned the same way, clear of the update
/// notice line (`frontend::UPDATE_NOTICE`, y 427 to 445); shown only while touch is in use.
pub(crate) const TOUCH_LINK: Rect = Rect::new(504, 392, 120, 28);
/// Its text.
pub(crate) const TOUCH_LINK_TEXT: &str = "Touch controls";

/// The width of `s` in `font` with spaces leaving `gap` pixels.
pub(crate) fn text_width(font: &Font, s: &str, gap: u32) -> u32 {
    style(&[[0; 3]; 8], gap).width(font, s)
}

/// Draws a main-menu link in `font10x` on its rectangle `r` ([`SETTINGS_LINK`] on the version line's row, or
/// [`TOUCH_LINK`]): right-aligned to end at [`SETTINGS_LINK_RIGHT`], as high in `r` as the Settings link's text is in
/// its own; white like the version line, gold while the pointer is over it.
pub(crate) fn draw_link(dst: &mut Picture, font: &Font, pal: &Palette, r: Rect, text: &str, hover: bool) {
    let w = text_width(font, text, SPACE_10X) as i32;
    let at = (SETTINGS_LINK_RIGHT - w, VERSION_CELL.1 + r.y - SETTINGS_LINK.y);
    draw_text(dst, font, pal, at, text, if hover { GOLD } else { VERSION }, SPACE_10X);
}

/// Draws the version line `s` in `font10x` with its glyph cell at [`VERSION_CELL`], spaces leaving 8 pixels.
pub(crate) fn draw_version_line(dst: &mut Picture, font: &Font, pal: &Palette, s: &str) {
    draw_text(dst, font, pal, VERSION_CELL, s, VERSION, SPACE_10X);
}

/// Draws a menu button ("Buttons"): its face for its size and state from the interface file (a flat stand-in with
/// the two rings where `faces` lacks it), the focus or disabled outline over the outer ring, and the label in
/// `font12x` by the label rule, its hotkey letter (`key`) in white on an enabled button.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_button(
    dst: &mut Picture,
    font: &Font,
    pal: &Palette,
    faces: Option<&ButtonFaces>,
    r: Rect,
    label: &str,
    key: Option<char>,
    s: ButtonState,
) {
    let rgb = |e: u8| pal.0[e as usize];
    match faces.and_then(|f| f.face(r.w, r.h, s)) {
        Some(face) => draw_face(dst, pal, face, (r.x, r.y)),
        None => {
            hud::outline(dst, r, rgb(*OUTER_RING.start()));
            let inner = Rect::new(r.x + 1, r.y + 1, r.w.saturating_sub(2), r.h.saturating_sub(2));
            hud::outline(dst, inner, rgb(SECOND_RING));
            let face = if s.enabled { STAND_IN_FACE.0 } else { STAND_IN_FACE.1 };
            hud::fill(
                dst,
                Rect::new(r.x + 2, r.y + 2, r.w.saturating_sub(4), r.h.saturating_sub(4)),
                rgb(face),
            );
        }
    }
    if let Some(e) = ring(s) {
        hud::outline(dst, r, rgb(e));
    }
    let map = label_map(s);
    let gap = label_gap(font);
    let at = label_cell(r, label_width(font, label), font.height as u32, s.enabled && s.pressed);
    draw_text(dst, font, pal, at, label, map, gap);
    if let Some(i) = key
        .filter(|_| s.enabled && map != WHITE)
        .and_then(|k| hotkey_at(label, k))
    {
        let x = at.0 + style(&[[0; 3]; 8], gap).width(font, &label[..i]) as i32;
        let end = i + label[i..].chars().next().map_or(1, char::len_utf8);
        draw_text(dst, font, pal, (x, at.1), &label[i..end], WHITE, gap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pad_step_matches_its_own_arrow_once() {
        let mut pad = Some(FocusKey::Next);
        assert_eq!(pad_step(&mut pad, Key::Up), None, "the other arrow is not the pad's");
        assert_eq!(pad_step(&mut pad, Key::Down), Some(FocusKey::Next));
        assert_eq!(pad_step(&mut pad, Key::Down), None, "taken once");
        assert_eq!(pad_step(&mut None, Key::Down), None, "a keyboard's arrow");
    }

    use MenuScreen::*;

    const SHARED: [MenuScreen; 6] = [Main, SinglePlayer, Campaign, LoadGame, Multiplayer, SmallDialog];

    /// A synthetic palette whose every entry is a different colour.
    fn palette() -> Palette {
        let mut p = Palette([[0; 3]; 256]);
        for (i, c) in p.0.iter_mut().enumerate() {
            *c = [i as u8, (i / 2) as u8, 255 - i as u8];
        }
        p
    }

    /// A synthetic font: every glyph from '!' a 2 × 3 box at (0, 0) in its cell with shades 1 2 / 3 4 / 5 5; the space
    /// has no glyph.
    fn font() -> Font {
        let g = tidewater_formats::fnt::Glyph {
            width: 2,
            height: 3,
            x: 0,
            y: 0,
            shades: vec![1, 2, 3, 4, 5, 5],
        };
        let mut glyphs = vec![None];
        glyphs.extend((33..=126).map(|_| Some(g.clone())));
        Font {
            first: 32,
            max_width: 2,
            height: 5,
            glyphs,
        }
    }

    fn px(p: &Picture, x: i32, y: i32) -> [u8; 3] {
        let i = ((y as u32 * p.width + x as u32) * 4) as usize;
        [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2]]
    }

    fn blank(w: u32, h: u32) -> Picture {
        Picture {
            width: w,
            height: h,
            rgba: vec![0; (w * h * 4) as usize],
        }
    }

    const NORMAL: ButtonState = ButtonState {
        enabled: true,
        hover: false,
        focused: false,
        pressed: false,
        default: false,
    };

    /// Synthetic faces: every frame of the table's sizes, frame `i` filled with entry 10 + `i` and its border with
    /// entry 130.
    fn faces() -> ButtonFaces {
        let mut v: Vec<Face> = (0..51)
            .map(|_| Face {
                w: 1,
                h: 1,
                pixels: vec![0],
            })
            .collect();
        for set in FACE_SETS {
            for k in 0..3 {
                let i = set.first + k;
                let (w, h) = (set.w, set.h);
                let pixels = (0..w * h)
                    .map(|p| {
                        let (x, y) = (p % w, p / w);
                        if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                            130
                        } else {
                            10 + i as u8
                        }
                    })
                    .collect();
                v[i] = Face { w, h, pixels };
            }
        }
        ButtonFaces(v)
    }

    #[test]
    fn the_text_colours_are_the_menu_palettes_entries() {
        // "Text colours": shades 1 to 4 of each kind of text; shade 5 is entry 0.
        assert_eq!(GOLD, [200, 199, 197, 192], "button labels, dialog text");
        assert_eq!(WHITE, [246, 246, 108, 104], "hotkey letter, hovered label, headings");
        assert_eq!(GREY, [108, 108, 105, 102], "disabled label");
        let pal = palette();
        let c = colours(&pal, GOLD);
        assert_eq!(c[1..5], [pal.0[200], pal.0[199], pal.0[197], pal.0[192]]);
        assert_eq!(c[5], pal.0[0], "the black shadow");
        assert_eq!(space_advance(SPACE_12X), 9, "a space leaves 10 pixels in font12x");
    }

    #[test]
    fn a_buttons_label_and_ring_follow_its_state() {
        assert_eq!((label_map(NORMAL), ring(NORMAL)), (GOLD, None), "the face's own ring");
        let hover = ButtonState { hover: true, ..NORMAL };
        assert_eq!(label_map(hover), WHITE, "the whole label white under the pointer");
        assert_eq!(ring(hover), None, "hover doesn't move the focus");
        let focused = ButtonState {
            focused: true,
            ..NORMAL
        };
        assert_eq!(
            (label_map(focused), ring(focused)),
            (GOLD, Some(251)),
            "the yellow ring stays on the focused button with the pointer elsewhere"
        );
        let disabled = ButtonState {
            enabled: false,
            ..NORMAL
        };
        assert_eq!(
            (label_map(disabled), ring(disabled)),
            (GREY, None),
            "a disabled button that isn't the default keeps its frame's ring (round 75: Delete, Up)"
        );
        let default = ButtonState {
            default: true,
            ..NORMAL
        };
        assert_eq!(ring(default), Some(247), "the default button's outline");
        let disabled_default = ButtonState {
            default: true,
            ..disabled
        };
        assert_eq!(
            ring(disabled_default),
            Some(247),
            "disabled, the default keeps it (round 75: Save with no name, Load with no saved game)"
        );
        let both = ButtonState {
            focused: true,
            ..disabled_default
        };
        assert_eq!(
            ring(both),
            Some(251),
            "focus draws over the default's outline (round 74: Quit to Menu)"
        );
    }

    #[test]
    fn each_state_takes_its_face_from_the_frame_table() {
        // "Buttons": 224 × 28 is frames 15 (disabled), 16 (normal) and 17 (pressed).
        assert_eq!(face_set(224, 28).map(|s| s.first), Some(15));
        assert_eq!(face_set(106, 28).map(|s| s.first), Some(9), "the ringed 106 × 28 set");
        assert_eq!(face_set(80, 15).map(|s| s.first), Some(48));
        assert_eq!(face_set(200, 28), None);
        let pressed = ButtonState {
            pressed: true,
            ..NORMAL
        };
        let disabled = ButtonState {
            enabled: false,
            pressed: true,
            ..NORMAL
        };
        assert_eq!(
            [face_index(disabled), face_index(NORMAL), face_index(pressed)],
            [0, 1, 2]
        );
        let f = faces();
        assert_eq!(f.face(224, 28, NORMAL).map(|f| f.pixels[225]), Some(26));
        assert_eq!(f.face(224, 28, pressed).map(|f| f.pixels[225]), Some(27));
        assert_eq!(f.face(224, 28, disabled).map(|f| f.pixels[225]), Some(25));
        assert!(
            ButtonFaces::default().face(224, 28, NORMAL).is_none(),
            "no file: no face"
        );
    }

    #[test]
    fn the_main_menus_buttons_are_its_dialog_files() {
        // "Buttons": 224 × 28 at x 208, y 240, 276, 312, 348 and 384.
        let r = button_rects(5);
        assert_eq!(r.iter().map(|r| r.y).collect::<Vec<_>>(), [240, 276, 312, 348, 384]);
        assert!(r.iter().all(|r| (r.x, r.w, r.h) == (208, 224, 28)));
    }

    #[test]
    fn labels_follow_the_placement_rule() {
        // "Label placement", round 74: a 224 × 28 button in font12x (17 high) puts the cell at (left + 113 − W / 2,
        // top + 7).
        let at = |y: i32, w: u32, pressed: bool| label_cell(Rect::new(208, y, 224, 28), w, 17, pressed);
        assert_eq!(at(240, 166, false), (238, 247), "Single Player Game");
        assert_eq!(at(276, 160, false), (241, 283), "Multi Player Game");
        assert_eq!(at(312, 168, false), (237, 319), "Replay Introduction");
        assert_eq!(at(348, 114, false), (264, 355), "Show Credits");
        assert_eq!(at(384, 116, false), (263, 391), "Exit Program");
        assert_eq!(at(348, 114, true), (266, 357), "held: 2 right and 2 down");
        assert_eq!(at(276, 93, false), (275, 283), "Load Game on the single-player menu");
        // The general rule on another size and font.
        assert_eq!(label_cell(Rect::new(10, 20, 80, 15), 30, 10, false), (36, 24));
    }

    #[test]
    fn the_campaign_menu_is_its_dialog_files_layout() {
        let tops: Vec<i32> = CAMPAIGN_BUTTONS.iter().map(|b| b.0.y).collect();
        assert_eq!(tops, [240, 276, 348, 384, 420]);
        assert!(CAMPAIGN_BUTTONS.iter().all(|b| (b.0.x, b.0.w, b.0.h) == (208, 224, 28)));
        let keys: Vec<Option<usize>> = CAMPAIGN_BUTTONS.iter().map(|b| hotkey_at(b.1, b.2)).collect();
        assert_eq!(
            keys,
            [Some(0), Some(0), Some(1), Some(1), Some(0)],
            "O, H, r, u and P are in their labels"
        );
        // Headings W 156 and 201 at the observed cells.
        assert_eq!(heading_cell(CAMPAIGN_HEADINGS[0].0, 156), (241, 216));
        assert_eq!(heading_cell(CAMPAIGN_HEADINGS[1].0, 201), (219, 324));
        // The label cells observed are each button's top + 7 and left + 113 − floor(W / 2).
        let cells = [(262, 247), (248, 283), (262, 355), (248, 391), (257, 427)];
        for (b, cell) in CAMPAIGN_BUTTONS.iter().zip(cells) {
            let half = b.0.x + 113 - cell.0;
            assert_eq!(label_cell(b.0, half as u32 * 2, 17, false), cell, "{}", b.1);
        }
    }

    #[test]
    fn a_held_button_shows_pressed_only_with_the_pointer_on_it_and_chooses_on_release_there() {
        // "Pressing", round 75: held on Single Player Game (0), moved off, back on, released: chosen.
        let mut h = Hold::default();
        h.down(Some(0));
        assert_eq!(h.pressed(Some(0)), Some(0), "pressed face");
        assert_eq!(h.pressed(None), None, "off the button: drawn normal");
        assert_eq!(
            h.pressed(Some(1)),
            None,
            "passing over another button doesn't press that one"
        );
        assert_eq!(h.pressed(Some(0)), Some(0), "back on: pressed again");
        assert_eq!(h.up(Some(0)), Some(0), "released on it: chosen");
        assert_eq!(h, Hold::default(), "and let go");
        // Released off it, or on another button: nothing is chosen.
        h.down(Some(0));
        assert_eq!(h.up(None), None);
        h.down(Some(0));
        assert_eq!(h.up(Some(1)), None);
        // Pressed where no button is, then released on one: nothing either.
        h.down(None);
        assert_eq!(h.up(Some(0)), None);
    }

    #[test]
    fn tab_steps_through_a_dialogs_stops_in_their_order_and_wraps() {
        // "Keyboard focus", round 75: the load-game screen's Load, Cancel, the list, then Load again.
        let stops = ["load", "cancel", "list"];
        assert_eq!(step_focus(&stops, "load", FocusKey::Next), "cancel");
        assert_eq!(step_focus(&stops, "cancel", FocusKey::Next), "list");
        assert_eq!(step_focus(&stops, "list", FocusKey::Next), "load");
        assert_eq!(step_focus(&stops, "load", FocusKey::Back), "list");
        // A focus that isn't a stop (gone hidden) goes to the first, or the last on Shift+Tab.
        assert_eq!(step_focus(&stops, "gone", FocusKey::Next), "load");
        assert_eq!(step_focus(&stops, "gone", FocusKey::Back), "list");
        assert_eq!(step_focus(&[], "x", FocusKey::Next), "x");
    }

    #[test]
    fn tab_and_shift_tab_move_the_focus_and_wrap() {
        // "Keyboard focus", round 74: from Single Player Game, Tab 5 times visits the other four, then the first;
        // Shift+Tab from the first goes to Exit Program.
        let all = [true; 5];
        let mut f = 0;
        let mut seen = Vec::new();
        for _ in 0..5 {
            f = move_focus(&all, f, FocusKey::Next);
            seen.push(f);
        }
        assert_eq!(seen, [1, 2, 3, 4, 0]);
        assert_eq!(move_focus(&all, 0, FocusKey::Back), 4);
        assert_eq!(move_focus(&all, 3, FocusKey::Back), 2);
        // Ours: a disabled button is passed over; with none enabled the focus stays.
        let some = [true, false, true];
        assert_eq!(move_focus(&some, 0, FocusKey::Next), 2);
        assert_eq!(move_focus(&some, 2, FocusKey::Next), 0);
        assert_eq!(move_focus(&some, 0, FocusKey::Back), 2);
        assert_eq!(move_focus(&[false, false], 1, FocusKey::Next), 1);
        assert_eq!(move_focus(&[], 0, FocusKey::Next), 0);
    }

    #[test]
    fn headings_use_their_own_rule() {
        // The campaign menu's headings on 640 × 28 rectangles: x = left + 319 − W / 2, y = top.
        assert_eq!(heading_cell(Rect::new(0, 216, 640, 28), 156), (241, 216));
        assert_eq!(heading_cell(Rect::new(0, 324, 640, 28), 201), (219, 324));
    }

    #[test]
    fn a_labels_width_counts_every_advance() {
        // The synthetic font: glyphs advance 3, the space floor(2 / 2) + 1 = 2.
        let f = font();
        assert_eq!(label_gap(&f), 3);
        assert_eq!(label_width(&f, "AB"), 6, "the pixel after the last glyph counts");
        assert_eq!(label_width(&f, "A B"), 8);
    }

    #[test]
    fn the_hotkey_letter_is_the_first_match_ignoring_case() {
        assert_eq!(hotkey_at("Load Game", 'L'), Some(0));
        assert_eq!(hotkey_at("Show Credits", 'C'), Some(5));
        assert_eq!(hotkey_at("Quit", 'x'), None);
    }

    #[test]
    fn a_button_is_drawn_from_its_face_in_the_menu_palette() {
        let (pal, f, fc) = (palette(), font(), faces());
        let r = Rect::new(10, 10, 80, 20);
        let draw = |s: ButtonState| {
            let mut p = blank(100, 40);
            draw_button(&mut p, &f, &pal, Some(&fc), r, "AB", Some('b'), s);
            p
        };
        // "AB" is W 6 in the synthetic font (5 high): its cell is at (10 + 40 + 1 − 3, 10 + 10 + 1 − 2) = (48, 19).
        let p = draw(NORMAL);
        assert_eq!(px(&p, 10, 10), pal.0[130], "the face's own outer ring");
        assert_eq!(px(&p, 12, 12), pal.0[17], "the normal face, frame 7");
        assert_eq!(px(&p, 48, 19), pal.0[200], "A: gold shade 1");
        assert_eq!(px(&p, 48, 20), pal.0[197], "A: gold shade 3");
        assert_eq!(px(&p, 48, 21), pal.0[0], "shadow");
        assert_eq!(px(&p, 51, 19), pal.0[246], "B, the hotkey: white shade 1");
        assert_eq!(px(&p, 52, 20), pal.0[104], "B: white shade 4");
        let p = draw(ButtonState {
            focused: true,
            ..NORMAL
        });
        assert_eq!(px(&p, 10, 10), pal.0[251], "focus ring over the outer ring");
        assert_eq!(px(&p, 89, 29), pal.0[251]);
        assert_eq!(px(&p, 11, 11), pal.0[17], "the face inside it");
        let p = draw(ButtonState { hover: true, ..NORMAL });
        assert_eq!(px(&p, 48, 19), pal.0[246], "hovered: the whole label white");
        let p = draw(ButtonState {
            pressed: true,
            ..NORMAL
        });
        assert_eq!(px(&p, 12, 12), pal.0[18], "the pressed face, frame 8");
        assert_eq!(px(&p, 50, 21), pal.0[200], "the label 2 right and 2 down");
        let p = draw(ButtonState {
            enabled: false,
            ..NORMAL
        });
        assert_eq!(
            px(&p, 10, 10),
            pal.0[130],
            "disabled, not the default: the face's own ring"
        );
        assert_eq!(px(&p, 12, 12), pal.0[16], "the disabled face, frame 6");
        assert_eq!(px(&p, 48, 19), pal.0[108], "grey label");
        assert_eq!(px(&p, 49, 20), pal.0[102]);
        assert_eq!(px(&p, 51, 19), pal.0[108], "no white hotkey on a disabled button");
        // Without the file: the stand-in, with the rings.
        let mut p = blank(100, 40);
        draw_button(&mut p, &f, &pal, None, r, "AB", None, NORMAL);
        assert_eq!(px(&p, 10, 10), pal.0[128]);
        assert_eq!(px(&p, 11, 11), pal.0[239]);
        assert_eq!(px(&p, 12, 12), pal.0[162]);
    }

    #[test]
    fn the_version_line_has_its_cell_at_60_460() {
        // "Pointer and fades" (round 73): font10x, glyph cell top-left (60, 460), 1 pixel between glyphs and 8 at a
        // space; shades 1 and 2 entry 246, 4 entry 104 (the grey edges), 5 black.
        assert_eq!(VERSION_CELL, (60, 460));
        assert_eq!(space_advance(SPACE_10X), 7);
        let (pal, f) = (palette(), font());
        let mut p = blank(640, 480);
        draw_version_line(&mut p, &f, &pal, "A B");
        assert_eq!(px(&p, 60, 460), pal.0[246], "shade 1 at the cell's top-left");
        assert_eq!(px(&p, 61, 460), pal.0[246], "shade 2");
        assert_eq!(px(&p, 61, 461), pal.0[104], "shade 4: grey");
        assert_eq!(px(&p, 60, 462), pal.0[0], "shade 5: black");
        // "A" is 2 pixels wide and leaves 1, the space 7 more: 8 pixels between "A" and "B".
        assert_eq!(px(&p, 70, 460), pal.0[246], "B");
        assert!((62..70).all(|x| p.rgba[((460 * 640 + x) * 4 + 3) as usize] == 0));
        let inked = |x: u32, y: u32| p.rgba[((y * p.width + x) * 4 + 3) as usize] != 0;
        assert!(!(0..640).any(|x| inked(x, 459)), "nothing above row 460");
        assert!(!(0..480).any(|y| inked(59, y)), "nothing left of x 60");
    }

    #[test]
    fn a_heading_is_white_on_its_rectangle() {
        let (pal, f) = (palette(), font());
        let mut p = blank(64, 20);
        // "AB" is W 6: left edge 0 + 63 / 2 − 3 = 28, at the rectangle's top.
        draw_heading(&mut p, &f, &pal, Rect::new(0, 4, 64, 10), "AB");
        assert_eq!(px(&p, 28, 4), pal.0[246]);
        assert_eq!(px(&p, 28, 5), pal.0[108]);
        assert_eq!(px(&p, 27, 4), [0, 0, 0]);
    }

    #[test]
    fn menus_on_the_shared_picture_switch_without_a_fade_or_a_reload() {
        // Round 73: main menu to single player or multiplayer, campaign menu back to single player, single player to
        // load game: same picture, no darker frame, the version line stays.
        for from in SHARED {
            for to in SHARED {
                assert!(!switch_fades(from, to), "{from:?} -> {to:?}");
                assert_eq!(
                    to.opening(Some(from)),
                    Opening {
                        reload: false,
                        fade_in_ms: 0,
                        version_line: true
                    },
                    "{from:?} -> {to:?}"
                );
                assert!(pointer_stays(Some(from), Some(to)), "{from:?} -> {to:?}");
            }
        }
    }

    #[test]
    fn the_main_menu_fades_in_from_other_screens() {
        // After the title, the credits or a game: loaded with its palette and a 200 ms fade-in, version line shown.
        assert_eq!(
            Main.opening(None),
            Opening {
                reload: true,
                fade_in_ms: 200,
                version_line: true
            }
        );
        // Custom-scenario setup, Cancel: fades out and back into the main menu.
        assert_eq!(Main.opening(Some(CustomSetup)), Main.opening(None));
        assert!(switch_fades(CustomSetup, Main));
    }

    #[test]
    fn the_custom_scenario_setup_reloads_the_picture_and_fades_with_no_version_line() {
        for from in SHARED.map(Some).into_iter().chain([None]) {
            assert_eq!(
                CustomSetup.opening(from),
                Opening {
                    reload: true,
                    fade_in_ms: 200,
                    version_line: false
                },
                "{from:?}"
            );
        }
        assert!(
            switch_fades(SinglePlayer, CustomSetup),
            "Custom Scenario on the single-player menu"
        );
        assert!(!CustomSetup.shares_picture());
    }

    #[test]
    fn the_settings_screen_opens_like_the_custom_scenario_setup() {
        for from in SHARED.map(Some).into_iter().chain([None]) {
            assert_eq!(
                Settings.opening(from),
                Opening {
                    reload: true,
                    fade_in_ms: 200,
                    version_line: false
                },
                "{from:?}"
            );
        }
        assert!(!Settings.shares_picture());
        assert!(switch_fades(Main, Settings) && switch_fades(Settings, Main));
        assert_eq!(Main.opening(Some(Settings)), Main.opening(None));
    }

    #[test]
    fn the_settings_link_is_a_finger_sized_target_on_the_version_lines_row() {
        let l = SETTINGS_LINK;
        assert!(l.h >= 24 && l.w >= 24);
        assert!(l.x >= 0 && (l.x as u32 + l.w) <= 640 && (l.y as u32 + l.h) <= 480);
        // On the version line's row, right of the version line's room (it starts at x 60 and a line of 24 letters of
        // `font10x` ends well before x 300), and its text ends inside the target.
        assert!(l.y <= VERSION_CELL.1 && VERSION_CELL.1 < l.y + l.h as i32);
        assert!(l.x > 300);
        assert!(SETTINGS_LINK_RIGHT <= l.x + l.w as i32);
        // The text at its widest still starts inside the target: 13 characters of at most 8 pixels.
        assert!(SETTINGS_LINK_RIGHT - 13 * 8 >= l.x);
    }

    #[test]
    fn the_pointer_hides_on_every_switch_that_fades() {
        // A menu button that changes screen with a fade (Custom Scenario; Show Credits leaves the menus).
        assert!(!pointer_stays(Some(SinglePlayer), Some(CustomSetup)));
        assert!(!pointer_stays(Some(CustomSetup), Some(Main)));
        assert!(!pointer_stays(Some(Main), None), "Show Credits");
        assert!(!pointer_stays(None, Some(Main)));
        assert!(!pointer_stays(None, None));
    }
}
