//! The in-game menu and the dialogs under it (spec `docs/specs/ui/game-menu.md`): the game menu (F10, Alt+M, the menu
//! button), options, help, the sound, speed and preferences dialogs, the objectives screen and the end-scenario menu
//! with its confirmations. The rules are in [`crate::menu`] and [`crate::prefs`]; this module draws the dialogs on the
//! front end's layer (as the victory and defeat dialogs are) and applies what they decide.
//!
//! While a menu is open the game doesn't advance in single player ([`crate::core::game_running`] is false). A network
//! game goes on underneath ([`crate::core::game_ticking`]), with the menu holder's game input off.
//!
//! Choices where the spec leaves a gap:
//! - The dialog files (`gamemenu.bin` and the others) have no spec, so the layouts, the labels and the texts of the
//!   confirmations and the keyboard help are our own, drawn like the outcome dialogs, without the race backdrops.
//! - Save and Load open the save and load dialogs (`game/save-load.md`; the rules are in [`crate::saves`], the layout and
//!   input in [`crate::savedlg`], the game's side in [`crate::savegame`]). They are screens of this menu's stack, so
//!   the game stays stopped while they are open. Load stays in the menu: this client has no network session. The
//!   victory dialog's Save opens the same dialog over it ([`open_from_outcome`]); a save there ends the game as
//!   Continue does, and Cancel shows the victory dialog again.
//! - Restart fades all sound first, as Quit to Menu does, then rebuilds the mission through the flow
//!   (`frontend::run_quit`, `frontend::restart`). Surrender (and Quit (draw), locally the same) sends a command
//!   (`Command::Surrender`) that the sim carries out at once, between cycles: the player's result is defeat and the
//!   game is over. The menus close, and the defeat dialog and score screen follow from the outcome dialog as for any
//!   defeat.
//! - Quit to menu, Return to chat and Load custom scenario all go to the main menu (the client has no chat room or
//!   custom-game screen yet). Exit ends the program.
//! - The speed dialog sets the game speed with speed commands when OK is pressed (the game is stopped while it is open,
//!   so nothing can tell the difference); the sim's speed starts at Normal whatever speed was stored.
//! - Fog of war changed in the preferences dialog takes effect on OK (the spec: nothing happens while the dialog is
//!   open); Cancel leaves it as it was.
//! - Sound dialog changes are applied 5 frames after the first change (the spec's open question default).
//! - Keyboard focus (`ui/screen-and-cursors.md`, "Keyboard focus"): each screen opens focused on its default button
//!   ([`default_act`]); Tab and Shift+Tab move the focus over every control shown, buttons, sliders, check boxes and
//!   each radio, in the dialog's Tab order ([`tab_stops`]: the settings dialogs' OK and Cancel first, as the round-75
//!   orders show, then the rest from the top), without changing any of them; Enter activates the focused button and
//!   does nothing on another control (ours: the spec only has Enter on buttons).
//! - Ctrl+S sets the digital volume to 0 and back (`Prefs::toggle_digital`); Ctrl+M isn't handled here, `music.rs`
//!   owns it (`Prefs::toggle_music` is ready for it).
//!
//! Not yet here:
//! - The tips dialog at the start of a game, and the "play the opening film" and "system pointer" bits.
//! - Consuming the music volume, the music bit and colour cycling: `music.rs` reads [`Stored`]`.0.music_volume`
//!   and `palette.rs` [`crate::prefs::Prefs::colour_cycling`].
//! - The pause greyscale (`palette.rs`), the Enhanced multiplayer pause counts, Battle.net rules and the restart
//!   permission check (no multiplayer).
//! - The dialogs' own art, and the "Network" button of the Battle.net options menu.

use crate::campaign as cf;
use crate::core::{Commands, Core, add_layer, remove_layers};
use crate::frontend::{self, AppState, Flow, Front, FrontScreen};
use crate::game::{self, Sim};
use crate::hud::{self, Rect};
use crate::input::{Button, Input, Key, LogCursor};
use crate::layers::{PictureId, Pictures, Role};
use crate::menu::{self, Confirm, EndButton, Kind, Nav, Saving, Screen};
use crate::mouse;
use crate::prefs::{self, Prefs, bit};
use crate::savedlg::{self, SaveUi};
use crate::savefile;
use crate::savegame;
use crate::saves::{self, SaveDir};
use crate::settings;

/// The stored settings in force.
pub struct Stored(pub Prefs);

/// The open menu. It exists exactly while a menu or dialog is open.
pub struct GameMenu {
    nav: Nav,
    image: PictureId,
    /// The settings as they were when the current settings dialog opened (Cancel puts them back).
    snapshot: Prefs,
    /// The settings the current dialog edits.
    draft: Prefs,
    /// The game speed slider, applied with OK.
    speed: u8,
    /// The fog of war radio, applied with OK.
    fog: bool,
    fog_at_open: bool,
    enhanced: bool,
    /// The extra speed levels above Fastest this game has.
    extra_speeds: u8,
    /// The game is recorded (option `auto-replay`), so the menu offers Save Replay.
    can_save_replay: bool,
    /// The player only watches this network game.
    observer: bool,
    /// A phone or tablet: no Exit Program (the system closes apps; `docs/design/touch.md`).
    no_exit: bool,
    /// Touch is in use: the Help menu offers the Touch Controls page (`touchhelp`).
    touch_help: bool,
    /// Text shown by the current screen (wrapped).
    lines: Vec<String>,
    /// The Touch Controls pages' state (the `TouchHelp` screen).
    help: crate::touchhelp::Pager,
    delay: menu::Delay,
    hot: Option<usize>,
    /// The focused button (the yellow ring): its index in the item list; `None` until the screen is next run, which
    /// puts it on the default button ([`default_act`]).
    focus: Option<usize>,
    /// The control the left mouse button went down on, while it is held (screen spec, "Pressing"), and the button
    /// last drawn pressed.
    held: crate::menuscreen::Hold<usize>,
    pressed: Option<usize>,
    /// The slider being dragged: its index in the item list.
    drag: Option<usize>,
    dirty: bool,
    /// The open save or load dialog (the `Saving` screen).
    save: Option<SaveUi>,
    /// The open Settings screen (the `Settings` screen).
    settings: Option<InGameSettings>,
    /// Something the dialog decided that needs the whole game: done by [`service_saving`].
    request: Option<saves::Step>,
    /// Opened from the victory dialog's Save.
    from_outcome: bool,
}

/// Tidewater's Settings screen over the game (`settings`): the screen, the clear picture it draws on (the game shows
/// round the panel) and the menu picture's palette that its colours come from.
struct InGameSettings {
    ui: settings::Screen,
    background: crate::scene::Picture,
    palette: tidewater_formats::tileset::Palette,
}

/// The game menu's input state: the save dialog's input for this frame (collected by [`gather_saving_input`]), the
/// places in the typed events and the wheel steps it has read, and whether the menu development aid has run.
///
/// The game menu's functions run last in the frame (`core::frame_systems`), after the game screen's, so a key that
/// opens or closes a menu is seen by one side only, the same way every frame (an Escape that closes a menu doesn't
/// also reach the game).
#[derive(Default)]
pub struct MenuInput {
    saving: savedlg::Input,
    cursors: (LogCursor, LogCursor),
    demo_done: bool,
}

// ---------------------------------------------------------------------------------------------------------------
// The dialogs' layout

const BOX_X: i32 = 160;
const BOX_W: u32 = 320;
const PAD: i32 = 16;
const ROW_H: u32 = 26;
const ROW_GAP: i32 = 6;
const INNER_W: u32 = BOX_W - 2 * PAD as u32;
/// A text link's tap target.
const LINK_W: u32 = 160;
const LINK_H: u32 = 24;
/// The key that opens the Settings screen in a game.
pub(crate) const SETTINGS_KEY: (mouse::Key, mouse::KeyMods) = (
    mouse::Key::Letter('G'),
    mouse::KeyMods {
        shift: false,
        ctrl: true,
        alt: false,
    },
);
/// The Settings link's text, in the style of the main menu's version line and Settings link.
const SETTINGS_LINK: &str = "Settings (Ctrl+G)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Open(Screen),
    CloseAll,
    /// Writes the recording of the game to the replays folder.
    SaveReplay,
    Back,
    Ok,
    Cancel,
    Do(Confirm),
    /// Opens Tidewater's Settings screen over the game; Back from it returns to the game, not to this menu.
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slide {
    Music,
    Digital,
    Game,
    Mouse,
    Key,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tick {
    Speech,
    Acknowledgements,
    Building,
    Subtitles,
    Colour,
}

impl Tick {
    fn bit(self) -> u32 {
        match self {
            Tick::Speech => bit::UNIT_SPEECH,
            Tick::Acknowledgements => bit::ACKNOWLEDGEMENTS,
            Tick::Building => bit::BUILDING_SOUNDS,
            Tick::Subtitles => bit::SUBTITLES,
            Tick::Colour => bit::COLOUR_CYCLING,
        }
    }
}

/// A radio option: which group, and the value that choosing it sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Radio {
    FirstMouseStyle(bool),
    Fog(bool),
    Terrain(bool),
}

#[derive(Clone, Debug, PartialEq)]
enum Kinds {
    Label(String),
    Lines(Vec<String>),
    Button(Act, String),
    Slider(Slide, String, u8, u8, bool),
    Check(Tick, String, bool),
    Radio(Radio, String, bool),
    /// A small text link of ours (not one of the original's buttons), drawn in the small font.
    Link(Act, String),
}

struct Item {
    r: Rect,
    kind: Kinds,
}

struct Built {
    title: &'static str,
    frame: Rect,
    items: Vec<Item>,
}

struct Builder {
    y: i32,
    line_h: i32,
    items: Vec<Item>,
}

impl Builder {
    fn new(line_h: i32) -> Self {
        Self {
            y: 0,
            line_h,
            items: Vec::new(),
        }
    }

    fn row(&mut self, h: u32, kind: Kinds) {
        self.items.push(Item {
            r: Rect::new(BOX_X + PAD, self.y, INNER_W, h),
            kind,
        });
        self.y += h as i32 + ROW_GAP;
    }

    fn button(&mut self, act: Act, label: &str) {
        self.row(ROW_H, Kinds::Button(act, label.to_string()));
    }

    /// A text link at the right end of a row, [`LINK_H`] high and [`LINK_W`] wide so a finger can hit it.
    fn link(&mut self, act: Act, label: &str) {
        self.items.push(Item {
            r: Rect::new(BOX_X + PAD + (INNER_W - LINK_W) as i32, self.y, LINK_W, LINK_H),
            kind: Kinds::Link(act, label.to_string()),
        });
        self.y += LINK_H as i32 + ROW_GAP;
    }

    /// Two buttons side by side (OK and Cancel).
    fn pair(&mut self, a: (Act, &str), b: (Act, &str)) {
        let w = (INNER_W - 8) / 2;
        for (i, (act, label)) in [a, b].into_iter().enumerate() {
            self.items.push(Item {
                r: Rect::new(BOX_X + PAD + i as i32 * (w as i32 + 8), self.y, w, ROW_H),
                kind: Kinds::Button(act, label.to_string()),
            });
        }
        self.y += ROW_H as i32 + ROW_GAP;
    }

    fn lines(&mut self, lines: &[String]) {
        let h = (lines.len() as i32 * self.line_h).max(self.line_h) as u32;
        self.row(h, Kinds::Lines(lines.to_vec()));
    }

    fn slider(&mut self, id: Slide, label: &str, value: u8, max: u8, enabled: bool) {
        self.row(34, Kinds::Slider(id, label.to_string(), value, max, enabled));
    }

    fn check(&mut self, id: Tick, label: &str, on: bool) {
        self.row(ROW_H - 6, Kinds::Check(id, label.to_string(), on));
    }

    /// A labelled pair of radio options.
    fn radios(&mut self, label: &str, a: (Radio, &str, bool), b: (Radio, &str, bool)) {
        self.row(self.line_h as u32, Kinds::Label(label.to_string()));
        let w = (INNER_W - 8) / 2;
        for (i, (id, text, on)) in [a, b].into_iter().enumerate() {
            self.items.push(Item {
                r: Rect::new(BOX_X + PAD + i as i32 * (w as i32 + 8), self.y, w, ROW_H - 6),
                kind: Kinds::Radio(id, text.to_string(), on),
            });
        }
        self.y += ROW_H as i32 - 6 + ROW_GAP;
    }

    fn finish(mut self, title: &'static str, title_h: i32) -> Built {
        let top = PAD + title_h + ROW_GAP;
        let h = top + self.y + PAD - ROW_GAP;
        let y0 = ((480 - h) / 2).max(0);
        for it in &mut self.items {
            it.r.y += y0 + top;
        }
        Built {
            title,
            frame: Rect::new(BOX_X, y0, BOX_W, h as u32),
            items: self.items,
        }
    }
}

impl GameMenu {
    /// The save dialog's name field while the dialog is open on its list, where typing names the game (640 x 480
    /// menu pixels).
    pub(crate) fn name_box(&self) -> Option<crate::hud::Rect> {
        self.save
            .as_ref()
            .is_some_and(|s| s.dialog.mode == saves::Mode::Save && s.dialog.stage == saves::Stage::List)
            .then_some(savedlg::FIELD)
    }

    fn screen(&self) -> Screen {
        self.nav.current().unwrap_or(Screen::Game)
    }

    fn build(&self, title_h: i32, line_h: i32, kind: Kind) -> Built {
        let mut b = Builder::new(line_h);
        let d = &self.draft;
        let (title, wide) = match self.screen() {
            Screen::Game => {
                b.button(Act::Open(Screen::Saving(Saving::Save)), "Save Game (F11)");
                b.button(Act::Open(Screen::Saving(Saving::Load)), "Load Game (F12)");
                if self.can_save_replay {
                    b.button(Act::SaveReplay, "Save Replay");
                }
                b.button(Act::Open(Screen::Options), "Options (F5)");
                b.button(Act::Open(Screen::Help), "Help (F1)");
                b.button(Act::Open(Screen::Objectives), "Objectives (O)");
                b.button(Act::Open(Screen::End), "End Scenario (E)");
                b.button(Act::CloseAll, "Return to Game (Esc)");
                ("Game Menu", false)
            }
            Screen::Options => {
                b.button(Act::Open(Screen::Sound), "Sound (F7)");
                b.button(Act::Open(Screen::Speed), "Speeds (F8)");
                b.button(Act::Open(Screen::Prefs), "Preferences (F9)");
                b.button(Act::Back, "Previous");
                b.link(Act::Settings, SETTINGS_LINK);
                ("Options", false)
            }
            Screen::Help => {
                b.button(Act::Open(Screen::KeyHelp), "Keystroke Help");
                if self.touch_help {
                    b.button(Act::Open(Screen::TouchHelp), "Touch Controls");
                }
                b.button(Act::Open(Screen::Tips), "Tips");
                b.button(Act::Back, "Previous");
                ("Help", false)
            }
            Screen::Sound => {
                b.slider(
                    Slide::Music,
                    "Music volume",
                    d.music_volume,
                    prefs::MAX_VOLUME,
                    d.has(bit::MUSIC),
                );
                b.slider(
                    Slide::Digital,
                    "Sound effects volume",
                    d.digital_volume,
                    prefs::MAX_VOLUME,
                    true,
                );
                for (t, l) in [
                    (Tick::Speech, "Unit speech"),
                    (Tick::Acknowledgements, "Acknowledgements"),
                    (Tick::Building, "Building sounds"),
                    (Tick::Subtitles, "Subtitles"),
                ] {
                    b.check(t, l, d.has(t.bit()));
                }
                b.pair((Act::Ok, "OK"), (Act::Cancel, "Cancel"));
                ("Sound", false)
            }
            Screen::Speed => {
                b.slider(
                    Slide::Game,
                    "Game speed",
                    self.speed,
                    menu::game_speed_max(self.enhanced, self.extra_speeds),
                    true,
                );
                b.slider(
                    Slide::Mouse,
                    "Mouse scroll speed",
                    d.mouse_scroll,
                    prefs::MAX_SCROLL_SPEED,
                    true,
                );
                b.slider(
                    Slide::Key,
                    "Key scroll speed",
                    d.key_scroll,
                    prefs::MAX_SCROLL_SPEED,
                    true,
                );
                b.pair((Act::Ok, "OK"), (Act::Cancel, "Cancel"));
                ("Speeds", false)
            }
            Screen::Prefs => {
                let first = d.has(bit::FIRST_MOUSE_STYLE);
                b.radios(
                    "Mouse style",
                    (Radio::FirstMouseStyle(true), "Right click commands", first),
                    (Radio::FirstMouseStyle(false), "Right click centres", !first),
                );
                b.radios(
                    "Fog of war",
                    (Radio::Fog(true), "On", self.fog),
                    (Radio::Fog(false), "Off", !self.fog),
                );
                b.radios(
                    "Minimap terrain",
                    (Radio::Terrain(true), "On", d.minimap_terrain),
                    (Radio::Terrain(false), "Off", !d.minimap_terrain),
                );
                b.check(Tick::Colour, "Colour cycling", d.has(bit::COLOUR_CYCLING));
                b.pair((Act::Ok, "OK"), (Act::Cancel, "Cancel"));
                ("Preferences", false)
            }
            Screen::Objectives => {
                b.lines(&self.lines);
                b.button(Act::Back, "Previous");
                ("Objectives", true)
            }
            Screen::End => {
                // An observer has nothing to surrender.
                for e in menu::end_buttons(kind)
                    .into_iter()
                    .filter(|e| !(self.observer && *e == EndButton::Surrender))
                    .filter(|e| !(self.no_exit && *e == EndButton::Exit))
                {
                    let label = match e {
                        EndButton::Restart => "Restart Scenario (Ctrl+R)",
                        EndButton::ReturnToChat => "Return to Chat Room",
                        EndButton::LoadCustom => "Load Another Scenario",
                        EndButton::Surrender => "Surrender (Ctrl+Q)",
                        EndButton::QuitToMenu => "Quit to Main Menu",
                        EndButton::Exit => "Exit Program (Ctrl+X)",
                    };
                    b.button(Act::Open(Screen::Confirm(e.confirm())), label);
                }
                b.button(Act::Back, "Previous");
                ("End Scenario", false)
            }
            Screen::Confirm(c) => {
                b.lines(&self.lines);
                b.pair((Act::Do(c), "OK"), (Act::Back, "Cancel"));
                (
                    match c {
                        Confirm::Restart => "Restart",
                        Confirm::Surrender | Confirm::QuitDraw => "Surrender",
                        Confirm::LoadCustom => "Load Scenario",
                        Confirm::ReturnToChat => "Chat Room",
                        Confirm::QuitToMenu => "Quit",
                        Confirm::Exit => "Exit",
                    },
                    false,
                )
            }
            // The save and load dialogs draw themselves (`savedlg`); this frame is only their place.
            Screen::Saving(_) | Screen::Settings => ("", false),
            Screen::KeyHelp => {
                b.lines(&self.lines);
                // Screen spec, "Focus" (round 75): the Keystroke Help dialog's button is OK.
                b.button(Act::Back, "OK");
                ("Keystroke Help", true)
            }
            // The Touch Controls pages draw themselves (`touchhelp`).
            Screen::TouchHelp => ("", false),
            Screen::Tips => {
                b.lines(&self.lines);
                b.button(Act::Back, "OK");
                ("Tip", false)
            }
            Screen::Refused(_) => {
                b.lines(&self.lines);
                b.button(Act::Back, "OK");
                ("Network", false)
            }
        };
        let _ = wide;
        b.finish(title, title_h)
    }
}

/// The texts and numbers a screen shows, set when it opens.
fn enter(m: &mut GameMenu, front: &mut Front, stored: &mut Stored, sim: &Sim, flow: &Flow, folder: &SaveDir) {
    let screen = m.screen();
    let wrap = |front: &Front, text: &str| cf::wrap(text, INNER_W, &|s: &str| front.font.width(s.as_bytes()));
    m.lines.clear();
    match screen {
        Screen::Sound | Screen::Speed | Screen::Prefs => {
            m.snapshot = stored.0.clone();
            m.draft = stored.0.clone();
            m.speed = sim.world.speed.min(menu::game_speed_max(m.enhanced, m.extra_speeds));
            m.fog = sim.world.rules.fog_of_war;
            m.fog_at_open = m.fog;
            m.delay.clear();
        }
        Screen::Objectives => {
            let entry = menu::objectives_entry(flow.game);
            let text = frontend::entry_text(&mut front.install, "rez\\objctivs.tbl", entry);
            m.lines = wrap(front, &text);
        }
        Screen::Tips => {
            let n = stored.0.take_tip();
            let text = frontend::entry_text(&mut front.install, "rez\\tips.tbl", n as u16);
            m.lines = wrap(front, &text);
        }
        Screen::Saving(s) => {
            let mode = if s == Saving::Save {
                saves::Mode::Save
            } else {
                saves::Mode::Load
            };
            let entries = folder.list(savefile::Kind::Single, sim.world.rules.enhanced);
            // Network entry 105 lists the characters a name can't hold.
            let forbidden = front.net(105);
            let forbidden = if forbidden.is_empty() {
                saves::FORBIDDEN.to_string()
            } else {
                forbidden
            };
            m.save = Some(SaveUi::new(saves::Dialog::open(mode, entries, &forbidden)));
        }
        Screen::Confirm(c) => {
            let text = match c {
                Confirm::Restart => "Restart this scenario from the beginning?",
                Confirm::Surrender | Confirm::QuitDraw => "Surrender? You will be defeated.",
                Confirm::LoadCustom => "Leave this game and load another scenario?",
                Confirm::ReturnToChat => "Leave this game and return to the chat room?",
                Confirm::QuitToMenu => "Quit this game and return to the main menu?",
                Confirm::Exit => "Exit the program?",
            };
            m.lines = wrap(front, text);
        }
        Screen::KeyHelp => m.lines = KEY_HELP.iter().map(|s| s.to_string()).collect(),
        Screen::TouchHelp => m.help = Default::default(),
        Screen::Refused(host) => {
            // Network entry 28 with the host's name; ours when the install has no such table.
            let template = front.net(REFUSED_ENTRY);
            let template = if template.is_empty() {
                "Only %s may do that.".to_string()
            } else {
                template
            };
            let text = crate::messages::fill_name(&template, &sim.session.player_name(host));
            m.lines = wrap(front, &text);
        }
        // The Settings screen is made when its first frame runs (`run_settings`), from the whole game.
        Screen::Settings => m.settings = None,
        Screen::Game | Screen::Options | Screen::Help | Screen::End => {}
    }
    m.dirty = true;
    m.hot = None;
    m.focus = None;
    m.held = Default::default();
    m.pressed = None;
    m.drag = None;
}

/// The button a screen opens focused on, its default button, by what it does (screen spec, "Buttons", "Focus", rounds
/// 74 and 75): Return to Game on the game menu, Previous on End Scenario, OK on a confirmation (Quit to Menu's),
/// Previous on the Options and Help menus, OK on the Sound, Speeds, Preferences, Keystroke Help and Tips dialogs. Ours
/// where the spec saw no capture: Previous on the objectives. The save and load dialogs keep their own (`savedlg`).
fn default_act(screen: Screen) -> Option<Act> {
    match screen {
        Screen::Game => Some(Act::CloseAll),
        Screen::Confirm(c) => Some(Act::Do(c)),
        Screen::Sound | Screen::Speed | Screen::Prefs => Some(Act::Ok),
        Screen::Options
        | Screen::Help
        | Screen::Objectives
        | Screen::End
        | Screen::KeyHelp
        | Screen::TouchHelp
        | Screen::Tips
        | Screen::Refused(_) => Some(Act::Back),
        Screen::Saving(_) | Screen::Settings => None,
    }
}

/// The index of the default button in a screen's items (the first button if none matches).
fn default_focus(built: &Built, screen: Screen) -> Option<usize> {
    let act = default_act(screen);
    let button = |it: &Item| matches!(it.kind, Kinds::Button(..));
    built
        .items
        .iter()
        .position(|it| matches!(it.kind, Kinds::Button(a, _) if Some(a) == act))
        .or_else(|| built.items.iter().position(button))
}

/// A game dialog's button outline (screen spec, "Buttons", rounds 74 and 75): the focus ring when focused, the
/// default button's 247 outline otherwise, enabled or disabled, and none on the other buttons.
fn button_ring(focused: bool, default: bool) -> Option<[u8; 3]> {
    match (focused, default) {
        (true, _) => Some(FOCUS_RGB),
        (false, true) => Some(DEFAULT_RGB),
        (false, false) => None,
    }
}

/// Whether Tab stops on a control: buttons, sliders, check boxes and each radio (screen spec, "Keyboard focus", round
/// 75); headings and text don't take the focus. Our dialogs build only the controls they show, so a hidden one is
/// never among the items.
fn takes_focus(kind: &Kinds) -> bool {
    matches!(
        kind,
        Kinds::Button(..) | Kinds::Slider(..) | Kinds::Check(..) | Kinds::Radio(..) | Kinds::Link(..)
    )
}

/// A screen's Tab stops (indices in its items) in its Tab order. The dialog files have no spec, so the order is the
/// round-75 observations': on the Sound, Speeds and Preferences dialogs OK and Cancel come first, then the sliders,
/// radios and check boxes from the top; every other screen's controls go from the top.
fn tab_stops(built: &Built, screen: Screen) -> Vec<usize> {
    let shown = built.items.iter().enumerate().filter(|(_, it)| takes_focus(&it.kind));
    let ok_cancel = |it: &Item| matches!(it.kind, Kinds::Button(Act::Ok | Act::Cancel, _));
    if matches!(screen, Screen::Sound | Screen::Speed | Screen::Prefs) {
        let (first, rest): (Vec<_>, Vec<_>) = shown.partition(|(_, it)| ok_cancel(it));
        first.into_iter().chain(rest).map(|(i, _)| i).collect()
    } else {
        shown.map(|(i, _)| i).collect()
    }
}

/// The focus ring's colour: menu palette entry 251 (screen spec, "Buttons"). The game dialogs draw in RGB.
const FOCUS_RGB: [u8; 3] = [255, 255, 0];
/// Entry 247 in a game's palette, (160, 160, 164) (screen spec, "Buttons"): the default button's outline (unless it has
/// the focus) and a focused check box's or radio's rectangle.
const DEFAULT_RGB: [u8; 3] = [160, 160, 164];

/// A focused check box's or radio's mark (screen spec, "Keyboard focus", round 75): a 1-pixel rectangle round its
/// label's text, not round the box. Ours: one pixel clear of the text on each side.
fn label_mark(x: i32, y: i32, w: u32, h: u32) -> Rect {
    Rect::new(x - 2, y - 2, w + 4, h + 4)
}

/// A focused slider's mark (round 75): a rectangle as wide as the slider and 2 pixels taller, here round its track.
fn slider_mark(track: Rect) -> Rect {
    Rect::new(track.x, track.y - 1, track.w, track.h + 2)
}

const KEY_HELP: [&str; 18] = [
    "+ and -   Game speed",
    "Ctrl+P   Pause or resume",
    "F10, Alt+M   Game menu",
    "F1   Help      F5   Options",
    "F7   Sound     F8   Speeds    F9   Preferences",
    "F11   Save     F12   Load",
    "F2-F4   Go to a remembered view",
    "Shift+F2-F4   Remember the view",
    "0-9   Select a group",
    "Ctrl+0-9   Make a group",
    "Alt+C   Centre on the selected unit",
    "Ctrl+S   Sound effects on or off",
    "Ctrl+M   Music on or off",
    "Ctrl+R   Restart the scenario",
    "Ctrl+Q   Quit to the main menu",
    "Ctrl+X   Exit the program",
    "Ctrl+T   Tips",
    "Ctrl+G   Tidewater settings",
];

// ---------------------------------------------------------------------------------------------------------------
// Drawing

fn draw(front: &Front, m: &GameMenu, built: &Built) -> crate::scene::Picture {
    let mut p = frontend::blank();
    hud::fill(&mut p, built.frame, [24, 20, 16]);
    hud::outline(&mut p, built.frame, [180, 160, 100]);
    let tw = front.big.width(built.title.as_bytes()) as i32;
    hud::text(
        &mut p,
        &front.big,
        built.frame.x + (built.frame.w as i32 - tw) / 2,
        built.frame.y + PAD,
        built.title,
    );
    let f = &front.font;
    let default = default_act(m.screen());
    for (i, it) in built.items.iter().enumerate() {
        let hot = m.hot == Some(i);
        let focused = m.focus == Some(i);
        match &it.kind {
            Kinds::Label(s) => {
                hud::text(&mut p, f, it.r.x, it.r.y, s);
            }
            Kinds::Lines(ls) => {
                for (n, l) in ls.iter().enumerate() {
                    hud::text(&mut p, f, it.r.x, it.r.y + n as i32 * (f.height as i32 + 4), l);
                }
            }
            Kinds::Button(act, label) => {
                frontend::button_pressed(&mut p, f, it.r, label, hot, m.pressed == Some(i));
                // Screen spec, "Buttons" (round 75): the focus ring in 251, else the default button's outline in 247.
                if let Some(c) = button_ring(focused, default == Some(*act)) {
                    hud::outline(&mut p, it.r, c);
                }
            }
            Kinds::Slider(_, label, v, max, enabled) => {
                let text = format!("{label}: {v}");
                hud::text(&mut p, f, it.r.x, it.r.y, &text);
                let track = slider_track(it.r);
                hud::fill(&mut p, track, [12, 10, 8]);
                hud::outline(&mut p, track, if *enabled { [180, 160, 100] } else { [90, 80, 50] });
                if focused {
                    hud::outline(&mut p, slider_mark(track), FOCUS_RGB);
                }
                if *max > 0 {
                    let x = track.x + (track.w as i32 - 6) * *v as i32 / *max as i32;
                    let knob = Rect::new(x, track.y - 2, 6, track.h + 4);
                    hud::fill(&mut p, knob, if *enabled { [220, 200, 130] } else { [110, 100, 70] });
                }
            }
            Kinds::Link(_, label) => {
                let small = &front.small;
                let w = small.width(label.as_bytes());
                let x = it.r.x + it.r.w as i32 - w as i32 - 2;
                let y = it.r.y + (it.r.h as i32 - small.height as i32) / 2;
                hud::text(&mut p, small, x, y, label);
                if hot {
                    hud::fill(&mut p, Rect::new(x, y + small.height as i32 + 1, w, 1), [220, 200, 130]);
                }
                if focused {
                    hud::outline(&mut p, it.r, FOCUS_RGB);
                }
            }
            Kinds::Check(_, label, on) => {
                let b = Rect::new(it.r.x, it.r.y, 16, 16);
                hud::fill(&mut p, b, if hot { [70, 60, 30] } else { [36, 32, 24] });
                hud::outline(&mut p, b, [180, 160, 100]);
                if *on {
                    hud::fill(&mut p, Rect::new(b.x + 4, b.y + 4, 8, 8), [220, 200, 130]);
                }
                hud::text(&mut p, f, it.r.x + 24, it.r.y + 1, label);
                if focused {
                    let w = f.width(label.as_bytes());
                    hud::outline(
                        &mut p,
                        label_mark(it.r.x + 24, it.r.y + 1, w, f.height as u32),
                        DEFAULT_RGB,
                    );
                }
            }
            Kinds::Radio(_, label, on) => {
                let r = it.r;
                hud::fill(
                    &mut p,
                    r,
                    if *on {
                        [90, 76, 36]
                    } else if hot {
                        [70, 60, 30]
                    } else {
                        [36, 32, 24]
                    },
                );
                hud::outline(&mut p, r, if *on { [220, 200, 130] } else { [180, 160, 100] });
                let w = f.width(label.as_bytes()) as i32;
                let (x, y) = (r.x + (r.w as i32 - w) / 2, r.y + (r.h as i32 - f.height as i32) / 2);
                hud::text(&mut p, f, x, y, label);
                if focused {
                    hud::outline(&mut p, label_mark(x, y, w as u32, f.height as u32), DEFAULT_RGB);
                }
            }
        }
    }
    p
}

fn slider_track(r: Rect) -> Rect {
    Rect::new(r.x, r.y + 20, r.w, 10)
}

// ---------------------------------------------------------------------------------------------------------------
// Opening

/// Opens the menu or dialog a key or the menu button asks for.
pub(crate) fn open_requests(c: &mut Core) {
    let Core {
        commands,
        sim,
        panel_ui: ui,
        stored,
        front,
        flow,
        store: folder,
        pictures,
        menu_input: MenuInput { demo_done, .. },
        input,
        touch,
        ..
    } = c;
    let keys = &input.keys;
    let Some(sim) = sim.as_ref() else { return };
    let mut asked = ui
        .as_mut()
        .is_some_and(|u| std::mem::take(&mut u.menu_requested))
        .then_some(Screen::Game);
    // Development aid: `TIDEWATER_DEMO_MENU=game|options|sound|speed|prefs|objectives|end|restart|tips|save|load` opens that
    // screen once, as its key would, after 50 cycles so the map and panels have been drawn from a running game.
    if sim.world.tick >= 50
        && !std::mem::replace(demo_done, true)
        && let Ok(v) = std::env::var("TIDEWATER_DEMO_MENU")
    {
        asked = match v.as_str() {
            "game" => Some(Screen::Game),
            "options" => Some(Screen::Options),
            "sound" => Some(Screen::Sound),
            "speed" => Some(Screen::Speed),
            "prefs" => Some(Screen::Prefs),
            "objectives" => Some(Screen::Objectives),
            "end" => Some(Screen::End),
            "restart" => Some(Screen::Confirm(Confirm::Restart)),
            "tips" => Some(Screen::Tips),
            "save" => Some(Screen::Saving(Saving::Save)),
            "load" => Some(Screen::Saving(Saving::Load)),
            _ => asked,
        };
    }
    let m = mouse::KeyMods {
        shift: keys.any_pressed([Key::ShiftLeft, Key::ShiftRight]),
        ctrl: keys.any_pressed([Key::ControlLeft, Key::ControlRight]),
        alt: keys.any_pressed([Key::AltLeft, Key::AltRight]),
    };
    for &k in keys.get_just_pressed() {
        let Some(key) = game::game_key(k) else { continue };
        if key == mouse::Key::F(10) {
            asked = Some(Screen::Game);
            continue;
        }
        // Not in the original: Tidewater's Settings screen. F2 is the first remembered view's key, so it is Ctrl+G
        // (`keymap` keeps the chord from being rebound to another action).
        if key == SETTINGS_KEY.0 && m == SETTINGS_KEY.1 {
            asked = Some(Screen::Settings);
            continue;
        }
        // The key map (`keymap`) turns a rebound key into its action's own chord; the pressed key without a `[keys]`
        // section.
        let Some(a) = stored
            .0
            .keys
            .game(k, m)
            .and_then(|(key, m)| mouse::key_action(key, m, false, sim.world.rules.enhanced, false))
        else {
            continue;
        };
        use mouse::KeyAction as K;
        let s = match a {
            K::GameMenu => Screen::Game,
            K::Options => Screen::Options,
            K::HelpMenu => Screen::Help,
            K::KeyboardHelp => Screen::KeyHelp,
            K::SoundDialog => Screen::Sound,
            K::SpeedDialog => Screen::Speed,
            K::Preferences => Screen::Prefs,
            K::Tips => Screen::Tips,
            K::SaveGame => Screen::Saving(Saving::Save),
            K::LoadGame => Screen::Saving(Saving::Load),
            K::Restart => Screen::Confirm(Confirm::Restart),
            K::Quit => Screen::Confirm(Confirm::QuitToMenu),
            // A phone or tablet has no Exit Program, so Ctrl+X does nothing there.
            K::QuitDialog if touch.touch_only => continue,
            K::QuitDialog => Screen::Confirm(Confirm::Exit),
            K::ToggleSounds => {
                stored.0.toggle_digital();
                continue;
            }
            _ => continue,
        };
        asked = Some(s);
    }
    // Ours: a phone or tablet shows the Touch Controls page by itself at the first game (`touchhelp`), once ever. Not in
    // a network game, whose menus do not stop the game, nor in a replay.
    if asked.is_none()
        && touch.touch_only
        && !stored.0.touch_help_seen
        && sim.world.tick >= 2
        && !sim.session.is_network()
        && !sim.session.observing()
    {
        stored.0.touch_help_seen = true;
        asked = Some(Screen::TouchHelp);
    }
    let Some(screen) = asked else { return };
    open_menu(
        commands,
        screen,
        pictures,
        stored,
        front,
        sim,
        flow,
        &folder.folder(),
        false,
        touch,
    );
}

/// Network entry 28: "only the host may" (`ui/game-menu.md`, "Who may restart, return or load").
const REFUSED_ENTRY: u16 = 28;

/// The screen a choice opens, or entry 28's box when this player may not make it in a network game ([`menu::host_check`];
/// the host follows a hand-over, `docs/design/network.md`, "Host rights").
fn gate(screen: Screen, sim: &Sim) -> Screen {
    let s = &sim.session;
    menu::host_check(screen, s.is_network(), s.observing(), s.host(), sim.human)
}

/// Opens the menu on `screen`.
#[allow(clippy::too_many_arguments)]
fn open_menu(
    commands: &mut Commands,
    screen: Screen,
    pictures: &mut Pictures,
    stored: &mut Stored,
    front: &mut Front,
    sim: &Sim,
    flow: &Flow,
    folder: &SaveDir,
    from_outcome: bool,
    touch: &crate::touch::TouchState,
) {
    let mut nav = Nav::default();
    nav.open(gate(screen, sim));
    let image = pictures.add(frontend::blank());
    add_layer(
        commands,
        crate::layers::Layer::new(image, crate::layers::Plane::Flow, 2.0),
        Role::Menu,
        Some(AppState::Game),
    );
    let mut m = GameMenu {
        nav,
        image,
        snapshot: stored.0.clone(),
        draft: stored.0.clone(),
        speed: sim.world.speed,
        fog: sim.world.rules.fog_of_war,
        fog_at_open: sim.world.rules.fog_of_war,
        enhanced: sim.world.rules.enhanced,
        extra_speeds: sim.world.rules.extras.extra_speeds,
        can_save_replay: sim.session.can_save_replay(),
        observer: sim.session.observing(),
        no_exit: touch.touch_only,
        touch_help: touch.touch_only || touch.emulate || touch.active,
        lines: Vec::new(),
        help: Default::default(),
        delay: menu::Delay::default(),
        hot: None,
        focus: None,
        held: Default::default(),
        pressed: None,
        drag: None,
        dirty: true,
        save: None,
        settings: None,
        request: None,
        from_outcome,
    };
    enter(&mut m, front, stored, sim, flow, folder);
    commands.queue(move |c| c.menu = Some(m));
}

/// The victory dialog's Save opens the save dialog over it.
pub(crate) fn open_from_outcome(c: &mut Core) {
    let Core {
        commands,
        dialog: Some(dialog),
        sim,
        stored,
        front,
        flow,
        store: folder,
        pictures,
        touch,
        ..
    } = c
    else {
        return;
    };
    let Some(sim) = sim.as_ref() else { return };
    if !dialog.save_wanted {
        return;
    }
    dialog.save_wanted = false;
    // The dialog changed: it draws again when it shows again.
    dialog.redraw = true;
    open_menu(
        commands,
        Screen::Saving(Saving::Save),
        pictures,
        stored,
        front,
        sim,
        flow,
        &folder.folder(),
        true,
        touch,
    );
}

/// Collects the typed text, keys, pointer and wheel for the save dialog.
pub(crate) fn gather_saving_input(c: &mut Core) {
    let m = &mut c.menu_input;
    m.saving = saving_input(&c.input, &mut m.cursors, &c.front_screen);
}

/// One frame's input for a save or load dialog (also the main menu's load dialog, `frontend`).
/// `cursors`: the places in the typed events and the wheel steps.
pub(crate) fn saving_input(
    input: &Input,
    (typed, wheel): &mut (LogCursor, LogCursor),
    cam: &FrontScreen,
) -> savedlg::Input {
    let (keys, mouse_in) = (&input.keys, &input.mouse);
    let mut i = savedlg::Input {
        pointer: frontend::pointer(input, cam).map(|p| (p.x, p.y)),
        press: mouse_in.just_pressed(Button::Left),
        click: mouse_in.just_released(Button::Left),
        enter: keys.any_just_pressed([Key::Enter, Key::NumpadEnter]),
        escape: keys.just_pressed(Key::Escape),
        up: keys.just_pressed(Key::Up),
        down: keys.just_pressed(Key::Down),
        backspace: keys.just_pressed(Key::Backspace),
        tab: frontend::focus_key(keys),
        ..Default::default()
    };
    for e in input.typed.read(typed) {
        if e.pressed
            && let Some(t) = &e.text
        {
            i.text.extend(t.chars().filter(|c| !c.is_control()));
        }
    }
    for w in input.wheel.read(wheel) {
        i.wheel += w.y.signum() as i32;
    }
    i
}

pub(crate) fn close_on_exit(c: &mut Core) {
    c.commands.queue(|c| c.menu = None);
}

/// The key the Touch Controls pages take this frame (`touchhelp::PageKey`).
pub(crate) fn help_key(keys: &crate::input::Buttons<Key>) -> Option<crate::touchhelp::PageKey> {
    use crate::touchhelp::PageKey;
    if keys.just_pressed(Key::Escape) {
        Some(PageKey::Close)
    } else if keys.any_just_pressed([Key::Enter, Key::NumpadEnter]) {
        Some(PageKey::Enter)
    } else if keys.just_pressed(Key::Right) {
        Some(PageKey::Next)
    } else if keys.just_pressed(Key::Left) {
        Some(PageKey::Back)
    } else {
        None
    }
}

fn close(commands: &mut Commands) {
    remove_layers(commands, |e| e.role == Role::Menu);
    commands.queue(|c| c.menu = None);
}

// ---------------------------------------------------------------------------------------------------------------
// Running

pub(crate) fn run_menu(c: &mut Core) {
    let Core {
        commands,
        menu: Some(m),
        stored,
        sim: Some(sim),
        front,
        flow,
        pictures,
        front_screen: cam,
        store: folder,
        exit,
        input,
        jukebox,
        quitting,
        ..
    } = c
    else {
        return;
    };
    let keys = &input.keys;
    let mouse_in = &input.mouse;
    // The save and load dialogs run in `run_saving`, the Settings screen in `run_settings`.
    if matches!(m.screen(), Screen::Saving(_) | Screen::Settings) {
        return;
    }
    // The Touch Controls pages take their own input and draw themselves; OK or Esc goes back as the other dialogs do.
    if m.screen() == Screen::TouchHelp {
        let at = frontend::pointer(input, cam).map(|p| (p.x as i32, p.y as i32));
        let feed = crate::touchhelp::Feed {
            at,
            down: mouse_in.just_pressed(Button::Left),
            up: mouse_in.just_released(Button::Left),
            key: help_key(keys),
        };
        match m.help.step(feed) {
            crate::touchhelp::Outcome::Close => {
                if m.nav.back() {
                    close(commands);
                } else {
                    enter(m, front, stored, sim, flow, &folder.folder());
                }
            }
            crate::touchhelp::Outcome::Redraw => m.dirty = true,
            crate::touchhelp::Outcome::Same => {}
        }
        if m.dirty && m.screen() == Screen::TouchHelp {
            m.dirty = false;
            let mut p = frontend::blank();
            m.help.draw(&mut p, &front.big, &front.font);
            frontend::set_image(pictures, &m.image, p);
        }
        return;
    }
    let kind = Kind::of(flow.game);
    let title_h = front.big.height as i32;
    let line_h = front.font.height as i32 + 4;
    let built = m.build(title_h, line_h, kind);
    let at = frontend::pointer(input, cam);

    // The delayed application of the sound dialog's changes.
    if m.delay.tick() {
        stored.0 = m.draft.clone();
    }

    // Hover.
    let hot = at.and_then(|p| {
        built
            .items
            .iter()
            .position(|it| !matches!(it.kind, Kinds::Label(_) | Kinds::Lines(_)) && it.r.contains(p.x, p.y))
    });
    if hot != m.hot {
        m.hot = hot;
        m.dirty = true;
    }

    // Screen spec, "Keyboard focus" (round 75): the screen opens focused on its default button; Tab and Shift+Tab move
    // the focus over its Tab stops, wrapping, and change none of them; pressing a button gives it the focus.
    let screen = m.screen();
    let mut focus = m.focus.or_else(|| default_focus(&built, screen));
    if mouse_in.just_pressed(Button::Left)
        && let Some(i) = hot.filter(|&i| matches!(built.items[i].kind, Kinds::Button(..) | Kinds::Link(..)))
    {
        focus = Some(i);
    }
    if let (Some(k), Some(f)) = (frontend::focus_key(keys).or(input.pad_focus), focus) {
        focus = Some(crate::menuscreen::step_focus(&tab_stops(&built, screen), f, k));
    }
    if focus != m.focus {
        m.focus = focus;
        m.dirty = true;
    }

    // Screen spec, "Pressing" (round 75): a control is chosen only by releasing the mouse button on the one it went
    // down on; a held button is drawn pressed while the pointer is on it, and normal off it.
    if mouse_in.just_pressed(Button::Left) {
        m.held.down(hot);
    }
    let released = mouse_in.just_released(Button::Left).then(|| m.held.up(hot)).flatten();
    let pressed = m.held.pressed(hot);
    if pressed != m.pressed {
        m.pressed = pressed;
        m.dirty = true;
    }

    // Sliders follow the pointer while the button is held.
    if mouse_in.just_pressed(Button::Left)
        && let Some(i) = hot
        && matches!(built.items[i].kind, Kinds::Slider(..))
    {
        m.drag = Some(i);
    }
    if !mouse_in.pressed(Button::Left) {
        m.drag = None;
    }
    if let (Some(i), Some(p)) = (m.drag, at)
        && let Kinds::Slider(id, _, value, max, true) = built.items[i].kind
    {
        let t = slider_track(built.items[i].r);
        let v = menu::slider_value(p.x, t.x as f32, t.w as f32, max);
        if v != value {
            set_slider(m, id, v);
            changed(m, stored);
        }
    }

    // What was asked. A pad's confirm also toggles the focused check box or radio, which Enter leaves alone.
    let pad_pick =
        focus.filter(|&i| input.pad_confirm && matches!(built.items[i].kind, Kinds::Check(..) | Kinds::Radio(..)));
    let mut act = None;
    if let Some(i) = released.or(pad_pick) {
        match &built.items[i].kind {
            Kinds::Button(a, _) | Kinds::Link(a, _) => act = Some(*a),
            Kinds::Check(id, _, on) => {
                m.draft.set(id.bit(), !on);
                changed(m, stored);
            }
            Kinds::Radio(id, _, _) => {
                match *id {
                    Radio::FirstMouseStyle(v) => m.draft.set(bit::FIRST_MOUSE_STYLE, v),
                    Radio::Fog(v) => m.fog = v,
                    Radio::Terrain(v) => m.draft.minimap_terrain = v,
                }
                changed(m, stored);
            }
            _ => {}
        }
    }
    let settings = matches!(screen, Screen::Sound | Screen::Speed | Screen::Prefs);
    if keys.just_pressed(Key::Escape) {
        act = Some(match screen {
            Screen::Game => Act::CloseAll,
            _ if settings => Act::Cancel,
            _ => Act::Back,
        });
    } else if keys.any_just_pressed([Key::Enter, Key::NumpadEnter]) {
        // Enter activates the focused button, as a click on it does.
        act = match m.focus.map(|i| &built.items[i].kind) {
            Some(Kinds::Button(a, _) | Kinds::Link(a, _)) => Some(*a),
            _ => None,
        };
    } else if act.is_none() {
        for &k in keys.get_just_pressed() {
            if let Some(key) = game::game_key(k)
                && let Some(s) = menu::menu_key(screen, key, true)
            {
                act = Some(Act::Open(s));
            }
        }
    }

    let Some(act) = act else {
        if m.dirty {
            m.dirty = false;
            let p = draw(front, m, &built);
            frontend::set_image(pictures, &m.image, p);
        }
        return;
    };
    m.dirty = true;
    match act {
        Act::Open(s) => {
            m.nav.open_child(gate(s, sim));
            enter(m, front, stored, sim, flow, &folder.folder());
        }
        Act::CloseAll => {
            m.nav.close();
            close(commands);
        }
        Act::Settings => {
            // Back from the Settings screen returns to the game, not to the Options dialog it was reached from.
            m.nav.open(Screen::Settings);
            enter(m, front, stored, sim, flow, &folder.folder());
        }
        Act::SaveReplay => {
            let text = match sim.session.save_replay() {
                Some(name) => format!("Replay saved as {name}."),
                None => "The replay could not be saved.".to_string(),
            };
            sim.notices.push(crate::messages::Text::Own(text));
            m.nav.close();
            close(commands);
        }
        Act::Back | Act::Cancel => {
            if act == Act::Cancel {
                // Puts back what the dialog changed (the game speed and fog were never applied).
                stored.0 = m.snapshot.clone();
            }
            if m.nav.back() {
                close(commands);
            } else {
                enter(m, front, stored, sim, flow, &folder.folder());
            }
        }
        Act::Ok => {
            if screen == Screen::Speed {
                if m.speed != sim.world.speed {
                    let player = sim.human;
                    sim.pending
                        .push(tidewater_sim::Command::Speed { player, speed: m.speed });
                }
                m.draft.game_speed = m.speed;
            }
            // Game-menu spec: the fog radio is disabled in multiplayer. It changes the game state outside the command
            // stream (`set_fog_of_war` reads the local player), so it never runs in a network game.
            if screen == Screen::Prefs && m.fog != m.fog_at_open && !sim.world.rules.multiplayer {
                sim.world.set_fog_of_war(m.fog);
                sim.session.note_unsynced("the fog of war was switched in Preferences");
            }
            stored.0 = m.draft.clone();
            m.nav.close();
            close(commands);
        }
        Act::Do(c) => {
            m.nav.close();
            close(commands);
            match c {
                // Music spec, "Where music fades out", Restart Scenario (round 75): the game stops and a fade of all
                // sound of its track runs first; the mission starts again once it has ended (`frontend::run_quit`),
                // so the briefing starts its own track from its start.
                Confirm::Restart => {
                    jukebox.fade_stop(true, None);
                    *quitting = Some(frontend::Leave::Restart);
                }
                // The local player is defeated; the defeat dialog and score screen follow. Both are commands
                // (`Command::Surrender`) the sim carries out at once, as a turn without a cycle, through the session
                // (`Sim::apply_now`): the game is stopped while the menu is open, so this is the cycle it was chosen in.
                //
                // TODO(multiplayer): `net/leave-and-drop.md`, "Surrender and quit (draw)": in a network game no
                // command is sent; the machine leaves the session as its game ends and the others mark the slot out
                // at their next victory check. A quit (draw) within 120 s of game time is reported to Battle.net as
                // a draw for everyone.
                Confirm::Surrender => {
                    let player = sim.human;
                    sim.apply_now(tidewater_sim::Command::Surrender { player });
                }
                Confirm::QuitDraw => {
                    let player = sim.human;
                    sim.apply_now(tidewater_sim::Command::QuitDraw { player });
                }
                // Music spec, "Leaving a game for the main menu": the game stops and a fade of all sound runs with the
                // game's picture kept; the main menu follows once it has ended (`frontend::run_quit`). Ours: Load
                // Another Scenario and Return to Chat Room go to the main menu too, the same way.
                Confirm::LoadCustom | Confirm::ReturnToChat | Confirm::QuitToMenu => {
                    jukebox.fade_stop(true, None);
                    *quitting = Some(frontend::Leave::Menu);
                }
                Confirm::Exit => *exit = true,
            }
        }
    }
}

/// Development aid: `TIDEWATER_DEMO_SAVE=name` saves the running game as `name` at cycle 200; from cycle 300,
/// `TIDEWATER_DEMO_LOAD=name` loads that saved game (the dialogs' own paths, without typing).
pub(crate) fn demo_save_load(c: &mut Core) {
    let tick = c.sim.as_ref().map_or(0, |s| s.world.tick);
    if let Ok(name) = std::env::var("TIDEWATER_DEMO_SAVE")
        && tick == 200
    {
        let r = game::save_game(c, &saves::file_stem(&name), &name);
        eprintln!("demo save {name}: {r:?}");
    }
    if let Ok(name) = std::env::var("TIDEWATER_DEMO_LOAD")
        && tick >= 300
    {
        let folder = c.store.folder();
        let Some(sim) = &c.sim else { return };
        let enhanced = sim.world.rules.enhanced;
        match folder.read(
            &SaveDir::file_of(&saves::file_stem(&name), savefile::Kind::Single),
            enhanced,
        ) {
            Ok((header, body)) => {
                savegame::load_game(&mut c.flow, &mut c.states, header, body);
                eprintln!("demo load {name}");
            }
            Err(r) => eprintln!("demo load {name}: {r:?}"),
        }
    }
}

/// One frame of the save or load dialog: its input, drawing, and what it decided.
pub(crate) fn run_saving(c: &mut Core) {
    let Core {
        commands,
        menu: Some(m),
        menu_input: MenuInput { saving: input, .. },
        front,
        pictures,
        store: folder,
        sim: Some(sim),
        ..
    } = c
    else {
        return;
    };
    let Screen::Saving(_) = m.screen() else { return };
    let Some(ui) = m.save.as_mut() else { return };
    let snap = |u: &SaveUi| {
        (
            u.dialog.name.clone(),
            u.dialog.selected,
            u.dialog.stage.clone(),
            u.scroll,
            u.hot,
            u.held,
            u.focus,
        )
    };
    let before = snap(ui);
    let step = ui.step(input);
    let mut redraw = before != snap(ui) || m.dirty;
    match step {
        saves::Step::Nothing => {}
        saves::Step::Close => {
            if m.nav.back() {
                close(commands);
                return;
            }
        }
        saves::Step::Delete(e) => {
            // A file that can't be removed simply stays in the list.
            let _ = folder.folder().delete(&e);
            let entries = folder.folder().list(savefile::Kind::Single, sim.world.rules.enhanced);
            if let Some(ui) = m.save.as_mut() {
                ui.dialog.relist(entries);
            }
            redraw = true;
        }
        step @ (saves::Step::Write { .. } | saves::Step::Load(_)) => {
            m.request = Some(step);
        }
    }
    if redraw {
        m.dirty = false;
        if let Some(ui) = &m.save {
            let p = ui.draw(&front.font, &front.big, &|n| front.net(n));
            frontend::set_image(pictures, &m.image, p);
        }
    }
}

/// One frame of the Settings screen over the game (`settings`, not in the original; opened by Ctrl+G or the link in the
/// Options dialog). It is made on its first frame from the whole game: the live options, the stored settings, the
/// platform and the rules the game runs with. Volumes, health bars, the clock and the like apply at once when OK is
/// pressed; the rules (the Gameplay page) are shown and cannot be changed. OK, Cancel and Esc return to the game.
pub(crate) fn run_settings(c: &mut Core) {
    if c.menu.as_ref().is_none_or(|m| m.screen() != Screen::Settings) {
        return;
    }
    let (touch_only, no_intro, no_check) = (c.touch.touch_only, c.opts.no_intro, c.opts.no_update_check);
    let touch_input = c.touch.emulate || c.touch.active;
    let pad = c.pad.seen;
    let devices = crate::controls::Devices::of(&c.pad, &c.touch);
    let data = frontend::game_files_line(&c.opts);
    let mods = c.store.available_mods();
    let Core {
        commands,
        menu: Some(m),
        sim: Some(sim),
        front,
        pictures,
        stored,
        extras,
        input,
        front_screen: cam,
        ..
    } = c
    else {
        return;
    };
    if m.settings.is_none() {
        let (_, palette) = frontend::menu_art(&mut front.install);
        let env = settings::Env {
            touch: touch_only,
            touch_input,
            pad,
            devices,
            updates: crate::update::build_version().is_some(),
            data,
            mods,
            in_game: true,
        };
        let mut values = settings::Values::new(&stored.0, extras, no_intro, no_check);
        values.show_mods(&stored.0.mods, &env.mods);
        values.show_game_rules(&sim.world.rules.extras);
        m.settings = Some(InGameSettings {
            ui: settings::Screen::new(env, values),
            background: frontend::blank(),
            palette,
        });
        m.dirty = true;
    }
    let Some(ig) = m.settings.as_mut() else { return };
    let outcome = ig.ui.update(&frontend::custom_input(input, cam));
    if ig.ui.take_changed() || m.dirty {
        m.dirty = false;
        let look = crate::customsetup::Look {
            font: &front.font,
            pal: &ig.palette,
            faces: &front.button_faces,
        };
        frontend::set_image(pictures, &m.image, ig.ui.draw(&ig.background, &look, &front.small));
    }
    match outcome {
        settings::Outcome::Stay | settings::Outcome::Reimport => {}
        settings::Outcome::Ok => {
            let values = ig.ui.values().clone();
            m.nav.close();
            close(commands);
            let was_magnifying = extras.map_zoom_wheel();
            settings::commit(&values, &mut stored.0, extras, &mut c.audio);
            let turned_off = was_magnifying && !extras.map_zoom_wheel();
            c.store_settings();
            // Turned off while magnified: back to the original's view (a pinch's zoom is the player's own).
            if turned_off {
                game::reset_magnifying(c);
            }
        }
        settings::Outcome::Cancel => {
            m.nav.close();
            close(commands);
        }
        settings::Outcome::OpenFolder => {
            let file = crate::host::settings_path(&c.opts);
            let dir = file.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
            let _ = std::fs::create_dir_all(&dir);
            c.request_open(settings::folder_url(&dir));
        }
    }
}

/// Carries out a save or load the dialog asked for, at once: both need the whole game.
pub(crate) fn service_saving(c: &mut Core) {
    let Some(req) = c.menu.as_mut().and_then(|m| m.request.take()) else {
        return;
    };
    match req {
        saves::Step::Write { stem, name } => match game::save_game(c, &stem, &name) {
            Ok(()) => finish_save(c),
            Err(reason) => fail(c, reason, &stem),
        },
        saves::Step::Load(entry) => {
            let Some(sim) = &c.sim else { return };
            let enhanced = sim.world.rules.enhanced;
            match c.store.folder().read(&entry.file, enhanced) {
                Ok((header, body)) => {
                    savegame::load_game(&mut c.flow, &mut c.states, header, body);
                    remove_menu(c);
                }
                Err(reason) => fail(c, reason, &entry.stem),
            }
        }
        _ => {}
    }
}

fn remove_menu(c: &mut Core) {
    c.remove_layers_now(|e| e.role == Role::Menu);
    c.menu = None;
}

fn fail(c: &mut Core, reason: savefile::Reason, file: &str) {
    let Some(m) = &mut c.menu else { return };
    if let Some(ui) = m.save.as_mut() {
        ui.dialog.failed(reason, file);
    }
    m.dirty = true;
}

/// A save went through: the menus close; from the victory dialog the game ends as Continue does.
fn finish_save(c: &mut Core) {
    let from_outcome = c.menu.as_ref().is_some_and(|m| m.from_outcome);
    remove_menu(c);
    if from_outcome && let Some(d) = &mut c.dialog {
        d.saved = cf::after_save(true);
        d.redraw = true;
    }
}

fn set_slider(m: &mut GameMenu, id: Slide, v: u8) {
    match id {
        Slide::Music => m.draft.music_volume = v,
        Slide::Digital => m.draft.digital_volume = v,
        Slide::Game => m.speed = v,
        Slide::Mouse => m.draft.mouse_scroll = v,
        Slide::Key => m.draft.key_scroll = v,
    }
}

/// A value in the open dialog changed: the sound dialog waits out its delay, the others apply at once.
fn changed(m: &mut GameMenu, stored: &mut Stored) {
    m.dirty = true;
    if m.screen() == Screen::Sound {
        m.delay.changed();
    } else {
        stored.0 = m.draft.clone();
    }
}

/// Puts the stored settings into force in the game: the scroll speeds, the mouse style and the sound options. Bevy's
/// app put them in only when the settings had changed or the game's state was new; nothing else writes these fields,
/// so putting them in every frame comes to the same.
pub(crate) fn sync_settings(c: &mut Core) {
    let Core {
        stored,
        screen_ui: scr,
        panel_ui: ui,
        speaker,
        jukebox: music,
        ..
    } = c;
    let s = &stored.0;
    if let Some(scr) = scr {
        scr.scroller.mouse_speed = s.mouse_scroll;
        scr.scroller.key_speed = s.key_scroll;
    }
    if let Some(ui) = ui {
        ui.style = if s.has(bit::FIRST_MOUSE_STYLE) {
            mouse::Style::First
        } else {
            mouse::Style::Second
        };
    }
    if let Some(sp) = speaker {
        use crate::sound::option;
        let mask = option::UNIT_SPEECH | option::ACKNOWLEDGEMENTS | option::BUILDING_SOUNDS | option::SUBTITLES;
        sp.director.settings.volume = s.digital_level();
        sp.director.settings.options = s.bits & mask;
    }
    // Music spec, "Volume": the stored volume reaches the music when it changes (OK, the countdown's end, Ctrl+M).
    if music.volume() != s.music_volume {
        music.set_volume(s.music_volume);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu_at(screen: Screen) -> GameMenu {
        let mut nav = Nav::default();
        nav.open(screen);
        GameMenu {
            nav,
            image: crate::layers::Pictures::default().add(frontend::blank()),
            snapshot: Prefs::default(),
            draft: Prefs::default(),
            speed: 4,
            fog: true,
            fog_at_open: true,
            enhanced: true,
            extra_speeds: 0,
            can_save_replay: false,
            observer: false,
            no_exit: false,
            touch_help: false,
            lines: vec!["text".into()],
            help: Default::default(),
            delay: menu::Delay::default(),
            hot: None,
            focus: None,
            held: Default::default(),
            pressed: None,
            drag: None,
            dirty: false,
            save: None,
            settings: None,
            request: None,
            from_outcome: false,
        }
    }

    /// The label of the button a screen opens focused on.
    fn focused_label(s: Screen) -> Option<String> {
        let b = menu_at(s).build(20, 16, Kind::Campaign);
        let i = default_focus(&b, s)?;
        match &b.items[i].kind {
            Kinds::Button(_, l) => Some(l.clone()),
            _ => None,
        }
    }

    #[test]
    fn the_default_button_keeps_its_247_outline_when_the_focus_moves_off_it() {
        // Screen spec, "Buttons" (round 75): focus draws 251, the default button 247 otherwise, and other buttons none.
        assert_eq!(
            button_ring(true, true),
            Some(FOCUS_RGB),
            "a focused default: the full 251 ring"
        );
        assert_eq!(button_ring(true, false), Some(FOCUS_RGB));
        assert_eq!(button_ring(false, true), Some(DEFAULT_RGB));
        assert_eq!(button_ring(false, false), None);
        assert_eq!(DEFAULT_RGB, [160, 160, 164], "entry 247 in a game's palette");
        // Tab off OK on the Sound dialog: OK is still the default, Cancel has the focus.
        let b = menu_at(Screen::Sound).build(20, 16, Kind::Campaign);
        let stops = tab_stops(&b, Screen::Sound);
        let ok = default_focus(&b, Screen::Sound).unwrap();
        let next = crate::menuscreen::step_focus(&stops, ok, crate::menuscreen::FocusKey::Next);
        assert!(matches!(b.items[next].kind, Kinds::Button(Act::Cancel, _)));
        assert!(matches!(b.items[ok].kind, Kinds::Button(a, _) if Some(a) == default_act(Screen::Sound)));
    }

    #[test]
    fn dialogs_open_focused_on_their_default_button() {
        // Screen spec, "Buttons", "Focus" (round 74 captures).
        assert_eq!(focused_label(Screen::Game).as_deref(), Some("Return to Game (Esc)"));
        assert_eq!(focused_label(Screen::End).as_deref(), Some("Previous"));
        assert_eq!(
            focused_label(Screen::Confirm(Confirm::QuitToMenu)).as_deref(),
            Some("OK")
        );
        // Round 75 captures: OK on the Sound, Speeds, Preferences, Help (Keystroke Help) and Tips dialogs; Previous on
        // the Options and Help menus.
        for s in [
            Screen::Sound,
            Screen::Speed,
            Screen::Prefs,
            Screen::KeyHelp,
            Screen::Tips,
        ] {
            assert_eq!(focused_label(s).as_deref(), Some("OK"), "{s:?}");
        }
        for s in [Screen::Options, Screen::Help] {
            assert_eq!(focused_label(s).as_deref(), Some("Previous"), "{s:?}");
        }
        // Ours, where the spec has no capture.
        assert_eq!(focused_label(Screen::Objectives).as_deref(), Some("Previous"));
        assert_eq!(default_act(Screen::Saving(Saving::Load)), None);
    }

    /// What a screen's Tab stops are, in Tab order, by their labels.
    fn stop_names(s: Screen) -> Vec<String> {
        let b = menu_at(s).build(20, 16, Kind::Campaign);
        tab_stops(&b, s)
            .into_iter()
            .map(|i| match &b.items[i].kind {
                Kinds::Button(_, l)
                | Kinds::Link(_, l)
                | Kinds::Slider(_, l, ..)
                | Kinds::Check(_, l, _)
                | Kinds::Radio(_, l, _) => l.clone(),
                k => panic!("{k:?} takes no focus"),
            })
            .collect()
    }

    #[test]
    fn a_phone_or_tablet_has_no_exit_program_in_end_scenario() {
        let labels = |m: &GameMenu| -> Vec<String> {
            m.build(20, 16, Kind::Custom)
                .items
                .iter()
                .filter_map(|it| match &it.kind {
                    Kinds::Button(_, l) => Some(l.clone()),
                    _ => None,
                })
                .collect()
        };
        let mut m = menu_at(Screen::End);
        assert!(labels(&m).iter().any(|l| l.starts_with("Exit Program")));
        m.no_exit = true;
        let on_phone = labels(&m);
        assert!(on_phone.iter().all(|l| !l.starts_with("Exit Program")), "{on_phone:?}");
        assert!(on_phone.iter().any(|l| l == "Quit to Main Menu"));
    }

    #[test]
    fn tab_stops_on_every_control_in_the_observed_orders() {
        // Screen spec, "Keyboard focus" (round 75): Sound: OK, Cancel, the two sliders, the shown check boxes; Speeds:
        // OK, Cancel, the three sliders; Preferences: OK, Cancel, the six radios, the check box.
        assert_eq!(
            stop_names(Screen::Sound),
            [
                "OK",
                "Cancel",
                "Music volume",
                "Sound effects volume",
                "Unit speech",
                "Acknowledgements",
                "Building sounds",
                "Subtitles"
            ]
        );
        assert_eq!(
            stop_names(Screen::Speed),
            ["OK", "Cancel", "Game speed", "Mouse scroll speed", "Key scroll speed"]
        );
        let prefs = stop_names(Screen::Prefs);
        assert_eq!(prefs.len(), 2 + 6 + 1, "{prefs:?}");
        assert_eq!(prefs[..2], ["OK", "Cancel"]);
        assert_eq!(
            prefs[2..4],
            ["Right click commands", "Right click centres"],
            "each radio on its own"
        );
        assert_eq!(prefs[8], "Colour cycling");
        // The other screens' buttons go from the top; text and headings never take the focus.
        assert_eq!(stop_names(Screen::Help), ["Keystroke Help", "Tips", "Previous"]);
        assert_eq!(stop_names(Screen::Confirm(Confirm::QuitToMenu)), ["OK", "Cancel"]);
    }

    #[test]
    fn the_options_dialog_has_a_settings_link_a_finger_can_hit_that_overlaps_nothing() {
        let b = menu_at(Screen::Options).build(20, 16, Kind::Campaign);
        let link = b
            .items
            .iter()
            .find(|it| matches!(it.kind, Kinds::Link(Act::Settings, _)))
            .expect("the link");
        assert!(link.r.w >= 24 && link.r.h >= 24, "{:?}", link.r);
        assert!(matches!(&link.kind, Kinds::Link(_, t) if t.contains("Ctrl+G")));
        for it in b.items.iter().filter(|it| !std::ptr::eq(*it, link)) {
            let (a, o) = (link.r, it.r);
            assert!(
                a.y + a.h as i32 <= o.y
                    || o.y + o.h as i32 <= a.y
                    || a.x + a.w as i32 <= o.x
                    || o.x + o.w as i32 <= a.x,
                "the link overlaps {:?}",
                it.kind
            );
        }
        // Inside the dialog's frame, and the dialog still fits the screen.
        let f = b.frame;
        assert!(link.r.x >= f.x && link.r.x + link.r.w as i32 <= f.x + f.w as i32);
        assert!(link.r.y + link.r.h as i32 <= f.y + f.h as i32 && f.y >= 0 && f.y + f.h as i32 <= 480);
        // The Tab order reaches it last, after Previous; the F10 menu has no link.
        assert_eq!(
            stop_names(Screen::Options).last().map(String::as_str),
            Some(SETTINGS_LINK)
        );
        assert!(
            !menu_at(Screen::Game)
                .build(20, 16, Kind::Campaign)
                .items
                .iter()
                .any(|it| matches!(it.kind, Kinds::Link(..)))
        );
    }

    #[test]
    fn f2_stays_the_first_remembered_view_and_the_settings_key_is_free_in_the_original_table() {
        let none = mouse::KeyMods::default();
        // F2 (the key the main menu uses) is a game key: the first view.
        assert_eq!(
            mouse::key_action(mouse::Key::F(2), none, false, true, false),
            Some(mouse::KeyAction::GotoView(0))
        );
        for enhanced in [false, true] {
            for multiplayer in [false, true] {
                assert_eq!(
                    mouse::key_action(SETTINGS_KEY.0, SETTINGS_KEY.1, false, enhanced, multiplayer),
                    None,
                    "Ctrl+G does nothing in the original"
                );
            }
        }
    }

    #[test]
    fn tab_visits_every_stop_and_wraps_without_changing_any() {
        let m = menu_at(Screen::Prefs);
        let b = m.build(20, 16, Kind::Campaign);
        let stops = tab_stops(&b, Screen::Prefs);
        let start = default_focus(&b, Screen::Prefs).unwrap();
        assert_eq!(stops[0], start, "OK has the focus first");
        let mut f = start;
        for want in stops[1..].iter().chain([&start]) {
            f = crate::menuscreen::step_focus(&stops, f, crate::menuscreen::FocusKey::Next);
            assert_eq!(f, *want);
        }
        let back = crate::menuscreen::step_focus(&stops, start, crate::menuscreen::FocusKey::Back);
        assert!(
            matches!(b.items[back].kind, Kinds::Check(Tick::Colour, ..)),
            "Shift+Tab: the last stop"
        );
    }

    #[test]
    fn dialogs_fit_the_screen_and_buttons_do_not_overlap() {
        for s in [
            Screen::Game,
            Screen::Options,
            Screen::Help,
            Screen::Sound,
            Screen::Speed,
            Screen::Prefs,
            Screen::Objectives,
            Screen::End,
            Screen::Confirm(Confirm::Exit),
            Screen::Saving(Saving::Save),
            Screen::Tips,
        ] {
            let b = menu_at(s).build(20, 16, Kind::Campaign);
            assert!(b.frame.y >= 0 && b.frame.y + b.frame.h as i32 <= 480, "{s:?}");
            for (i, a) in b.items.iter().enumerate() {
                assert!(
                    a.r.y >= b.frame.y && a.r.y + a.r.h as i32 <= b.frame.y + b.frame.h as i32,
                    "{s:?}"
                );
                for c in &b.items[i + 1..] {
                    let apart = a.r.x + a.r.w as i32 <= c.r.x
                        || c.r.x + c.r.w as i32 <= a.r.x
                        || a.r.y + a.r.h as i32 <= c.r.y
                        || c.r.y + c.r.h as i32 <= a.r.y;
                    assert!(apart, "{s:?}: {:?} overlaps {:?}", a.kind, c.kind);
                }
            }
        }
    }

    #[test]
    fn the_end_menu_follows_the_kind_of_game() {
        let n = |k| {
            menu_at(Screen::End)
                .build(20, 16, k)
                .items
                .iter()
                .filter(|i| matches!(i.kind, Kinds::Button(..)))
                .count()
        };
        // Four choices and Previous.
        assert_eq!(n(Kind::Campaign), 5);
        assert_eq!(n(Kind::Custom), 5);
    }

    #[test]
    fn the_game_speed_slider_range_follows_the_rules() {
        let mut m = menu_at(Screen::Speed);
        let max = |m: &GameMenu| {
            m.build(20, 16, Kind::Campaign).items.iter().find_map(|i| match i.kind {
                Kinds::Slider(Slide::Game, _, _, max, _) => Some(max),
                _ => None,
            })
        };
        assert_eq!(max(&m), Some(8));
        m.enhanced = false;
        assert_eq!(max(&m), Some(6));
    }

    #[test]
    fn music_slider_is_disabled_without_the_music_bit() {
        let mut m = menu_at(Screen::Sound);
        m.draft.set(bit::MUSIC, false);
        let enabled = m.build(20, 16, Kind::Campaign).items.iter().find_map(|i| match i.kind {
            Kinds::Slider(Slide::Music, _, _, _, e) => Some(e),
            _ => None,
        });
        assert_eq!(enabled, Some(false));
    }
}
