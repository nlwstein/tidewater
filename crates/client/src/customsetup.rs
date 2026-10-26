//! The single-player Custom Scenario screen, "Custom Game Setup" (spec `docs/specs/ui/game-setup-lobby.md`,
//! "Single-player Custom Scenario screen" and "The Select scenario dialog"; the choices' effects are
//! `docs/specs/game/setup.md`, "Custom choices"): the map list, the five option boxes, the scenario box, the Select
//! scenario dialog with its filters, and the start parameters handed to `Setup::Custom`. SDL-free and engine-free:
//! `frontend` opens the screen, feeds it the frame's input ([`Input`]) and draws what [`Screen::draw`] gives.
//!
//! The dialog files (`rez\loadcstm.bin`, `rez\filefltr.bin`) aren't read, so the screen's own boxes and labels use the
//! spec's coordinates and the Select scenario dialog has a layout of our own (the spec gives none for it).
//!
//! Our readings where the spec is silent (each marked `// spec gap:` where it is used):
//! - The list of built-in scenarios isn't known (the archives list no file names), so "Built-in scenario" is offered
//!   only when a built-in list exists, which it doesn't yet; the dialog's Type box then offers "Custom scenario" only.
//! - Custom scenarios are every `.pud` under the `Maps` folder, subfolders included, named by their path below `Maps`
//!   without the extension (a map in the folder itself is just its file name).
//! - Your Race lists Human, Orc and Map Default, and starts on Map Default, as every other box starts on its default.
//! - The dialog shows the map's terrain as a fourth info line; the Options start on their defaults each time the
//!   screen opens (only the map pick is remembered, as the spec says).

use tidewater_formats::fnt::Font;
use tidewater_formats::pud::{Era, Pud};
use tidewater_formats::tileset::Palette;
use tidewater_sim::setup::{Custom, Game, Resources, Setup, TerrainChoice, race};

use crate::hud::{self, Rect};
use crate::input::Key;
use crate::menuscreen::{self, ButtonFaces, ButtonState, Hold};
use crate::scene::Picture;

/// The folder the custom scenarios are in.
pub(crate) const MAPS_DIR: &str = "Maps";

// ---------------------------------------------------------------------------------------------------------------
// The maps

/// Which list the Select scenario dialog shows ("Type").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapKind {
    BuiltIn,
    Custom,
}

impl MapKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            MapKind::BuiltIn => "Built-in scenario",
            MapKind::Custom => "Custom scenario",
        }
    }
}

/// A scenario the dialog can list, with what the info lines and the filters read from its header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MapInfo {
    pub kind: MapKind,
    /// The file's name in the installation, to start it from (`Maps\Dark Paths BNE.pud`).
    pub path: String,
    /// The list's line: the path below `Maps` without the extension.
    pub name: String,
    /// The Scenario box's second line: the path below `Maps` with the extension.
    pub file: String,
    pub description: String,
    /// The map is square: its side in tiles.
    pub size: u16,
    pub era: Era,
    /// Playable slots: slots 0 to 7 with owner code 4 or 5.
    pub players: u8,
    /// Slots 0 to 7 with owner code 5 (a slot for a human).
    pub humans: u8,
    /// Slots 0 to 7 with owner code 4 (a computer).
    pub computers: u8,
}

/// The owner codes of a slot (`game/setup.md`): a computer and a human.
const COMPUTER: u8 = 4;
const HUMAN: u8 = 5;

impl MapInfo {
    /// The scenario a map file's bytes describe, or `None` for a file that fails the header check ("A file that fails the
    /// header check is left out"). `path` is the name in the installation, below the [`MAPS_DIR`] folder.
    pub(crate) fn from_bytes(path: &str, data: &[u8]) -> Option<MapInfo> {
        let pud = Pud::parse(data).ok()?;
        let owners = pud.owners();
        let count = |code: u8| owners.iter().take(8).filter(|&&o| o == code).count() as u8;
        let rel = path.split_once(['\\', '/']).map_or(path, |(_, rest)| rest);
        let name = rel.rsplit_once('.').map_or(rel, |(stem, _)| stem);
        Some(MapInfo {
            kind: MapKind::Custom,
            path: path.to_string(),
            name: name.to_string(),
            file: rel.to_string(),
            description: pud.description(),
            size: pud.width,
            era: pud.era,
            players: count(COMPUTER) + count(HUMAN),
            humans: count(HUMAN),
            computers: count(COMPUTER),
        })
    }
}

/// The scenarios among the installation's files `names` (the [`MAPS_DIR`] folder's `.pud` files, subfolders included),
/// read with `read`, sorted by name ignoring case.
pub(crate) fn scan_maps(names: &[String], mut read: impl FnMut(&str) -> Option<Vec<u8>>) -> Vec<MapInfo> {
    let mut maps: Vec<MapInfo> = names
        .iter()
        .filter(|n| n.to_ascii_lowercase().ends_with(".pud"))
        .filter_map(|n| MapInfo::from_bytes(n, &read(n)?))
        .collect();
    maps.sort_by_cached_key(|m| (m.name.to_ascii_lowercase(), m.name.clone()));
    maps
}

/// The Players filter's lines, in the dialog's order ("2 to 8 players", "2 to 4 players", "5 to 8 players", then "1
/// player" to "8 players").
pub(crate) const PLAYER_FILTERS: [(&str, u8, u8); 11] = [
    ("2 to 8 players", 2, 8),
    ("2 to 4 players", 2, 4),
    ("5 to 8 players", 5, 8),
    ("1 player", 1, 1),
    ("2 players", 2, 2),
    ("3 players", 3, 3),
    ("4 players", 4, 4),
    ("5 players", 5, 5),
    ("6 players", 6, 6),
    ("7 players", 7, 7),
    ("8 players", 8, 8),
];

/// The Map size filter's lines: "Any size", then the four sizes.
pub(crate) const SIZE_FILTERS: [(&str, u16); 5] = [
    ("Any size", 0),
    ("32 x 32", 32),
    ("64 x 64", 64),
    ("96 x 96", 96),
    ("128 x 128", 128),
];

/// The Select scenario dialog's three filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Filter {
    pub kind: MapKind,
    /// An index into [`PLAYER_FILTERS`].
    pub players: usize,
    /// An index into [`SIZE_FILTERS`].
    pub size: usize,
}

impl Filter {
    /// The dialog's first choices.
    pub(crate) const DEFAULT: Filter = Filter {
        kind: MapKind::Custom,
        players: 0,
        size: 0,
    };

    /// Whether a map is listed: its type, its playable slots in the Players range, its square size equal to the choice.
    pub(crate) fn passes(&self, m: &MapInfo) -> bool {
        let (_, lo, hi) = PLAYER_FILTERS[self.players];
        let want = SIZE_FILTERS[self.size].1;
        m.kind == self.kind && (lo..=hi).contains(&m.players) && (want == 0 || m.size == want)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The options

/// The screen's five option boxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opt {
    Race,
    Opponents,
    Resources,
    Tileset,
    Units,
}

impl Opt {
    pub(crate) const ALL: [Opt; 5] = [Opt::Race, Opt::Opponents, Opt::Resources, Opt::Tileset, Opt::Units];

    /// The box's choices in the dialog's order (`rez\chatroom.tbl`); Enhanced rules add Random last to Resources and Map
    /// Tileset (single player is always Enhanced).
    pub(crate) fn labels(self) -> &'static [&'static str] {
        match self {
            // spec gap: the spec lists "Human, Orc or Map Default" without saying where Map Default sits.
            Opt::Race => &["Human", "Orc", "Map Default"],
            Opt::Opponents => &[
                "Map Default",
                "1 Opponent",
                "2 Opponents",
                "3 Opponents",
                "4 Opponents",
                "5 Opponents",
                "6 Opponents",
                "7 Opponents",
            ],
            Opt::Resources => &["Map Default", "Low", "Medium", "High", "Random"],
            Opt::Tileset => &["Map Default", "Forest", "Winter", "Wasteland", "Orc Swamp", "Random"],
            Opt::Units => &["Map Default", "One Peasant Only"],
        }
    }

    fn title(self) -> &'static str {
        match self {
            Opt::Race => "Your Race:",
            Opt::Opponents => "Opponents:",
            Opt::Resources => "Resources:",
            Opt::Tileset => "Map Tileset:",
            Opt::Units => "Units:",
        }
    }

    fn drop_rect(self) -> Rect {
        match self {
            Opt::Race => Rect::new(44, 274, 124, 24),
            Opt::Opponents => Rect::new(44, 324, 124, 24),
            Opt::Resources => Rect::new(224, 274, 144, 24),
            Opt::Tileset => Rect::new(224, 324, 144, 24),
            Opt::Units => Rect::new(404, 274, 180, 24),
        }
    }

    /// The label's cell, 18 pixels above its box.
    fn label_at(self) -> (i32, i32) {
        let r = self.drop_rect();
        (r.x - 4, r.y - 18)
    }
}

/// The choice shown in a box: an index into [`Opt::labels`].
pub(crate) fn choice(c: &Custom, o: Opt) -> usize {
    match o {
        Opt::Race => match c.race {
            Some(race::HUMAN) => 0,
            Some(_) => 1,
            None => 2,
        },
        Opt::Opponents => c.opponents.map_or(0, |n| n.min(7) as usize),
        Opt::Resources => match c.resources {
            Resources::MapDefault => 0,
            Resources::Low => 1,
            Resources::Medium => 2,
            Resources::High => 3,
            Resources::Random => 4,
        },
        Opt::Tileset => match c.terrain {
            TerrainChoice::MapDefault => 0,
            TerrainChoice::Set(Era::Forest) => 1,
            TerrainChoice::Set(Era::Winter) => 2,
            TerrainChoice::Set(Era::Wasteland) => 3,
            TerrainChoice::Set(Era::Swamp) => 4,
            TerrainChoice::Random => 5,
        },
        Opt::Units => usize::from(c.one_worker),
    }
}

/// Picks choice `i` of a box; an index past its choices does nothing.
pub(crate) fn set_choice(c: &mut Custom, o: Opt, i: usize) {
    if i >= o.labels().len() {
        return;
    }
    match o {
        Opt::Race => c.race = [Some(race::HUMAN), Some(race::ORC), None][i],
        Opt::Opponents => c.opponents = (i > 0).then_some(i as u8),
        Opt::Resources => {
            c.resources = [
                Resources::MapDefault,
                Resources::Low,
                Resources::Medium,
                Resources::High,
                Resources::Random,
            ][i]
        }
        Opt::Tileset => {
            c.terrain = [
                TerrainChoice::MapDefault,
                TerrainChoice::Set(Era::Forest),
                TerrainChoice::Set(Era::Winter),
                TerrainChoice::Set(Era::Wasteland),
                TerrainChoice::Set(Era::Swamp),
                TerrainChoice::Random,
            ][i]
        }
        Opt::Units => c.one_worker = i == 1,
    }
}

/// Moves a box to the next (`step` 1) or previous (`step` -1) choice, wrapping round.
pub(crate) fn cycle_choice(c: &mut Custom, o: Opt, step: i32) {
    let n = o.labels().len() as i32;
    let i = (choice(c, o) as i32 + step).rem_euclid(n);
    set_choice(c, o, i as usize);
}

// ---------------------------------------------------------------------------------------------------------------
// The screen's rules

/// What the screen asks for after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Stay,
    /// Cancel Game: back to the main menu.
    Cancel,
    /// Start Game: [`Screen::start`] gives the game.
    Start,
}

/// One drop-down box, on the screen or in the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Drop {
    Opt(Opt),
    Type,
    Players,
    Size,
}

/// Something the pointer can be on, or the focus can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Drop(Drop),
    SelectScenario,
    Start,
    Cancel,
    /// A line of the open drop-down's list.
    PopupRow(usize),
    /// A line of the dialog's scenario list (an index into the shown lines).
    ListRow(usize),
    /// The dialog's scroll bar track.
    Track(bool),
    DialogCancel,
    DialogOk,
}

/// The frame's input, in screen pixels.
#[derive(Clone, Debug, Default)]
pub(crate) struct Input {
    pub at: Option<(i32, i32)>,
    /// The left button went down / came up this frame.
    pub down: bool,
    pub up: bool,
    /// Keys that went down this frame.
    pub keys: Vec<Key>,
    pub shift: bool,
    /// Ctrl or Alt held (the Settings screen's Keys page reads the chord a key press makes).
    pub ctrl: bool,
    pub alt: bool,
    /// Wheel lines, positive away from the user.
    pub wheel: i32,
    /// The key events that went down this frame in order, with the text each types: for the screens with a text box.
    pub typed: Vec<crate::input::KeyEvent>,
    /// Ctrl or Alt is held (a combination is never typed into a text box).
    pub command_held: bool,
    /// A pad's d-pad up or down this frame as a focus step (`input::Input::pad_focus`); its arrow key is in `keys` too.
    pub pad_focus: Option<menuscreen::FocusKey>,
}

/// An open drop-down list.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Popup {
    owner: Drop,
    labels: Vec<String>,
    /// The highlighted line.
    at: usize,
}

/// The Select scenario dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Dialog {
    filter: Filter,
    /// The listed maps (indices into [`Screen::maps`]).
    list: Vec<usize>,
    /// The selected line of `list`.
    sel: usize,
    /// The first shown line.
    top: usize,
    focus: Target,
}

const DIALOG_LINES: usize = 6;

pub(crate) struct Screen {
    maps: Vec<MapInfo>,
    /// The scenario picked (an index into `maps`).
    chosen: Option<usize>,
    pub custom: Custom,
    filter: Filter,
    focus: Target,
    popup: Option<Popup>,
    dialog: Option<Dialog>,
    hot: Option<Target>,
    held: Hold<Target>,
    /// Something to look at changed since the last [`Screen::take_changed`].
    changed: bool,
}

/// The main layer's Tab stops, in order.
const STOPS: [Target; 8] = [
    Target::Drop(Drop::Opt(Opt::Race)),
    Target::Drop(Drop::Opt(Opt::Opponents)),
    Target::Drop(Drop::Opt(Opt::Resources)),
    Target::Drop(Drop::Opt(Opt::Tileset)),
    Target::Drop(Drop::Opt(Opt::Units)),
    Target::SelectScenario,
    Target::Start,
    Target::Cancel,
];

const DIALOG_STOPS: [Target; 5] = [
    Target::Drop(Drop::Type),
    Target::Drop(Drop::Players),
    Target::Drop(Drop::Size),
    Target::DialogCancel,
    Target::DialogOk,
];

// The screen's layout: the spec's coordinates ("Layout").
const TITLE: Rect = Rect::new(160, 220, 320, 20);
const SCENARIO_LABEL: (i32, i32) = (16, 402);
const SCENARIO_TEXT: (i32, i32) = (8, 424);
const SCENARIO_WIDTH: u32 = 380;
const BUTTONS: [(Target, i32, &str, char); 3] = [
    (Target::SelectScenario, 368, "Select Scenario", 'E'),
    (Target::Start, 404, "Start Game", 'S'),
    (Target::Cancel, 440, "Cancel Game", 'C'),
];
const BUTTON_X: i32 = 400;

// The dialog's layout (ours).
const PANEL: Rect = Rect::new(110, 64, 420, 352);
const DIALOG_DROPS: [(Drop, Rect, &str); 3] = [
    (Drop::Type, Rect::new(126, 116, 124, 24), "Type:"),
    (Drop::Players, Rect::new(262, 116, 140, 24), "Players:"),
    (Drop::Size, Rect::new(414, 116, 104, 24), "Map size:"),
];
const LIST: Rect = Rect::new(126, 172, 372, 120);
const TRACK: Rect = Rect::new(498, 172, 16, 120);
const LINE_H: i32 = 20;
const DIALOG_CANCEL: Rect = Rect::new(126, 384, 80, 20);
const DIALOG_OK: Rect = Rect::new(434, 384, 80, 20);
const POPUP_LINE: i32 = 20;

impl Screen {
    /// The screen as it opens: the options at their defaults and the scenario last picked in this session (`last`, a
    /// map's path), else the first scenario the dialog's first choices list. With no scenarios, none, and Start is grey.
    pub(crate) fn new(maps: Vec<MapInfo>, last: Option<&str>) -> Screen {
        // spec gap: "with none, the first built-in scenario": there is no built-in list, so the first custom one.
        let chosen = last
            .and_then(|l| maps.iter().position(|m| m.path == l))
            .or_else(|| maps.iter().position(|m| Filter::DEFAULT.passes(m)));
        Screen {
            maps,
            chosen,
            custom: Custom::default(),
            filter: Filter::DEFAULT,
            focus: STOPS[0],
            popup: None,
            dialog: None,
            hot: None,
            held: Hold::default(),
            changed: true,
        }
    }

    /// The scenario picked.
    pub(crate) fn map(&self) -> Option<&MapInfo> {
        self.chosen.map(|i| &self.maps[i])
    }

    /// Whether Start Game can be used: a scenario with a slot for a human, and at least one opponent.
    pub(crate) fn can_start(&self) -> bool {
        let Some(m) = self.map() else { return false };
        // spec gap: "at least one computer slot or an opponents count of 1 or more", but "map default" counts at least one
        // opponent (`game/setup.md`, custom step 3), so a chosen map with a human slot always starts.
        let opponents = self.custom.opponents.unwrap_or(m.computers.max(1));
        m.humans >= 1 && opponents >= 1
    }

    /// The game Start Game starts: the map's name in the installation and the options for `Setup`. Single-player
    /// custom games are Enhanced; fog stays on and fixed starts are never set (`ui/game-setup-lobby.md`).
    pub(crate) fn start(&self) -> Option<(String, Setup)> {
        let m = self.map().filter(|_| self.can_start())?;
        let custom = Custom {
            fixed_starts: false,
            fog_of_war: true,
            ..self.custom
        };
        Some((
            m.path.clone(),
            Setup {
                game: Game::Custom(custom),
                enhanced: true,
            },
        ))
    }

    /// The multiplayer lobby borrows the Select scenario dialog: whether the dialog (or a list over it) is open.
    pub(crate) fn picker_open(&self) -> bool {
        self.dialog.is_some() || self.popup.is_some()
    }

    /// Opens the Select scenario dialog (the lobby's Select Scenario button).
    pub(crate) fn open_picker(&mut self) {
        if self.dialog.is_none() {
            self.open_dialog();
        }
        self.changed = true;
    }

    /// Draws the dialog and the list over it, if open, onto `p` (the lobby draws its own controls under them).
    pub(crate) fn draw_picker(&self, p: &mut Picture, look: &Look) {
        if let Some(d) = &self.dialog {
            self.draw_dialog(p, look, d);
        }
        if let Some(pop) = &self.popup {
            self.draw_popup(p, look, pop);
        }
    }

    /// Whether the picture needs drawing again, and forget it does.
    pub(crate) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// The dialog's Type choices: the types that have a scenario, always the custom one.
    fn kinds(&self) -> Vec<MapKind> {
        let mut v = Vec::new();
        if self.maps.iter().any(|m| m.kind == MapKind::BuiltIn) {
            v.push(MapKind::BuiltIn);
        }
        v.push(MapKind::Custom);
        v
    }

    /// The maps a filter lists.
    fn listed(&self, f: &Filter) -> Vec<usize> {
        (0..self.maps.len()).filter(|&i| f.passes(&self.maps[i])).collect()
    }

    fn open_dialog(&mut self) {
        let filter = self.filter;
        let list = self.listed(&filter);
        let sel = self.chosen.and_then(|c| list.iter().position(|&i| i == c)).unwrap_or(0);
        self.dialog = Some(Dialog {
            filter,
            list,
            sel,
            top: (sel + 1).saturating_sub(DIALOG_LINES),
            focus: Target::DialogOk,
        });
    }

    /// Rebuilds the dialog's list after a filter changed and selects its first line.
    fn refilter(&mut self) {
        let Some(d) = &self.dialog else { return };
        let list = self.listed(&d.filter);
        if let Some(d) = &mut self.dialog {
            d.list = list;
            d.sel = 0;
            d.top = 0;
        }
    }

    /// The map the dialog's selected line is.
    fn selected(&self) -> Option<&MapInfo> {
        let d = self.dialog.as_ref()?;
        d.list.get(d.sel).map(|&i| &self.maps[i])
    }

    fn select_line(&mut self, line: usize) {
        if let Some(d) = &mut self.dialog
            && line < d.list.len()
        {
            d.sel = line;
            if line < d.top {
                d.top = line;
            } else if line >= d.top + DIALOG_LINES {
                d.top = line + 1 - DIALOG_LINES;
            }
        }
    }

    fn scroll_lines(&mut self, by: i32) {
        if let Some(d) = &mut self.dialog {
            let max = d.list.len().saturating_sub(DIALOG_LINES) as i32;
            d.top = (d.top as i32 + by).clamp(0, max) as usize;
        }
    }

    fn move_selection(&mut self, by: i32) {
        let Some(d) = &self.dialog else { return };
        if d.list.is_empty() {
            return;
        }
        let line = (d.sel as i32 + by).clamp(0, d.list.len() as i32 - 1) as usize;
        self.select_line(line);
    }

    // --- Geometry ----------------------------------------------------------------------------------------------

    fn drop_rect(&self, d: Drop) -> Rect {
        match d {
            Drop::Opt(o) => o.drop_rect(),
            d => DIALOG_DROPS
                .iter()
                .find(|(x, ..)| *x == d)
                .map_or_else(Rect::default, |&(_, r, _)| r),
        }
    }

    fn popup_rect(&self, p: &Popup) -> Rect {
        let b = self.drop_rect(p.owner);
        let h = p.labels.len() as u32 * POPUP_LINE as u32 + 2;
        let below = b.y + b.h as i32;
        let y = if below + h as i32 > 478 {
            (b.y - h as i32).max(0)
        } else {
            below
        };
        Rect::new(b.x, y, b.w, h)
    }

    fn button_rect(t: Target) -> Rect {
        let y = BUTTONS.iter().find(|b| b.0 == t).map_or(0, |b| b.1);
        Rect::new(BUTTON_X, y, menuscreen::BUTTON_W, menuscreen::BUTTON_H)
    }

    /// The targets of the layer that takes the pointer, topmost first, with their rectangles.
    fn targets(&self) -> Vec<(Target, Rect)> {
        let mut v = Vec::new();
        if let Some(p) = &self.popup {
            let r = self.popup_rect(p);
            for i in 0..p.labels.len() {
                v.push((
                    Target::PopupRow(i),
                    Rect::new(r.x + 1, r.y + 1 + i as i32 * POPUP_LINE, r.w - 2, POPUP_LINE as u32),
                ));
            }
        } else if let Some(d) = &self.dialog {
            for (dr, r, _) in DIALOG_DROPS {
                v.push((Target::Drop(dr), r));
            }
            for line in 0..DIALOG_LINES.min(d.list.len().saturating_sub(d.top)) {
                v.push((
                    Target::ListRow(d.top + line),
                    Rect::new(LIST.x, LIST.y + line as i32 * LINE_H, LIST.w, LINE_H as u32),
                ));
            }
            let thumb = Self::thumb(d);
            v.push((
                Target::Track(false),
                Rect::new(TRACK.x, TRACK.y, TRACK.w, (thumb.y - TRACK.y) as u32),
            ));
            v.push((
                Target::Track(true),
                Rect::new(
                    TRACK.x,
                    thumb.y + thumb.h as i32,
                    TRACK.w,
                    (TRACK.y + TRACK.h as i32 - thumb.y - thumb.h as i32).max(0) as u32,
                ),
            ));
            v.push((Target::DialogCancel, DIALOG_CANCEL));
            v.push((Target::DialogOk, DIALOG_OK));
        } else {
            for o in Opt::ALL {
                v.push((Target::Drop(Drop::Opt(o)), o.drop_rect()));
            }
            for (t, ..) in BUTTONS {
                if t != Target::Start || self.can_start() {
                    v.push((t, Self::button_rect(t)));
                }
            }
        }
        v
    }

    /// The scroll bar's thumb.
    fn thumb(d: &Dialog) -> Rect {
        let n = d.list.len().max(1);
        if n <= DIALOG_LINES {
            return Rect::new(TRACK.x, TRACK.y, TRACK.w, TRACK.h);
        }
        let h = (TRACK.h as usize * DIALOG_LINES / n).max(12) as u32;
        let span = TRACK.h - h;
        let y = TRACK.y + (span as usize * d.top / (n - DIALOG_LINES)) as i32;
        Rect::new(TRACK.x, y, TRACK.w, h)
    }

    // --- Input -------------------------------------------------------------------------------------------------

    /// One frame: the pointer, the left button and the keys.
    pub(crate) fn update(&mut self, input: &Input) -> Outcome {
        let before_hot = self.hot;
        let before_pressed = self.held.pressed(self.hot);
        self.hot = input.at.and_then(|(x, y)| {
            self.targets()
                .into_iter()
                .find(|(_, r)| r.contains(x as f32, y as f32))
                .map(|(t, _)| t)
        });
        if input.down {
            if self.popup.is_some() && !matches!(self.hot, Some(Target::PopupRow(_))) {
                // A press outside the open list closes it.
                self.popup = None;
                self.held = Hold::default();
                self.changed = true;
            } else {
                self.held.down(self.hot);
                if let Some(t @ (Target::Drop(_) | Target::SelectScenario | Target::Start | Target::Cancel)) = self.hot
                    && self.dialog.is_none()
                {
                    self.focus = t;
                }
            }
        }
        if input.wheel != 0 && self.popup.is_none() {
            self.scroll_lines(-input.wheel);
            self.changed = true;
        }
        let mut out = Outcome::Stay;
        if input.up
            && let Some(t) = self.held.up(self.hot)
        {
            out = self.activate(t);
        }
        if out == Outcome::Stay {
            out = self.keys(input);
        }
        if before_hot != self.hot || before_pressed != self.held.pressed(self.hot) {
            self.changed = true;
        }
        out
    }

    fn activate(&mut self, t: Target) -> Outcome {
        self.changed = true;
        match t {
            Target::Drop(d) => self.open_popup(d),
            Target::PopupRow(i) => self.choose(i),
            Target::SelectScenario => self.open_dialog(),
            Target::Start => {
                if self.can_start() {
                    return Outcome::Start;
                }
            }
            Target::Cancel => return Outcome::Cancel,
            Target::ListRow(i) => self.select_line(i),
            Target::Track(down) => self.scroll_lines(if down {
                DIALOG_LINES as i32
            } else {
                -(DIALOG_LINES as i32)
            }),
            Target::DialogCancel => self.dialog = None,
            Target::DialogOk => self.accept(),
        }
        Outcome::Stay
    }

    /// The dialog's OK: the selected line becomes the scenario, and the filters are kept.
    fn accept(&mut self) {
        let Some(d) = self.dialog.take() else { return };
        self.filter = d.filter;
        if let Some(&i) = d.list.get(d.sel) {
            self.chosen = Some(i);
        }
    }

    fn open_popup(&mut self, owner: Drop) {
        let (labels, at): (Vec<String>, usize) = match owner {
            Drop::Opt(o) => (
                o.labels().iter().map(|s| s.to_string()).collect(),
                choice(&self.custom, o),
            ),
            Drop::Type => {
                let kinds = self.kinds();
                let cur = self.dialog.as_ref().map_or(MapKind::Custom, |d| d.filter.kind);
                (
                    kinds.iter().map(|k| k.label().to_string()).collect(),
                    kinds.iter().position(|&k| k == cur).unwrap_or(0),
                )
            }
            Drop::Players => (
                PLAYER_FILTERS.iter().map(|f| f.0.to_string()).collect(),
                self.dialog.as_ref().map_or(0, |d| d.filter.players),
            ),
            Drop::Size => (
                SIZE_FILTERS.iter().map(|f| f.0.to_string()).collect(),
                self.dialog.as_ref().map_or(0, |d| d.filter.size),
            ),
        };
        self.popup = Some(Popup { owner, labels, at });
    }

    /// Picks line `i` of the open list.
    fn choose(&mut self, i: usize) {
        let Some(p) = self.popup.take() else { return };
        match p.owner {
            Drop::Opt(o) => set_choice(&mut self.custom, o, i),
            Drop::Type => {
                let kinds = self.kinds();
                if let (Some(d), Some(&k)) = (&mut self.dialog, kinds.get(i)) {
                    d.filter.kind = k;
                }
                self.refilter();
            }
            Drop::Players => {
                if let Some(d) = &mut self.dialog {
                    d.filter.players = i.min(PLAYER_FILTERS.len() - 1);
                }
                self.refilter();
            }
            Drop::Size => {
                if let Some(d) = &mut self.dialog {
                    d.filter.size = i.min(SIZE_FILTERS.len() - 1);
                }
                self.refilter();
            }
        }
    }

    fn keys(&mut self, input: &Input) -> Outcome {
        let mut out = Outcome::Stay;
        let mut pad = input.pad_focus;
        for &k in &input.keys {
            self.changed = true;
            // On the main screen the pad's up and down move the focus; its left and right still cycle an option.
            if self.popup.is_none()
                && self.dialog.is_none()
                && let Some(step) = menuscreen::pad_step(&mut pad, k)
            {
                self.focus = menuscreen::step_focus(&self.stops(), self.focus, step);
                continue;
            }
            out = if self.popup.is_some() {
                self.popup_key(k)
            } else if self.dialog.is_some() {
                self.dialog_key(k, input.shift)
            } else {
                self.main_key(k, input.shift)
            };
            if out != Outcome::Stay {
                break;
            }
        }
        out
    }

    fn popup_key(&mut self, k: Key) -> Outcome {
        let Some(p) = &mut self.popup else { return Outcome::Stay };
        let n = p.labels.len();
        match k {
            Key::Escape => self.popup = None,
            Key::Up => p.at = p.at.saturating_sub(1),
            Key::Down => p.at = (p.at + 1).min(n - 1),
            Key::Home => p.at = 0,
            Key::End => p.at = n - 1,
            Key::Enter | Key::NumpadEnter | Key::Space => {
                let at = p.at;
                self.choose(at);
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// The main screen's focus stops, Start only when a game can start.
    fn stops(&self) -> Vec<Target> {
        STOPS
            .iter()
            .copied()
            .filter(|&t| t != Target::Start || self.can_start())
            .collect()
    }

    fn main_key(&mut self, k: Key, shift: bool) -> Outcome {
        let stops = self.stops();
        match k {
            Key::Tab => {
                let key = if shift {
                    menuscreen::FocusKey::Back
                } else {
                    menuscreen::FocusKey::Next
                };
                self.focus = menuscreen::step_focus(&stops, self.focus, key);
            }
            Key::Escape => return Outcome::Cancel,
            Key::Enter | Key::NumpadEnter | Key::Space => return self.activate(self.focus),
            Key::Left | Key::Up | Key::Right | Key::Down => {
                if let Target::Drop(Drop::Opt(o)) = self.focus {
                    let step = if matches!(k, Key::Left | Key::Up) { -1 } else { 1 };
                    cycle_choice(&mut self.custom, o, step);
                }
            }
            Key::Letter(c) => {
                if let Some(&(t, ..)) = BUTTONS.iter().find(|b| b.3 == c.to_ascii_uppercase()) {
                    self.focus = t;
                    return self.activate(t);
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    fn dialog_key(&mut self, k: Key, shift: bool) -> Outcome {
        let Some(d) = &self.dialog else { return Outcome::Stay };
        let focus = d.focus;
        match k {
            Key::Escape => self.dialog = None,
            Key::Tab => {
                let key = if shift {
                    menuscreen::FocusKey::Back
                } else {
                    menuscreen::FocusKey::Next
                };
                let f = menuscreen::step_focus(&DIALOG_STOPS, focus, key);
                if let Some(d) = &mut self.dialog {
                    d.focus = f;
                }
            }
            Key::Enter | Key::NumpadEnter | Key::Space => {
                self.activate(focus);
            }
            Key::Up => self.move_selection(-1),
            Key::Down => self.move_selection(1),
            Key::PageUp => self.move_selection(-(DIALOG_LINES as i32)),
            Key::PageDown => self.move_selection(DIALOG_LINES as i32),
            Key::Home => self.move_selection(i32::MIN / 2),
            Key::End => self.move_selection(i32::MAX / 2),
            _ => {}
        }
        Outcome::Stay
    }

    // --- Drawing -----------------------------------------------------------------------------------------------

    /// The screen over `bg`, the menu picture.
    pub(crate) fn draw(&self, bg: &Picture, look: &Look) -> Picture {
        let mut p = Picture {
            width: bg.width,
            height: bg.height,
            rgba: bg.rgba.clone(),
        };
        draw_title(&mut p, look.font, look.pal);
        let modal = self.popup.is_some() || self.dialog.is_some();
        let hot = |t: Target| self.hot == Some(t);
        let pressed = |t: Target| self.held.pressed(self.hot) == Some(t);
        for o in Opt::ALL {
            let t = Target::Drop(Drop::Opt(o));
            look.label(&mut p, o.label_at(), o.title());
            let text = o.labels()[choice(&self.custom, o)];
            look.drop(
                &mut p,
                o.drop_rect(),
                text,
                DropState::of(!modal && hot(t), !modal && self.focus == t, !modal && pressed(t)),
            );
        }
        look.label(&mut p, SCENARIO_LABEL, "Scenario:");
        let lines = self.map().map_or_else(Vec::new, |m| {
            vec![m.kind.label().to_string(), m.file.clone(), m.description.clone()]
        });
        for (i, l) in lines.iter().enumerate() {
            let at = (SCENARIO_TEXT.0, SCENARIO_TEXT.1 + i as i32 * 17);
            look.text(&mut p, at, &fit(look.font, l, SCENARIO_WIDTH), menuscreen::GOLD);
        }
        for (t, _, label, key) in BUTTONS {
            let enabled = t != Target::Start || self.can_start();
            let s = ButtonState {
                enabled,
                hover: !modal && hot(t),
                focused: !modal && self.focus == t,
                pressed: !modal && pressed(t),
                default: false,
            };
            menuscreen::draw_button(
                &mut p,
                look.font,
                look.pal,
                Some(look.faces),
                Self::button_rect(t),
                label,
                Some(key),
                s,
            );
        }
        if let Some(d) = &self.dialog {
            self.draw_dialog(&mut p, look, d);
        }
        if let Some(pop) = &self.popup {
            self.draw_popup(&mut p, look, pop);
        }
        p
    }

    fn draw_dialog(&self, p: &mut Picture, look: &Look, d: &Dialog) {
        let rgb = |e: u8| look.pal.0[e as usize];
        hud::fill(p, PANEL, rgb(menuscreen::BLACK));
        hud::outline(p, PANEL, rgb(*menuscreen::OUTER_RING.start()));
        let inner = Rect::new(PANEL.x + 1, PANEL.y + 1, PANEL.w - 2, PANEL.h - 2);
        hud::outline(p, inner, rgb(menuscreen::SECOND_RING));
        menuscreen::draw_heading(
            p,
            look.font,
            look.pal,
            Rect::new(PANEL.x, PANEL.y + 8, PANEL.w, 20),
            "Select scenario",
        );
        let hot = |t: Target| self.hot == Some(t) && self.popup.is_none();
        let pressed = |t: Target| self.held.pressed(self.hot) == Some(t);
        for (dr, r, title) in DIALOG_DROPS {
            let t = Target::Drop(dr);
            look.label(p, (r.x, r.y - 20), title);
            let text = match dr {
                Drop::Type => d.filter.kind.label(),
                Drop::Players => PLAYER_FILTERS[d.filter.players].0,
                _ => SIZE_FILTERS[d.filter.size].0,
            };
            let modal = self.popup.is_some();
            look.drop(
                p,
                r,
                text,
                DropState::of(!modal && hot(t), !modal && d.focus == t, !modal && pressed(t)),
            );
        }
        if d.filter.kind == MapKind::Custom {
            look.label(p, (126, 146), "Folder:");
            look.text(p, (196, 146), MAPS_DIR, menuscreen::GOLD);
        }
        hud::fill(p, LIST, rgb(menuscreen::BLACK));
        hud::outline(p, LIST, rgb(*menuscreen::OUTER_RING.start()));
        for line in 0..DIALOG_LINES {
            let Some(&mi) = d.list.get(d.top + line) else { break };
            let r = Rect::new(
                LIST.x + 1,
                LIST.y + 1 + line as i32 * LINE_H - 1,
                LIST.w - 2,
                LINE_H as u32,
            );
            let on = d.top + line == d.sel;
            if on {
                hud::fill(p, r, rgb(162));
            }
            let map = if on || hot(Target::ListRow(d.top + line)) {
                menuscreen::WHITE
            } else {
                menuscreen::GOLD
            };
            look.text(
                p,
                (r.x + 4, r.y + 1),
                &fit(look.font, &self.maps[mi].name, LIST.w - 12),
                map,
            );
        }
        hud::fill(p, TRACK, rgb(menuscreen::BLACK));
        hud::outline(p, TRACK, rgb(*menuscreen::OUTER_RING.start()));
        let thumb = Self::thumb(d);
        hud::fill(
            p,
            Rect::new(thumb.x + 2, thumb.y + 2, thumb.w - 4, thumb.h.saturating_sub(4)),
            rgb(162),
        );
        // The info lines of the selected line: the description, the size, the players, and the terrain.
        if let Some(m) = self.selected() {
            let tileset = match m.era {
                Era::Forest => "Forest",
                Era::Winter => "Winter",
                Era::Wasteland => "Wasteland",
                Era::Swamp => "Orc Swamp",
            };
            let info = [
                m.description.clone(),
                format!("{} x {}", m.size, m.size),
                format!("{} player{}", m.players, if m.players == 1 { "" } else { "s" }),
                // spec gap: the spec's info lines are the description, the size and the players; the terrain is ours.
                tileset.to_string(),
            ];
            for (i, l) in info.iter().enumerate() {
                look.text(p, (126, 298 + i as i32 * 19), &fit(look.font, l, 388), menuscreen::GOLD);
            }
        }
        for (t, r, label, key) in [
            (Target::DialogCancel, DIALOG_CANCEL, "Cancel", 'C'),
            (Target::DialogOk, DIALOG_OK, "OK", 'O'),
        ] {
            let s = ButtonState {
                enabled: true,
                hover: hot(t),
                focused: d.focus == t && self.popup.is_none(),
                pressed: pressed(t) && hot(t),
                default: t == Target::DialogOk,
            };
            menuscreen::draw_button(p, look.font, look.pal, Some(look.faces), r, label, Some(key), s);
        }
    }

    fn draw_popup(&self, p: &mut Picture, look: &Look, pop: &Popup) {
        let rgb = |e: u8| look.pal.0[e as usize];
        let r = self.popup_rect(pop);
        hud::fill(p, r, rgb(menuscreen::BLACK));
        hud::outline(p, r, rgb(*menuscreen::OUTER_RING.start()));
        for (i, l) in pop.labels.iter().enumerate() {
            let row = Rect::new(r.x + 1, r.y + 1 + i as i32 * POPUP_LINE, r.w - 2, POPUP_LINE as u32);
            let on = pop.at == i || self.hot == Some(Target::PopupRow(i));
            if on {
                hud::fill(p, row, rgb(162));
            }
            let map = if on { menuscreen::WHITE } else { menuscreen::GOLD };
            look.text(
                p,
                (row.x + 6, row.y + (POPUP_LINE - look.font.height as i32) / 2),
                &fit(look.font, l, row.w - 12),
                map,
            );
        }
    }
}

/// The screen's title, "Custom Game Setup", on the picture.
pub(crate) fn draw_title(dst: &mut Picture, font: &Font, pal: &Palette) {
    menuscreen::draw_heading(dst, font, pal, TITLE, "Custom Game Setup");
}

/// The text of `s` cut short to fit `max` pixels of width.
pub(crate) fn fit(font: &Font, s: &str, max: u32) -> String {
    let mut out = s.to_string();
    while !out.is_empty() && menuscreen::label_width(font, &out) > max {
        out.pop();
    }
    out
}

/// The end of `s` cut short at the front to fit `max` pixels of width (a text box that has scrolled).
pub(crate) fn fit_tail(font: &Font, s: &str, max: u32) -> String {
    let mut out: &str = s;
    while let Some(c) = out.chars().next() {
        if menuscreen::label_width(font, out) <= max {
            break;
        }
        out = &out[c.len_utf8()..];
    }
    out.to_string()
}

/// What the screen is drawn with.
pub(crate) struct Look<'a> {
    pub font: &'a Font,
    pub pal: &'a Palette,
    pub faces: &'a ButtonFaces,
}

#[derive(Clone, Copy)]
pub(crate) struct DropState {
    hover: bool,
    focused: bool,
    pressed: bool,
    /// Shown but not changeable: grey text and no arrow.
    dim: bool,
}

impl DropState {
    pub(crate) fn of(hover: bool, focused: bool, pressed: bool) -> Self {
        DropState {
            hover,
            focused,
            pressed,
            dim: false,
        }
    }

    /// The box of a setting that cannot be changed now.
    pub(crate) fn dim(self) -> Self {
        DropState { dim: true, ..self }
    }
}

impl Look<'_> {
    pub(crate) fn text(&self, dst: &mut Picture, at: (i32, i32), s: &str, map: menuscreen::ShadeMap) {
        menuscreen::draw_text(dst, self.font, self.pal, at, s, map, menuscreen::label_gap(self.font));
    }

    pub(crate) fn label(&self, dst: &mut Picture, at: (i32, i32), s: &str) {
        self.text(dst, at, s, menuscreen::GOLD);
    }

    /// A drop-down box: the current choice and a down arrow, in a ring (yellow when it has the focus).
    pub(crate) fn drop(&self, dst: &mut Picture, r: Rect, text: &str, s: DropState) {
        let rgb = |e: u8| self.pal.0[e as usize];
        hud::fill(dst, r, rgb(menuscreen::BLACK));
        hud::outline(
            dst,
            r,
            rgb(if s.focused {
                menuscreen::FOCUS_RING
            } else {
                *menuscreen::OUTER_RING.start()
            }),
        );
        let inner = Rect::new(r.x + 1, r.y + 1, r.w - 2, r.h - 2);
        hud::outline(dst, inner, rgb(menuscreen::SECOND_RING));
        if s.pressed {
            hud::fill(dst, Rect::new(r.x + 2, r.y + 2, r.w - 4, r.h - 4), rgb(162));
        }
        let map = if s.dim {
            menuscreen::GREY
        } else if s.hover {
            menuscreen::WHITE
        } else {
            menuscreen::GOLD
        };
        let y = r.y + (r.h as i32 - self.font.height as i32) / 2;
        self.text(dst, (r.x + 4, y), &fit(self.font, text, r.w - 14), map);
        if s.dim {
            return;
        }
        let (cx, cy) = (r.x + r.w as i32 - 8, r.y + r.h as i32 / 2 - 2);
        for i in 0..4 {
            hud::fill(
                dst,
                Rect::new(cx - 3 + i, cy + i, (7 - 2 * i) as u32, 1),
                rgb(menuscreen::WHITE[0]),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use tidewater_formats::pud::build::Map;

    use super::*;

    /// A synthetic map: `owners` for slots 0 to 7 (the rest 3, nobody), a side of `size` tiles.
    fn pud(size: u16, owners: &[u8], desc: &str) -> Vec<u8> {
        let mut m = Map {
            size,
            ..Default::default()
        };
        m.owners[..owners.len()].copy_from_slice(owners);
        let mut bytes = m.bytes();
        // The description: the DESC section's payload is the first 32 bytes after its 8-byte header.
        let at = bytes.windows(4).position(|w| w == b"DESC").unwrap() + 8;
        bytes[at..at + 32].fill(0);
        bytes[at..at + desc.len()].copy_from_slice(desc.as_bytes());
        bytes
    }

    fn info(path: &str, size: u16, owners: &[u8]) -> MapInfo {
        MapInfo::from_bytes(path, &pud(size, owners, "Gold separates east from west")).unwrap()
    }

    /// Three maps: two players on 32, four on 64, eight on 128.
    fn maps() -> Vec<MapInfo> {
        vec![
            info("Maps\\b four.pud", 64, &[5, 5, 4, 4]),
            info("Maps\\A two.pud", 32, &[5, 4]),
            info("Maps\\Sub\\c eight.pud", 128, &[5, 5, 5, 5, 5, 5, 5, 5]),
        ]
    }

    fn sorted() -> Vec<MapInfo> {
        let m = maps();
        let names: Vec<String> = m.iter().map(|m| m.path.clone()).collect();
        scan_maps(&names, |n| {
            let i = m.iter().position(|x| x.path == n)?;
            Some(pud(m[i].size, &[5, 5, 5, 5, 5, 5, 5, 5][..m[i].players as usize], ""))
        })
    }

    fn click(s: &mut Screen, x: i32, y: i32) -> Outcome {
        s.update(&Input {
            at: Some((x, y)),
            down: true,
            ..Default::default()
        });
        s.update(&Input {
            at: Some((x, y)),
            up: true,
            ..Default::default()
        })
    }

    fn key(s: &mut Screen, k: Key) -> Outcome {
        s.update(&Input {
            keys: vec![k],
            ..Default::default()
        })
    }

    #[test]
    fn a_map_is_described_by_its_header() {
        let m = info("Maps\\Dark Paths BNE.pud", 128, &[5, 5, 5, 5, 4, 4, 4, 2, 5]);
        assert_eq!(
            (m.name.as_str(), m.file.as_str()),
            ("Dark Paths BNE", "Dark Paths BNE.pud")
        );
        // Slots 0 to 7 only: the ninth (code 5) doesn't count, and code 2 isn't playable.
        assert_eq!((m.players, m.humans, m.computers), (7, 4, 3));
        assert_eq!((m.size, m.description.as_str()), (128, "Gold separates east from west"));
        let sub = info("Maps\\Sub\\x.pud", 32, &[5]);
        assert_eq!((sub.name.as_str(), sub.file.as_str()), ("Sub\\x", "Sub\\x.pud"));
        assert!(MapInfo::from_bytes("Maps\\bad.pud", b"not a map").is_none());
    }

    #[test]
    fn the_list_is_sorted_ignoring_case_and_leaves_out_bad_files() {
        let names: Vec<String> = [
            "Maps\\b.pud",
            "Maps\\A.pud",
            "Maps\\readme.txt",
            "Maps\\bad.pud",
            "Maps\\C.PUD",
        ]
        .map(String::from)
        .to_vec();
        let ok = pud(32, &[5, 4], "");
        let got = scan_maps(&names, |n| (n != "Maps\\bad.pud").then(|| ok.clone()));
        let order: Vec<&str> = got.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(order, ["A", "b", "C"]);
    }

    #[test]
    fn filters_pick_by_playable_slots_and_square_size() {
        let m = maps();
        let pass = |players, size| {
            let f = Filter {
                kind: MapKind::Custom,
                players,
                size,
            };
            m.iter().filter(|x| f.passes(x)).map(|x| x.players).collect::<Vec<_>>()
        };
        assert_eq!(pass(0, 0), [4, 2, 8], "2 to 8 players, any size");
        assert_eq!(pass(1, 0), [4, 2], "2 to 4 players");
        assert_eq!(pass(2, 0), [8], "5 to 8 players");
        assert_eq!(pass(3, 0), Vec::<u8>::new(), "1 player");
        assert_eq!(pass(4, 0), [2], "2 players");
        assert_eq!(pass(10, 0), [8], "8 players");
        assert_eq!(pass(0, 2), [4], "64 x 64");
        assert_eq!(pass(0, 4), [8], "128 x 128");
        assert_eq!(pass(2, 1), Vec::<u8>::new(), "5 to 8 players on 32 x 32");
        let builtin = Filter {
            kind: MapKind::BuiltIn,
            ..Filter::DEFAULT
        };
        assert!(m.iter().all(|x| !builtin.passes(x)), "no built-in scenarios");
    }

    #[test]
    fn the_screen_opens_on_defaults_and_the_first_listed_map() {
        let s = Screen::new(sorted(), None);
        assert_eq!(s.custom, Custom::default());
        assert!(s.custom.fog_of_war && !s.custom.fixed_starts && s.custom.opponents.is_none());
        assert_eq!(s.map().map(|m| m.name.as_str()), Some("A two"));
        for o in Opt::ALL {
            assert_eq!(o.labels()[choice(&s.custom, o)], "Map Default");
        }
        // The map last picked in the session wins.
        let s = Screen::new(sorted(), Some("Maps\\Sub\\c eight.pud"));
        assert_eq!(s.map().map(|m| m.players), Some(8));
        assert!(Screen::new(Vec::new(), None).map().is_none());
        assert!(!Screen::new(Vec::new(), None).can_start());
    }

    #[test]
    fn the_options_have_the_specs_choices_in_its_order() {
        let n = |o: Opt| o.labels().to_vec();
        assert_eq!(n(Opt::Race), ["Human", "Orc", "Map Default"]);
        assert_eq!(n(Opt::Opponents)[..3], ["Map Default", "1 Opponent", "2 Opponents"]);
        assert_eq!(n(Opt::Opponents).len(), 8);
        assert_eq!(n(Opt::Resources), ["Map Default", "Low", "Medium", "High", "Random"]);
        assert_eq!(
            n(Opt::Tileset),
            ["Map Default", "Forest", "Winter", "Wasteland", "Orc Swamp", "Random"]
        );
        assert_eq!(n(Opt::Units), ["Map Default", "One Peasant Only"]);
    }

    #[test]
    fn cycling_wraps_and_every_choice_round_trips() {
        let mut c = Custom::default();
        cycle_choice(&mut c, Opt::Resources, 1);
        assert_eq!(c.resources, Resources::Low);
        cycle_choice(&mut c, Opt::Resources, -1);
        cycle_choice(&mut c, Opt::Resources, -1);
        assert_eq!(
            c.resources,
            Resources::Random,
            "back past Map Default wraps to the last"
        );
        cycle_choice(&mut c, Opt::Units, 1);
        assert!(c.one_worker);
        cycle_choice(&mut c, Opt::Units, 1);
        assert!(!c.one_worker);
        for o in Opt::ALL {
            for i in 0..o.labels().len() {
                let mut c = Custom::default();
                set_choice(&mut c, o, i);
                assert_eq!(choice(&c, o), i, "{o:?} {i}");
            }
        }
        let mut c = Custom::default();
        set_choice(&mut c, Opt::Race, 0);
        assert_eq!(c.race, Some(race::HUMAN));
        set_choice(&mut c, Opt::Race, 1);
        assert_eq!(c.race, Some(race::ORC));
        set_choice(&mut c, Opt::Race, 2);
        assert_eq!(c.race, None);
        set_choice(&mut c, Opt::Opponents, 3);
        assert_eq!(c.opponents, Some(3));
        set_choice(&mut c, Opt::Tileset, 4);
        assert_eq!(c.terrain, TerrainChoice::Set(Era::Swamp));
        set_choice(&mut c, Opt::Tileset, 99);
        assert_eq!(c.terrain, TerrainChoice::Set(Era::Swamp), "out of range does nothing");
    }

    #[test]
    fn start_hands_the_choices_to_setup_custom() {
        let mut s = Screen::new(sorted(), None);
        set_choice(&mut s.custom, Opt::Race, 1);
        set_choice(&mut s.custom, Opt::Opponents, 3);
        set_choice(&mut s.custom, Opt::Resources, 4);
        set_choice(&mut s.custom, Opt::Tileset, 2);
        set_choice(&mut s.custom, Opt::Units, 1);
        let (path, setup) = s.start().unwrap();
        assert_eq!(path, "Maps\\A two.pud");
        assert!(setup.enhanced, "single player is Enhanced");
        assert_eq!(
            setup.game,
            Game::Custom(Custom {
                race: Some(race::ORC),
                opponents: Some(3),
                resources: Resources::Random,
                terrain: TerrainChoice::Set(Era::Winter),
                one_worker: true,
                fixed_starts: false,
                fog_of_war: true,
            })
        );
        assert_eq!(Screen::new(Vec::new(), None).start(), None);
    }

    #[test]
    fn start_needs_a_map_with_a_slot_for_a_human() {
        let computers_only = info("Maps\\c.pud", 32, &[4, 4]);
        let mut s = Screen::new(vec![computers_only], None);
        assert!(!s.can_start(), "no human slot");
        assert_eq!(s.start(), None);
        s.maps.push(info("Maps\\h.pud", 32, &[5]));
        s.chosen = Some(1);
        assert!(s.can_start(), "map default counts at least one opponent");
        set_choice(&mut s.custom, Opt::Opponents, 1);
        assert!(s.can_start());
    }

    #[test]
    fn a_click_opens_a_list_and_a_click_on_a_line_picks_it() {
        let mut s = Screen::new(sorted(), None);
        // The Resources box, then its fourth line (High).
        assert_eq!(click(&mut s, 300, 285), Outcome::Stay);
        assert!(s.popup.is_some());
        let line = s.popup_rect(s.popup.as_ref().unwrap());
        assert_eq!(click(&mut s, 300, line.y + 1 + 3 * POPUP_LINE + 5), Outcome::Stay);
        assert!(s.popup.is_none());
        assert_eq!(s.custom.resources, Resources::High);
        // A press outside an open list closes it and picks nothing.
        click(&mut s, 300, 285);
        click(&mut s, 20, 20);
        assert!(s.popup.is_none());
        assert_eq!(s.custom.resources, Resources::High);
    }

    #[test]
    fn keys_cycle_the_focused_box_and_press_the_buttons() {
        let mut s = Screen::new(sorted(), None);
        assert_eq!(s.focus, Target::Drop(Drop::Opt(Opt::Race)));
        key(&mut s, Key::Right);
        assert_eq!(s.custom.race, Some(race::HUMAN), "Map Default wraps to Human");
        key(&mut s, Key::Tab);
        key(&mut s, Key::Down);
        assert_eq!(s.custom.opponents, Some(1));
        s.update(&Input {
            keys: vec![Key::Tab],
            shift: true,
            ..Default::default()
        });
        assert_eq!(s.focus, Target::Drop(Drop::Opt(Opt::Race)));
        assert_eq!(key(&mut s, Key::Letter('s')), Outcome::Start);
        assert_eq!(key(&mut s, Key::Letter('C')), Outcome::Cancel);
        assert_eq!(key(&mut s, Key::Escape), Outcome::Cancel);
    }

    #[test]
    fn the_pad_moves_the_focus_with_up_and_down_and_the_keyboard_still_cycles() {
        let mut s = Screen::new(sorted(), None);
        let pad = |s: &mut Screen, k: Key, f: menuscreen::FocusKey| {
            s.update(&Input {
                keys: vec![k],
                pad_focus: Some(f),
                ..Default::default()
            })
        };
        key(&mut s, Key::Down);
        assert_eq!(s.custom.race, Some(race::HUMAN), "the keyboard's Down cycles the box");
        assert_eq!(s.focus, Target::Drop(Drop::Opt(Opt::Race)));
        pad(&mut s, Key::Down, menuscreen::FocusKey::Next);
        assert_eq!(s.custom.race, Some(race::HUMAN), "the pad's Down leaves the value");
        assert_ne!(s.focus, Target::Drop(Drop::Opt(Opt::Race)));
        pad(&mut s, Key::Up, menuscreen::FocusKey::Back);
        assert_eq!(s.focus, Target::Drop(Drop::Opt(Opt::Race)));
        key(&mut s, Key::Right);
        assert_eq!(s.custom.race, Some(race::ORC), "left and right still cycle");
    }

    #[test]
    fn a_grey_start_does_not_start() {
        let mut s = Screen::new(Vec::new(), None);
        assert_eq!(click(&mut s, 500, 418), Outcome::Stay);
        assert_eq!(key(&mut s, Key::Letter('S')), Outcome::Stay);
        assert_eq!(click(&mut s, 500, 454), Outcome::Cancel, "Cancel Game");
    }

    #[test]
    fn the_dialog_filters_the_list_and_ok_picks_the_line() {
        let mut s = Screen::new(sorted(), None);
        click(&mut s, 500, 382); // Select Scenario
        assert!(s.dialog.is_some());
        assert_eq!(s.dialog.as_ref().unwrap().list.len(), 3);
        assert_eq!(
            s.selected().map(|m| m.name.as_str()),
            Some("A two"),
            "the picked map is selected"
        );
        // Players: "5 to 8 players" rebuilds the list and selects its first line.
        click(&mut s, 300, 128);
        let p = s.popup_rect(s.popup.as_ref().unwrap());
        click(&mut s, 300, p.y + 1 + 2 * POPUP_LINE + 5);
        let d = s.dialog.as_ref().unwrap();
        assert_eq!((d.filter.players, d.list.len(), d.sel), (2, 1, 0));
        assert_eq!(s.selected().map(|m| m.players), Some(8));
        // Cancel keeps the old pick and the old filter.
        click(&mut s, 150, 394);
        assert!(s.dialog.is_none());
        assert_eq!(s.map().map(|m| m.name.as_str()), Some("A two"));
        assert_eq!(s.filter, Filter::DEFAULT);
        // Again, and OK this time.
        click(&mut s, 500, 382);
        click(&mut s, 300, 128);
        let p = s.popup_rect(s.popup.as_ref().unwrap());
        click(&mut s, 300, p.y + 1 + 2 * POPUP_LINE + 5);
        click(&mut s, 470, 394);
        assert!(s.dialog.is_none());
        assert_eq!(s.map().map(|m| m.players), Some(8));
        assert_eq!(s.filter.players, 2, "the dialog's filters stay for next time");
        // Keys move the list: Esc leaves, Down/End move.
        key(&mut s, Key::Letter('E'));
        key(&mut s, Key::Escape);
        assert!(s.dialog.is_none());
    }

    #[test]
    fn the_dialog_list_scrolls_by_lines() {
        let many: Vec<MapInfo> = (0..20)
            .map(|i| info(&format!("Maps\\m{i:02}.pud"), 32, &[5, 4]))
            .collect();
        let mut s = Screen::new(many, None);
        key(&mut s, Key::Letter('E'));
        key(&mut s, Key::End);
        let d = s.dialog.as_ref().unwrap();
        assert_eq!((d.sel, d.top), (19, 14), "the selected last line shows at the bottom");
        key(&mut s, Key::Home);
        assert_eq!((s.dialog.as_ref().unwrap().sel, s.dialog.as_ref().unwrap().top), (0, 0));
        s.update(&Input {
            at: Some((200, 200)),
            wheel: -2,
            ..Default::default()
        });
        assert_eq!(
            s.dialog.as_ref().unwrap().top,
            2,
            "the wheel turned toward the user scrolls down"
        );
        key(&mut s, Key::Enter);
        assert!(s.dialog.is_none(), "Enter is OK");
        assert_eq!(s.map().map(|m| m.name.as_str()), Some("m00"));
    }

    #[test]
    fn a_dialog_with_nothing_listed_picks_nothing() {
        let mut s = Screen::new(sorted(), None);
        key(&mut s, Key::Letter('E'));
        click(&mut s, 300, 128);
        let p = s.popup_rect(s.popup.as_ref().unwrap());
        click(&mut s, 300, p.y + 1 + 3 * POPUP_LINE + 5); // 1 player
        assert!(s.dialog.as_ref().unwrap().list.is_empty());
        key(&mut s, Key::Enter);
        assert_eq!(
            s.map().map(|m| m.name.as_str()),
            Some("A two"),
            "OK on an empty list keeps the pick"
        );
    }

    #[test]
    fn the_type_box_offers_only_types_that_have_maps() {
        let s = Screen::new(sorted(), None);
        assert_eq!(s.kinds(), [MapKind::Custom]);
        let mut with_builtin = sorted();
        with_builtin[0].kind = MapKind::BuiltIn;
        assert_eq!(
            Screen::new(with_builtin, None).kinds(),
            [MapKind::BuiltIn, MapKind::Custom]
        );
    }
}
