//! Tidewater's own Settings screen: the optional improvements (`extras`), the window, the mouse lock, the intro video,
//! the update check and the game files, changed in the game instead of in `settings.txt` or on the command line.
//! Not part of the original, so it is opened by a key and a link on the main menu (F2 or "Settings (F2)"), which leave
//! the original's menus as they were (`docs/design/extras.md`, "The Settings screen"). SDL-free: `frontend` opens the
//! screen, feeds it the frame's input ([`Input`]) and draws what [`Screen::draw`] gives, as for the Custom Scenario
//! screen (`customsetup`).
//!
//! **Look.** The menu picture, the game's fonts, the menu buttons' faces and colours (`menuscreen`) on a black panel
//! like the small dialogs'. A value is chosen in the same drop-down as the Custom Scenario screen's boxes
//! (`customsetup::Look::drop`, the open list `mpwidgets::Popup`). The description of the focused or hovered row is in
//! the small font under the rows.
//!
//! **Generated from the registry.** The rows of the Interface, Sound and Gameplay pages, and the Scaling and Widescreen
//! rows of the General page, come from [`extras::REGISTRY`]: the row's name is [`extras::Spec::label`], its values come
//! from the kind, its description is the registry's. Only the page each option is on ([`extra_page`]) and its values'
//! words ([`CHOICE_WORDS`]) are written here; the page's `match` has no catch-all arm, so a new option does not build
//! until it is on a page, and a test checks that every option is on exactly one.
//!
//! **The Keys page** (desktop and any build with a keyboard; hidden on touch-only platforms) lists every rebindable
//! action (`keymap::rows`) with its key in a scrolling list in the Custom Scenario dialog's look. Enter or a click on a
//! row asks for the next key press ("Press a key... (Esc cancels)"), which binds it unless `keymap`'s rules refuse it:
//! a key another action has (or an action that was not rebound still uses) is not bound and the description names the
//! action that has it, so two actions swap keys by moving one away first ([`keymap::with_binding`]). Backspace or Delete
//! gives a row its original key back and **Reset keys** every row. The screen edits the `[keys]` section's lines; OK
//! stores them in the prefs' keys, so `settings.txt` holds them as the file's own text.
//!
//! **The Mods page** (`tidewater_formats::mods`, `docs/mods.md`) is shown only when the mods folder holds a mod
//! (`Env::mods`). It has six rows, [`Item::ModSlot`], each choosing one mod or None from the folders there are; a later
//! row overrides an earlier one. OK writes the rows' mods, in order and once each, as the `[mods]` section; they apply
//! from the next start, because the installation is opened once at start-up.
//!
//! **What OK does.** The screen edits a [`Values`] copy. OK ([`commit`]) writes only the rows that were changed (or
//! reset by Defaults) into the settings file's view and into the run's live values, so an option the command line set
//! for this run stays as it is unless the player chose it here; the frontend then writes `settings.txt` at once.
//! Cancel and Esc throw the copy away.
//!
//! Choices of ours where nothing else says (the screen is ours): the tabs are the pages (five; the Keys page is not on a phone, and the Mods page shows only with a mod to enable), the selected one drawn
//! pressed; Up and Down move the focus like Tab and Shift+Tab; Left and Right change the focused drop-down's value, move
//! between the tabs, or between OK, Cancel and Defaults; PageUp and PageDown change page; O, C and D press the
//! bottom buttons.

use tidewater_formats::fnt::Font;
use tidewater_formats::mods::ModInfo;

use crate::audio::Audio;
use crate::customsetup::{DropState, Input, Look};
use crate::extras::{self, ClientExtras, Id, Kind, REGISTRY, Scope};
use crate::hud::{self, Rect};
use crate::input::Key;
use crate::keymap::{self, Chord, KeyMap};
use crate::menuscreen::{self, ButtonState};
use crate::mouse::KeyMods;
use crate::mpwidgets::{self, Popup, PopupKey, Ptr};
use crate::prefs::{Prefs, bit};
use crate::scene::Picture;

// ---------------------------------------------------------------------------------------------------------------
// Pages and rows

/// A page of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    General,
    Interface,
    Sound,
    Gameplay,
    /// The mods to enable (`tidewater_formats::mods`); shown only when there is a mod to enable.
    Mods,
    Keys,
    Touch,
    Controller,
}

impl Page {
    pub(crate) const ALL: [Page; 8] = [
        Page::General,
        Page::Interface,
        Page::Sound,
        Page::Gameplay,
        Page::Mods,
        Page::Keys,
        Page::Touch,
        Page::Controller,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Interface => "Interface",
            Page::Sound => "Sound",
            Page::Gameplay => "Gameplay",
            Page::Mods => "Mods",
            Page::Keys => "Keys",
            Page::Touch => "Touch",
            Page::Controller => "Controller",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Page::General => {
                "The window, the picture, the mouse and start-up. The game files Tidewater runs on are here too."
            }
            Page::Interface => "Extra help on the game screen. Each one is off until you turn it on.",
            Page::Sound => {
                "How loud the unit voices, the sound effects and the music are, as a share of the game's own volume."
            }
            Page::Gameplay => "Rule changes for the games you start. Point at a row to read what it does.",
            Page::Mods => {
                "Mods change the game's files: pictures, sounds, maps and data. Choose which to use; each row below \
                 overrides the ones above it."
            }
            Page::Keys => {
                "The keys of the game screen. Choose an action and press Enter, or click it, then press the new key. \
                 Backspace puts the original key back."
            }
            Page::Touch => "How your fingers work the game. Point at a row to read what it does.",
            Page::Controller => "How the controller works the game. Point at a row to read what it does.",
        }
    }
}

/// The pages the platform shows, in tab order: the Keys page needs a keyboard, the Touch page touch, the Controller
/// page a controller seen in this run.
pub(crate) fn pages(env: &Env) -> Vec<Page> {
    Page::ALL
        .into_iter()
        .filter(|&p| match p {
            Page::Keys => !env.touch,
            Page::Mods => !env.mods.is_empty(),
            Page::Touch => env.touch || env.touch_input,
            Page::Controller => env.pad,
            _ => true,
        })
        .collect()
}

/// The page an option is on. No catch-all arm: a new option has to be put on a page to build.
pub(crate) fn extra_page(id: Id) -> Page {
    match id {
        Id::Scaling | Id::Widescreen | Id::Controls => Page::General,
        Id::HealthBars
        | Id::IdleWorker
        | Id::GameClock
        | Id::ControlGroupBar
        | Id::SmartAttackMove
        | Id::MinimapPing
        | Id::AutoReplay
        | Id::MapZoom => Page::Interface,
        Id::VolumeVoices | Id::VolumeEffects | Id::VolumeMusic => Page::Sound,
        Id::RallyPoints | Id::ProductionQueue | Id::SelectionLimit | Id::SpeedLevels | Id::ShiftQueue => Page::Gameplay,
        Id::TouchDrag
        | Id::TouchSmartTap
        | Id::TouchTwoFinger
        | Id::TouchPinch
        | Id::TouchHold
        | Id::TouchButtons
        | Id::TouchSwapBars
        | Id::TouchMenuTap => Page::Touch,
        Id::PadSpeed | Id::PadAcceleration | Id::PadDeadZone | Id::PadSwap | Id::PadHideTouch => Page::Controller,
    }
}

/// One setting with a drop-down: an option of the registry or one of the stored ones that are not options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    Extra(Id),
    /// Window or full screen (`Prefs::fullscreen`).
    Window,
    /// The mouse lock during a game (`Prefs::mouse_lock`).
    MouseLock,
    /// The introduction video at start-up (`prefs::bit::OPENING_FILM`).
    IntroVideo,
    /// The check for a newer release at start-up (`Prefs::update_check`).
    UpdateCheck,
    /// The mod used in place `n` of the Mods page (0 is the first, overridden by the places after it).
    ModSlot(u8),
}

/// How many mods the Mods page can enable (more can be listed in `settings.txt`).
pub(crate) const MOD_SLOTS: u8 = 6;

/// The rows that are not registry options, after the registry's.
const OTHER_ITEMS: [Item; 4 + MOD_SLOTS as usize] = [
    Item::Window,
    Item::MouseLock,
    Item::IntroVideo,
    Item::UpdateCheck,
    Item::ModSlot(0),
    Item::ModSlot(1),
    Item::ModSlot(2),
    Item::ModSlot(3),
    Item::ModSlot(4),
    Item::ModSlot(5),
];

/// How many settings the screen edits.
const ITEMS: usize = extras::COUNT + OTHER_ITEMS.len();

fn index(item: Item) -> usize {
    match item {
        Item::Extra(id) => id as usize,
        other => extras::COUNT + OTHER_ITEMS.iter().position(|&i| i == other).unwrap_or(0),
    }
}

fn item_at(i: usize) -> Item {
    if i < extras::COUNT {
        Item::Extra(REGISTRY[i].id)
    } else {
        OTHER_ITEMS[i - extras::COUNT]
    }
}

/// The rows of a page in the order they are shown, before the platform's filter ([`shown`]). The General page leads
/// with the window row, then the registry's options for it, then the stored switches.
pub(crate) fn items_on(page: Page) -> Vec<Item> {
    let mut v = Vec::new();
    if page == Page::General {
        v.push(Item::Window);
    }
    v.extend(
        REGISTRY
            .iter()
            .filter(|s| extra_page(s.id) == page)
            .map(|s| Item::Extra(s.id)),
    );
    if page == Page::General {
        v.extend([Item::MouseLock, Item::IntroVideo, Item::UpdateCheck]);
    }
    if page == Page::Mods {
        v.extend((0..MOD_SLOTS).map(Item::ModSlot));
    }
    v
}

/// What the platform the screen runs on allows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Env {
    /// A phone or tablet (`Core::touch.touch_only`): no window, mouse lock or settings folder rows.
    pub touch: bool,
    /// Touch is in use on a computer (a touch screen, `--touch-emulation`): the Touch page shows, as on a phone.
    pub touch_input: bool,
    /// A controller has been connected in this run: the Controller page shows.
    pub pad: bool,
    /// The input devices there are now: the Controls row offers only the choices they allow (`offered`).
    pub devices: crate::controls::Devices,
    /// The program checks for a newer release at all (`update`): a desktop release build.
    pub updates: bool,
    /// The line about the imported game files, `None` when there is nothing to manage (`--install` reads the files in
    /// place): without it there are no game-files row and no "Import game files again..." button.
    pub data: Option<String>,
    /// The mods that could be enabled (`Storage::available_mods`), by folder name. The Mods page shows when there are
    /// any.
    pub mods: Vec<ModInfo>,
    /// The screen is open over a running game (`gamemenu`): the Gameplay page shows the rules the game started with and
    /// cannot change them, and there is no "Import game files again...".
    pub in_game: bool,
}

/// Whether the row is shown here.
pub(crate) fn shown(item: Item, env: &Env) -> bool {
    match item {
        Item::Extra(_) | Item::IntroVideo => true,
        Item::Window | Item::MouseLock => !env.touch,
        Item::UpdateCheck => env.updates,
        Item::ModSlot(_) => !env.mods.is_empty(),
    }
}

/// Whether the row is shown but cannot be changed here: during a game the Gameplay page's options are the game's rules
/// (`Rules::extras`), fixed when it started.
pub(crate) fn read_only(item: Item, env: &Env) -> bool {
    env.in_game && matches!(item, Item::Extra(id) if extra_page(id) == Page::Gameplay)
}

/// A button that is not a drop-down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Open the folder with `settings.txt` in the system's file manager.
    OpenFolder,
    /// Open the first-run import screen again.
    Reimport,
    /// Every key back to the original's (the Keys page).
    ResetKeys,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::OpenFolder => "Open settings folder",
            Action::Reimport => "Import game files again...",
            Action::ResetKeys => "Reset keys",
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Values

/// The words of each choice option's values, in the order of its registry names (`extras::Kind::Choice`).
pub(crate) const CHOICE_WORDS: &[(Id, &[&str])] = &[
    (Id::HealthBars, &["Off", "Hovered", "Selected", "Always"]),
    (Id::GameClock, &["Off", "Clock", "Clock and APM"]),
    (Id::Scaling, &["Automatic", "Whole numbers", "Sharp (fills)"]),
    (Id::SelectionLimit, &["9 (original)", "12", "18", "Unlimited"]),
    (Id::SpeedLevels, &["Off", "1 more", "2 more", "3 more"]),
    (Id::AutoReplay, &["Off", "Save from menu", "Always save"]),
    (Id::Widescreen, &["Off", "16:10", "16:9", "Auto"]),
    (Id::MapZoom, &["Off", "Mouse wheel"]),
    (Id::TouchDrag, &["Pan", "Pan, no glide", "Selection box"]),
    (Id::TouchSmartTap, &["On", "Off"]),
    (Id::TouchTwoFinger, &["Pan", "Box corners", "Off"]),
    (Id::TouchPinch, &["Auto (phones)", "On", "Off"]),
    (Id::TouchHold, &["Normal", "Long", "Longer"]),
    (Id::TouchButtons, &["Medium", "Small", "Large"]),
    (Id::TouchMenuTap, &["On", "Off"]),
    (Id::PadSpeed, &["Normal", "Slow", "Fast"]),
    (Id::PadAcceleration, &["On", "Off"]),
    (Id::PadDeadZone, &["Normal", "Small", "Large"]),
    (Id::PadHideTouch, &["On", "Off"]),
    (
        Id::Controls,
        &[
            "Automatic",
            "Keyboard and mouse",
            "Touch",
            "Controller",
            "Controller and touch",
        ],
    ),
];

/// The default of an item.
fn default_of(item: Item) -> u8 {
    match item {
        Item::Extra(id) => id.spec().default,
        Item::Window => 0,
        Item::MouseLock | Item::IntroVideo | Item::UpdateCheck => 1,
        Item::ModSlot(_) => 0,
    }
}

/// The values a drop-down offers, as (stored number, words), for an item whose value now is `current`.
pub(crate) fn options(item: Item, current: u8) -> Vec<(u8, String)> {
    let on_off = || vec![(0, "Off".to_string()), (1, "On".to_string())];
    match item {
        Item::Extra(id) => match id.spec().kind {
            Kind::Flag => on_off(),
            Kind::Choice(names) => {
                let words = CHOICE_WORDS.iter().find(|(i, _)| *i == id).map(|(_, w)| *w);
                (0..names.len())
                    .map(|i| {
                        let w = words.and_then(|w| w.get(i)).copied().unwrap_or(names[i]);
                        (i as u8, w.to_string())
                    })
                    .collect()
            }
            Kind::Percent => {
                let mut v: Vec<u8> = (0..=10).map(|i| i * 10).collect();
                // A stored value that is not a multiple of 10 is shown and kept until it is changed.
                if !v.contains(&current) {
                    v.push(current);
                    v.sort_unstable();
                }
                v.into_iter().map(|n| (n, format!("{n}%"))).collect()
            }
        },
        Item::Window => vec![(0, "Window".to_string()), (1, "Full screen".to_string())],
        Item::MouseLock | Item::IntroVideo | Item::UpdateCheck => on_off(),
        // The mods are the environment's: see [`mod_options`].
        Item::ModSlot(_) => vec![(0, "None".to_string())],
    }
}

/// What a Mods page row offers: "None", then every mod there is (value `n` is `mods[n - 1]`).
fn mod_options(env: &Env) -> Vec<(u8, String)> {
    std::iter::once((0, "None".to_string()))
        .chain(
            env.mods
                .iter()
                .take(usize::from(u8::MAX))
                .enumerate()
                .map(|(i, m)| (i as u8 + 1, mod_words(m))),
        )
        .collect()
}

/// A mod in the drop-down: its title and version, cut to fit the box.
fn mod_words(m: &ModInfo) -> String {
    let full = if m.version.is_empty() {
        m.display().to_string()
    } else {
        format!("{} {}", m.display(), m.version)
    };
    if full.chars().count() > 24 {
        format!("{}..", full.chars().take(22).collect::<String>())
    } else {
        full
    }
}

/// The values a drop-down offers now: [`options`] without the choices of `controls` whose devices are not there
/// (`controls::Devices::available`). Every other row offers everything.
pub(crate) fn offered(item: Item, current: u8, env: &Env) -> Vec<(u8, String)> {
    if matches!(item, Item::ModSlot(_)) {
        return mod_options(env);
    }
    let mut v = options(item, current);
    if item == Item::Extra(Id::Controls) {
        v.retain(|(n, _)| env.devices.available(crate::controls::Mode::from_index(*n)));
    }
    v
}

/// The description of a row for the value it has now: the registry's, and for `controls` a note when the stored
/// choice cannot be used because its devices are not there.
fn describe_now(item: Item, current: u8, env: &Env) -> String {
    let mut text = describe(item);
    if matches!(item, Item::ModSlot(_))
        && let Some(m) = usize::from(current).checked_sub(1).and_then(|i| env.mods.get(i))
    {
        let mut about = format!("{}: ", m.display());
        about.push_str(if m.description.is_empty() {
            "no description."
        } else {
            &m.description
        });
        if !m.authors.is_empty() {
            about.push_str(&format!(" By {}.", m.authors.join(", ")));
        }
        text = format!("{about} {text}");
    }
    if item == Item::Extra(Id::Controls) && !env.devices.available(crate::controls::Mode::from_index(current)) {
        text.push_str(" Its device is not here now, so Automatic is in use until it is back.");
    }
    text
}

/// The settings as the screen edits them: a number for each item, and which items were changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Values {
    v: [u8; ITEMS],
    touched: [bool; ITEMS],
    /// The `[keys]` section's lines as the Keys page edits them, and whether it changed them.
    keys: Vec<String>,
    keys_touched: bool,
    /// The folder names of the mods the Mods page offers (value `n` of a mod row is `mod_folders[n - 1]`).
    mod_folders: Vec<String>,
}

impl Values {
    /// The values now in force: the run's live options (the settings file's, then the command line's), the stored
    /// switches, and the intro video and update check as the command line left them (`--no-intro`,
    /// `--no-update-check`).
    pub(crate) fn new(prefs: &Prefs, live: &ClientExtras, no_intro: bool, no_update_check: bool) -> Values {
        let mut v = [0; ITEMS];
        for s in &REGISTRY {
            v[s.id as usize] = live.get(s.id);
        }
        v[index(Item::Window)] = u8::from(prefs.fullscreen);
        v[index(Item::MouseLock)] = u8::from(prefs.mouse_lock);
        v[index(Item::IntroVideo)] = u8::from(prefs.has(bit::OPENING_FILM) && !no_intro);
        v[index(Item::UpdateCheck)] = u8::from(prefs.update_check && !no_update_check);
        Values {
            v,
            touched: [false; ITEMS],
            keys: prefs.keys.lines().to_vec(),
            keys_touched: false,
            mod_folders: Vec::new(),
        }
    }

    /// Puts the enabled mods `enabled` (folder names, lowest priority first) in the Mods page's rows, out of the
    /// mods there are.
    pub(crate) fn show_mods(&mut self, enabled: &[String], available: &[ModInfo]) {
        self.mod_folders = available.iter().map(|m| m.folder.clone()).collect();
        let places = enabled
            .iter()
            .filter_map(|e| self.mod_folders.iter().position(|f| f.eq_ignore_ascii_case(e)))
            .take(usize::from(MOD_SLOTS));
        for k in 0..MOD_SLOTS {
            self.v[index(Item::ModSlot(k))] = 0;
        }
        for (k, place) in places.enumerate() {
            self.v[index(Item::ModSlot(k as u8))] = place as u8 + 1;
        }
    }

    /// The lines of the `[keys]` section being edited.
    pub(crate) fn key_lines(&self) -> &[String] {
        &self.keys
    }

    fn set_key_lines(&mut self, lines: Vec<String>) {
        self.keys = lines;
        self.keys_touched = true;
    }

    pub(crate) fn get(&self, item: Item) -> u8 {
        self.v[index(item)]
    }

    pub(crate) fn set(&mut self, item: Item, value: u8) {
        self.v[index(item)] = value;
        self.touched[index(item)] = true;
    }

    /// Shows the rules of the running game on the Gameplay rows (they are not touched, so OK never writes them).
    pub(crate) fn show_game_rules(&mut self, rules: &tidewater_sim::Extras) {
        for (id, v) in ClientExtras::of_sim(rules) {
            self.v[id as usize] = v;
        }
    }

    /// Every shown row that can be changed back to its default.
    fn defaults(&mut self, env: &Env) {
        for i in 0..ITEMS {
            let item = item_at(i);
            if shown(item, env) && !read_only(item, env) {
                self.set(item, default_of(item));
            }
        }
    }

    /// Writes the changed rows into the settings file's view and the run's live options, and the edited keys into the
    /// stored keys, which the game reads at once.
    pub(crate) fn apply(&self, stored: &mut Prefs, live: &mut ClientExtras) {
        if self.keys_touched {
            (stored.keys, stored.key_problems) = KeyMap::parse(&self.keys);
        }
        for i in (0..ITEMS).filter(|&i| self.touched[i]) {
            let v = self.v[i];
            match item_at(i) {
                Item::Extra(id) => {
                    stored.extras.set(id, v);
                    live.set(id, v);
                }
                Item::Window => stored.fullscreen = v != 0,
                Item::MouseLock => stored.mouse_lock = v != 0,
                Item::IntroVideo => stored.set(bit::OPENING_FILM, v != 0),
                Item::UpdateCheck => stored.update_check = v != 0,
                Item::ModSlot(_) => {}
            }
        }
        // The mods are one list: any row changed writes all of them, in order, each mod once.
        if (0..MOD_SLOTS).any(|k| self.touched[index(Item::ModSlot(k))]) {
            let mut names: Vec<String> = Vec::new();
            for k in 0..MOD_SLOTS {
                let n = usize::from(self.v[index(Item::ModSlot(k))]);
                if let Some(f) = n.checked_sub(1).and_then(|i| self.mod_folders.get(i))
                    && !names.contains(f)
                {
                    names.push(f.clone());
                }
            }
            stored.mods = names;
        }
    }
}

/// What OK does with the values: the changed rows into the stored settings and the live options, and the mixer's
/// volumes at once. The caller writes the settings file.
pub(crate) fn commit(values: &Values, stored: &mut Prefs, live: &mut ClientExtras, audio: &mut Audio) {
    values.apply(stored, live);
    audio.set_trims(live.trims());
}

// ---------------------------------------------------------------------------------------------------------------
// Descriptions

/// The note on the Gameplay page during a game.
pub(crate) const GAMEPLAY_NOTE_IN_GAME: &str = "Fixed for this game: they were set when it started.";

/// The note on the Mods page.
const MODS_NOTE: &str = "Mods live in the mods folder next to the game files (see docs/mods.md). They apply from the next \
                        start, and a replay or network game needs the same ones.";

/// The note on the Gameplay page.
const GAMEPLAY_NOTE: &str = "These options change the game rules. They apply to single-player games you start; in a \
                             multiplayer game the host chooses them in the lobby.";

/// The description of a row, shown under the rows.
pub(crate) fn describe(item: Item) -> String {
    match item {
        Item::Extra(id) => {
            let s = id.spec();
            let when = match (s.scope, id) {
                (_, Id::Widescreen) => " Applies from the next game you start.",
                (Scope::Sim, _) => " Applies from the next game you start.",
                _ => "",
            };
            format!("{}.{when}", s.description.trim_end_matches('.'))
        }
        Item::Window => "Run Tidewater in a window or full screen. Applies at once.".to_string(),
        Item::MouseLock => "Keep the pointer inside the window during a game so it cannot slip out at the edge. Hold \
                            Cmd (the Windows key elsewhere) to free it. The --no-mouse-lock flag turns it off for one run."
            .to_string(),
        Item::IntroVideo => {
            "Play the introduction video when Tidewater starts. Takes effect the next time it starts.".to_string()
        }
        Item::UpdateCheck => "Look for a newer Tidewater release when it starts and offer it on the main menu. Takes \
                              effect the next time it starts."
            .to_string(),
        Item::ModSlot(_) => "Mods apply the next time Tidewater starts. A mod in a lower row overrides the ones above it \
                             and the game's own files. In a network game everyone needs the same mods."
            .to_string(),
    }
}

fn describe_action(a: Action) -> &'static str {
    match a {
        Action::OpenFolder => {
            "Open the folder that holds settings.txt in your file manager. Keys are rebound in the [keys] section of \
             that file."
        }
        Action::ResetKeys => {
            "Put every key back to the original's. Lines in the [keys] section that are not keys, such as comments, stay. \
             Cancel still throws the change away."
        }
        Action::Reimport => {
            "Choose the game files again, for example after reinstalling the game. Esc on that screen leaves \
             everything as it was. Changes made here are discarded."
        }
    }
}

/// The line about the imported game files, from the import's manifest.
pub(crate) fn data_summary(m: &tidewater_import::Manifest) -> String {
    let how = match m.source_kind.as_str() {
        "install-folder" => "game folder",
        "single-mpq" => "one archive",
        "mpq-folder" => "folder of archives",
        "gog-installer" => "GOG installer",
        "cd-image" => "CD image",
        _ => "an earlier import",
    };
    let maps = m
        .files
        .iter()
        .filter(|f| {
            let p = f.path.to_ascii_lowercase();
            p.starts_with("maps/") && p.ends_with(".pud")
        })
        .count();
    let mut s = format!(
        "Game files: {how}, {} files, {maps} maps, imported {}",
        m.files.len(),
        m.imported.split('T').next().unwrap_or("")
    );
    if let Some(v) = &m.known_version {
        s.push_str(&format!(" ({v})"));
    }
    s
}

/// The `file://` address of a folder, for the system to open in its file manager (SDL's `SDL_OpenURL`). Letters,
/// digits and `-._~/:` stay as they are, every other byte of the path is `%XX`; a Windows path's backslashes become
/// slashes and its drive letter follows three slashes.
pub(crate) fn folder_url(dir: &std::path::Path) -> String {
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let text = dir.to_string_lossy().replace('\\', "/");
    let mut url = String::from("file://");
    if !text.starts_with('/') {
        url.push('/');
    }
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// The line when the files are imported but there is no readable record of the import.
pub(crate) const DATA_UNKNOWN: &str = "Game files: imported (no record of where from)";

// ---------------------------------------------------------------------------------------------------------------
// Layout (ours: the screen is not in the original)

const PANEL: Rect = Rect::new(40, 24, 560, 424);
const HEADING: Rect = Rect::new(40, 34, 560, 20);
const TAB_Y: i32 = 62;
const TAB_W: u32 = 100;
const TAB_H: u32 = 28;
const TAB_GAP: i32 = 8;
const ROWS_TOP: i32 = 100;
const ROW_X: i32 = 56;
const ROW_W: u32 = 528;
/// A row's tap target; rows are 30 pixels apart.
const ROW_H: u32 = 28;
const ROW_PITCH: i32 = 30;
/// The General page has seven rows and the game files' line and buttons under them: its rows are 28 apart so the
/// buttons clear the description.
const GENERAL_PITCH: i32 = 28;
const LABEL_X: i32 = 64;
const DROP_X: i32 = 352;
const DROP_W: u32 = 224;
const DROP_H: u32 = 24;
const TEXT_LINE: i32 = 14;
const ACTION_W: u32 = 224;
const ACTION_GAP: i32 = 16;
const DESC: Rect = Rect::new(ROW_X, 344, ROW_W, 4 * TEXT_LINE as u32);
const BUTTON_Y: i32 = 410;
const BUTTON_W: u32 = 106;
const BUTTON_H: u32 = 28;
const OK: Rect = Rect::new(470, BUTTON_Y, BUTTON_W, BUTTON_H);
const CANCEL: Rect = Rect::new(356, BUTTON_Y, BUTTON_W, BUTTON_H);
const DEFAULTS: Rect = Rect::new(64, BUTTON_Y, BUTTON_W, BUTTON_H);

/// A tab: [`TAB_W`] wide, narrower when that many do not fit the panel.
fn tab_rect(p: Page, pages: &[Page]) -> Rect {
    let n = pages.len().max(1) as i32;
    let room = PANEL.w as i32 - 2 * TAB_GAP - (n - 1) * TAB_GAP;
    let w = (TAB_W as i32).min(room / n);
    let total = n * w + (n - 1) * TAB_GAP;
    let i = pages.iter().position(|&x| x == p).unwrap_or(0) as i32;
    Rect::new(
        PANEL.x + (PANEL.w as i32 - total) / 2 + i * (w + TAB_GAP),
        TAB_Y,
        w as u32,
        TAB_H,
    )
}

/// The drop-down box inside a row's tap target.
fn drop_rect(row: Rect) -> Rect {
    Rect::new(DROP_X, row.y + (row.h as i32 - DROP_H as i32) / 2, DROP_W, DROP_H)
}

// The Keys page: the list of actions (ours; the look is the Select scenario dialog's list and scroll bar).
const KEYS_LIST: Rect = Rect::new(56, 100, 512, 192);
const KEYS_TRACK: Rect = Rect::new(568, 100, 16, 192);
const KEYS_LINES: usize = 8;
const KEYS_LINE_H: i32 = 24;
/// Where a row's key starts, and where its action's name starts.
const KEYS_KEY_X: i32 = 392;
const KEYS_NAME_X: i32 = 64;
const KEYS_RESET: Rect = Rect::new(56, 300, 160, 28);

/// Something the pointer can be on or the focus can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Tab(Page),
    Row(Item),
    Action(Action),
    Ok,
    Cancel,
    Defaults,
    /// A line of the open drop-down's list.
    PopupRow(usize),
    /// A row of the Keys page's list (an index into `keymap::rows`), shown or not scrolled out.
    KeyRow(usize),
    /// The Keys page's scroll bar above (false) or below (true) its thumb.
    KeyTrack(bool),
    /// The Keys page's list as a whole: where the focus is while an action of it is chosen. Not under the pointer.
    KeyList,
}

/// The scroll bar's thumb for a list of `n` rows whose first shown row is `top`.
fn key_thumb(top: usize, n: usize) -> Rect {
    let t = KEYS_TRACK;
    if n <= KEYS_LINES {
        return t;
    }
    let h = (t.h as usize * KEYS_LINES / n).max(12) as u32;
    let y = t.y + ((t.h - h) as usize * top / (n - KEYS_LINES)) as i32;
    Rect::new(t.x, y, t.w, h)
}

/// A page's targets and its fixed text lines, from the platform.
struct Layout {
    targets: Vec<(Target, Rect)>,
    /// Text in the small font: the lines to draw and the rectangle they wrap in.
    texts: Vec<(String, Rect)>,
}

fn layout(page: Page, env: &Env) -> Layout {
    let mut targets = Vec::new();
    let mut texts = Vec::new();
    let mut y = ROWS_TOP;
    let pitch = if page == Page::General {
        GENERAL_PITCH
    } else {
        ROW_PITCH
    };
    for item in items_on(page).into_iter().filter(|&i| shown(i, env)) {
        targets.push((Target::Row(item), Rect::new(ROW_X, y, ROW_W, ROW_H)));
        y += pitch;
    }
    if page == Page::General {
        if let Some(line) = &env.data {
            texts.push((line.clone(), Rect::new(LABEL_X, y + 2, ROW_W - 16, TEXT_LINE as u32)));
            y += TEXT_LINE + 6;
        }
        let actions: Vec<Action> = [
            (!env.touch, Action::OpenFolder),
            (env.data.is_some() && !env.in_game, Action::Reimport),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, a)| a)
        .collect();
        for (i, a) in actions.into_iter().enumerate() {
            targets.push((
                Target::Action(a),
                Rect::new(ROW_X + i as i32 * (ACTION_W as i32 + ACTION_GAP), y, ACTION_W, ROW_H),
            ));
        }
    }
    if page == Page::Mods {
        texts.push((
            MODS_NOTE.to_string(),
            Rect::new(LABEL_X, y + 6, ROW_W - 16, 3 * TEXT_LINE as u32),
        ));
    }
    if page == Page::Gameplay {
        texts.push((
            if env.in_game {
                GAMEPLAY_NOTE_IN_GAME
            } else {
                GAMEPLAY_NOTE
            }
            .to_string(),
            Rect::new(LABEL_X, y + 6, ROW_W - 16, 3 * TEXT_LINE as u32),
        ));
    }
    Layout { targets, texts }
}

/// The Mods page's row labels.
const MOD_LABELS: [&str; MOD_SLOTS as usize] = [
    "First mod",
    "Second mod",
    "Third mod",
    "Fourth mod",
    "Fifth mod",
    "Sixth mod",
];

/// The text of `s` in lines no wider than `max` pixels in `font` (spaces leaving [`menuscreen::SPACE_10X`] pixels).
pub(crate) fn wrap(font: &Font, s: &str, max: u32) -> Vec<String> {
    let width = |t: &str| menuscreen::text_width(font, t, menuscreen::SPACE_10X);
    let mut lines: Vec<String> = Vec::new();
    for word in s.split_whitespace() {
        match lines.last_mut() {
            Some(last) if width(&format!("{last} {word}")) <= max => {
                last.push(' ');
                last.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

// ---------------------------------------------------------------------------------------------------------------
// The screen

/// What a frame of the screen asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Stay,
    /// OK: apply and save ([`commit`]), then back to the main menu.
    Ok,
    /// Cancel or Esc: back to the main menu, the changes thrown away.
    Cancel,
    /// Open the settings folder.
    OpenFolder,
    /// Open the import screen again.
    Reimport,
}

/// An open drop-down list.
struct Open {
    item: Item,
    popup: Popup,
    /// The stored number of each line.
    values: Vec<u8>,
}

pub(crate) struct Screen {
    env: Env,
    values: Values,
    page: Page,
    focus: Target,
    open: Option<Open>,
    ptr: Ptr<Target>,
    changed: bool,
    keys: KeysUi,
}

/// The Keys page's state.
struct KeysUi {
    rows: Vec<keymap::Row>,
    /// The keys as the lines now stand.
    map: KeyMap,
    sel: usize,
    /// The first row shown.
    top: usize,
    /// The next key press binds the selected row.
    capturing: bool,
    /// What the last try said (a conflict, a key that cannot be used, what was bound); shown in the description.
    note: Option<String>,
}

impl Screen {
    /// The screen as it opens, on the General page with the first tab focused.
    pub(crate) fn new(env: Env, values: Values) -> Screen {
        let keys = KeysUi {
            rows: keymap::rows(),
            map: KeyMap::parse(values.key_lines()).0,
            sel: 0,
            top: 0,
            capturing: false,
            note: None,
        };
        Screen {
            env,
            values,
            keys,
            page: Page::General,
            focus: Target::Tab(Page::General),
            open: None,
            ptr: Ptr::default(),
            changed: true,
        }
    }

    pub(crate) fn values(&self) -> &Values {
        &self.values
    }

    #[cfg(test)]
    pub(crate) fn page(&self) -> Page {
        self.page
    }

    pub(crate) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Every target of the layer that takes the pointer, topmost first.
    fn targets(&self) -> Vec<(Target, Rect)> {
        if let Some(o) = &self.open {
            return (0..o.popup.labels.len())
                .map(|i| (Target::PopupRow(i), o.popup.row_rect(i)))
                .collect();
        }
        self.page_targets()
    }

    /// The targets of the page and the fixed ones, in Tab order.
    fn page_targets(&self) -> Vec<(Target, Rect)> {
        let pages = pages(&self.env);
        let mut v: Vec<(Target, Rect)> = pages.iter().map(|&p| (Target::Tab(p), tab_rect(p, &pages))).collect();
        v.extend(layout(self.page, &self.env).targets);
        if self.page == Page::Keys {
            v.extend(self.key_targets());
        }
        v.extend([(Target::Ok, OK), (Target::Cancel, CANCEL), (Target::Defaults, DEFAULTS)]);
        v
    }

    /// The Keys page's targets: the shown rows, the two parts of the scroll bar and the Reset keys button.
    fn key_targets(&self) -> Vec<(Target, Rect)> {
        let mut v = Vec::new();
        let n = self.keys.rows.len();
        for line in 0..KEYS_LINES.min(n.saturating_sub(self.keys.top)) {
            v.push((
                Target::KeyRow(self.keys.top + line),
                Rect::new(
                    KEYS_LIST.x,
                    KEYS_LIST.y + line as i32 * KEYS_LINE_H,
                    KEYS_LIST.w,
                    KEYS_LINE_H as u32,
                ),
            ));
        }
        let thumb = key_thumb(self.keys.top, n);
        let above = (thumb.y - KEYS_TRACK.y) as u32;
        let below = (KEYS_TRACK.y + KEYS_TRACK.h as i32 - thumb.y - thumb.h as i32).max(0) as u32;
        if above > 0 {
            v.push((
                Target::KeyTrack(false),
                Rect::new(KEYS_TRACK.x, KEYS_TRACK.y, KEYS_TRACK.w, above),
            ));
        }
        if below > 0 {
            v.push((
                Target::KeyTrack(true),
                Rect::new(KEYS_TRACK.x, thumb.y + thumb.h as i32, KEYS_TRACK.w, below),
            ));
        }
        v.push((Target::Action(Action::ResetKeys), KEYS_RESET));
        v
    }

    /// The Tab stops: the rows of the Keys page are one stop, its list.
    fn stops(&self) -> Vec<Target> {
        let mut v = Vec::new();
        for (t, _) in self.page_targets() {
            match t {
                Target::KeyRow(_) | Target::KeyTrack(_) => {}
                Target::Action(Action::ResetKeys) => v.extend([Target::KeyList, t]),
                t => v.push(t),
            }
        }
        v
    }

    // --- Input -----------------------------------------------------------------------------------------------

    /// One frame: the pointer, the left button and the keys.
    pub(crate) fn update(&mut self, input: &Input) -> Outcome {
        let targets = self.targets();
        let ev = self.ptr.update(&targets, input);
        self.changed |= ev.changed;
        if input.down {
            if self.open.is_some() && ev.down.is_none() {
                // A press outside the open list closes it.
                self.open = None;
                self.ptr.reset();
                self.changed = true;
            } else if let Some(t) = ev.down
                && !matches!(t, Target::PopupRow(_))
            {
                self.focus = if matches!(t, Target::KeyRow(_) | Target::KeyTrack(_)) {
                    Target::KeyList
                } else {
                    t
                };
                self.changed = true;
            }
        }
        // The wheel scrolls the Keys page's list while the pointer is over it.
        if self.page == Page::Keys
            && self.open.is_none()
            && input.wheel != 0
            && matches!(self.ptr.hot, Some(Target::KeyRow(_) | Target::KeyTrack(_)))
        {
            self.scroll_keys(-(input.wheel as isize));
        }
        let mut out = Outcome::Stay;
        if let Some(t) = ev.clicked {
            out = self.activate(t);
        }
        if out == Outcome::Stay {
            for &k in &input.keys {
                self.changed = true;
                out = if self.open.is_some() {
                    self.popup_key(k)
                } else if self.keys.capturing {
                    self.capture(k, input);
                    Outcome::Stay
                } else {
                    self.main_key(k, input.shift)
                };
                if out != Outcome::Stay {
                    break;
                }
            }
        }
        out
    }

    fn activate(&mut self, t: Target) -> Outcome {
        self.changed = true;
        // Anything but a row of the list ends the wait for a key.
        if !matches!(t, Target::KeyRow(_)) {
            self.keys.capturing = false;
        }
        match t {
            Target::Tab(p) => {
                self.page = p;
                self.focus = Target::Tab(p);
                self.keys.note = None;
            }
            Target::KeyRow(i) => {
                self.select_key(i);
                self.focus = Target::KeyList;
                self.start_capture();
            }
            Target::KeyList => self.start_capture(),
            Target::KeyTrack(down) => self.scroll_keys(if down {
                KEYS_LINES as isize
            } else {
                -(KEYS_LINES as isize)
            }),
            Target::Action(Action::ResetKeys) => {
                let lines = keymap::reset_lines(self.values.key_lines());
                self.set_keys(lines);
                self.keys.note = Some("Every key is the original's again.".to_string());
            }
            Target::Row(item) if read_only(item, &self.env) => {}
            Target::Row(item) => self.open_list(item),
            Target::Action(Action::OpenFolder) => return Outcome::OpenFolder,
            Target::Action(Action::Reimport) => return Outcome::Reimport,
            Target::Ok => return Outcome::Ok,
            Target::Cancel => return Outcome::Cancel,
            Target::Defaults => self.values.defaults(&self.env),
            Target::PopupRow(i) => self.choose(i),
        }
        Outcome::Stay
    }

    // --- The Keys page -------------------------------------------------------------------------------------

    /// The lines changed: the keys they give are worked out again.
    fn set_keys(&mut self, lines: Vec<String>) {
        self.keys.map = KeyMap::parse(&lines).0;
        self.values.set_key_lines(lines);
    }

    fn select_key(&mut self, i: usize) {
        let n = self.keys.rows.len();
        self.keys.sel = i.min(n.saturating_sub(1));
        self.keys.note = None;
        // Keep the row on the screen.
        if self.keys.sel < self.keys.top {
            self.keys.top = self.keys.sel;
        } else if self.keys.sel >= self.keys.top + KEYS_LINES {
            self.keys.top = self.keys.sel + 1 - KEYS_LINES;
        }
    }

    fn scroll_keys(&mut self, by: isize) {
        let max = self.keys.rows.len().saturating_sub(KEYS_LINES) as isize;
        let top = (self.keys.top as isize + by).clamp(0, max) as usize;
        if top != self.keys.top {
            self.keys.top = top;
            self.changed = true;
        }
    }

    fn start_capture(&mut self) {
        self.keys.capturing = true;
        self.keys.note = None;
    }

    /// The key a row has now and whether it is the player's own.
    fn key_text(&self, i: usize) -> (String, bool) {
        let row = &self.keys.rows[i];
        match self.keys.map.chord_of(&row.name) {
            Some(c) => (c.text(), true),
            None => (row.default.clone(), false),
        }
    }

    /// A key pressed while the page waits for one: Esc gives up, a modifier alone is waited out, any other key is bound to
    /// the selected row if the rules allow it, and the description says what happened.
    fn capture(&mut self, k: Key, input: &Input) {
        match k {
            Key::Escape => {
                self.keys.capturing = false;
                self.keys.note = None;
            }
            Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
            | Key::CapsLock => {}
            _ => {
                self.keys.capturing = false;
                let mods = KeyMods {
                    shift: input.shift,
                    ctrl: input.ctrl,
                    alt: input.alt,
                };
                let row = self.keys.rows[self.keys.sel].clone();
                let Some(chord) = Chord::pressed(k, mods) else {
                    self.keys.note = Some(
                        "That key cannot be used. Use a letter, a digit, F1 to F12, minus or equal, with Ctrl, Alt or \
                         Shift where the action needs one."
                            .to_string(),
                    );
                    return;
                };
                self.keys.note = Some(match keymap::with_binding(self.values.key_lines(), &row.name, chord) {
                    Ok(lines) => {
                        self.set_keys(lines);
                        format!("{} is now {}.", row.label, chord.text())
                    }
                    Err(why) => why,
                });
            }
        }
    }

    /// The selected row's own key back (Backspace or Delete on the list).
    fn restore_key(&mut self) {
        let row = self.keys.rows[self.keys.sel].clone();
        let lines = keymap::without_binding(self.values.key_lines(), &row.name);
        self.set_keys(lines);
        self.keys.note = Some(format!("{} has its original key again: {}.", row.label, row.default));
    }

    fn open_list(&mut self, item: Item) {
        let current = self.values.get(item);
        let opts = offered(item, current, &self.env);
        let at = opts.iter().position(|(n, _)| *n == current).unwrap_or(0);
        let anchor = self
            .page_targets()
            .into_iter()
            .find(|(t, _)| *t == Target::Row(item))
            .map_or_else(Rect::default, |(_, r)| drop_rect(r));
        let labels = opts.iter().map(|(_, w)| w.clone()).collect();
        self.open = Some(Open {
            item,
            popup: Popup::new(0, anchor, labels, at),
            values: opts.iter().map(|(n, _)| *n).collect(),
        });
    }

    /// Picks line `i` of the open list.
    fn choose(&mut self, i: usize) {
        let Some(o) = self.open.take() else { return };
        if let Some(&n) = o.values.get(i) {
            self.values.set(o.item, n);
        }
    }

    fn popup_key(&mut self, k: Key) -> Outcome {
        let Some(o) = &mut self.open else { return Outcome::Stay };
        match o.popup.key(k) {
            PopupKey::Open => {}
            PopupKey::Closed => self.open = None,
            PopupKey::Picked(i) => self.choose(i),
        }
        Outcome::Stay
    }

    fn step(&mut self, back: bool) {
        let key = if back {
            menuscreen::FocusKey::Back
        } else {
            menuscreen::FocusKey::Next
        };
        self.focus = menuscreen::step_focus(&self.stops(), self.focus, key);
    }

    /// The value of the focused drop-down moved by `by` places, wrapping.
    fn cycle(&mut self, item: Item, by: isize) {
        if read_only(item, &self.env) {
            return;
        }
        let current = self.values.get(item);
        let opts = offered(item, current, &self.env);
        let at = opts.iter().position(|(n, _)| *n == current).unwrap_or(0);
        let next = (at as isize + by).rem_euclid(opts.len() as isize) as usize;
        self.values.set(item, opts[next].0);
    }

    fn switch_page(&mut self, by: isize) {
        let pages = pages(&self.env);
        let at = pages.iter().position(|&p| p == self.page).unwrap_or(0) as isize;
        let p = pages[(at + by).rem_euclid(pages.len() as isize) as usize];
        self.page = p;
        self.keys.capturing = false;
        self.keys.note = None;
        if matches!(self.focus, Target::Tab(_)) {
            self.focus = Target::Tab(p);
        }
    }

    fn key_move(&mut self, to: usize) -> Outcome {
        self.select_key(to);
        Outcome::Stay
    }

    fn main_key(&mut self, k: Key, shift: bool) -> Outcome {
        if self.focus == Target::KeyList {
            // On the Keys page's list the arrows move through the rows; Tab leaves it.
            let last = self.keys.rows.len().saturating_sub(1);
            match k {
                Key::Down => return self.key_move(self.keys.sel.saturating_add(1).min(last)),
                Key::Up => return self.key_move(self.keys.sel.saturating_sub(1)),
                Key::Home => return self.key_move(0),
                Key::End => return self.key_move(last),
                Key::Backspace => {
                    self.restore_key();
                    return Outcome::Stay;
                }
                _ => {}
            }
        }
        match k {
            Key::Tab => self.step(shift),
            Key::Down => self.step(false),
            Key::Up => self.step(true),
            Key::Escape => return Outcome::Cancel,
            Key::Enter | Key::NumpadEnter | Key::Space => return self.activate(self.focus),
            Key::PageDown => self.switch_page(1),
            Key::PageUp => self.switch_page(-1),
            Key::Left | Key::Right => {
                let by = if k == Key::Left { -1 } else { 1 };
                match self.focus {
                    Target::Row(item) => self.cycle(item, by),
                    Target::Tab(_) => {
                        self.switch_page(by);
                        self.focus = Target::Tab(self.page);
                    }
                    Target::Ok | Target::Cancel | Target::Defaults => {
                        let row = [Target::Defaults, Target::Cancel, Target::Ok];
                        let at = row.iter().position(|&t| t == self.focus).unwrap_or(0) as isize;
                        self.focus = row[(at + by).clamp(0, 2) as usize];
                    }
                    _ => {}
                }
            }
            Key::Letter(c) => {
                let t = match c.to_ascii_uppercase() {
                    'O' => Target::Ok,
                    'C' => Target::Cancel,
                    'D' => Target::Defaults,
                    _ => return Outcome::Stay,
                };
                self.focus = t;
                return self.activate(t);
            }
            _ => {}
        }
        Outcome::Stay
    }

    // --- Drawing ---------------------------------------------------------------------------------------------

    /// The picture with the panel and the heading: what fades in with the screen.
    pub(crate) fn frame(bg: &Picture, look: &Look) -> Picture {
        let mut p = Picture {
            width: bg.width,
            height: bg.height,
            rgba: bg.rgba.clone(),
        };
        mpwidgets::draw_panel(look, &mut p, PANEL);
        menuscreen::draw_heading(&mut p, look.font, look.pal, HEADING, "Tidewater Settings");
        p
    }

    /// The description to show: the hovered row's, else the focused one's.
    fn described(&self) -> String {
        if self.page == Page::Keys {
            if self.keys.capturing {
                return "Press a key... (Esc cancels)".to_string();
            }
            if let Some(n) = &self.keys.note {
                return n.clone();
            }
        }
        let t = match self.ptr.hot {
            Some(Target::PopupRow(_)) | None => self.open.as_ref().map(|o| Target::Row(o.item)).unwrap_or(self.focus),
            Some(t) => t,
        };
        match t {
            Target::Tab(p) => p.blurb().to_string(),
            Target::Row(item) => describe_now(item, self.values.get(item), &self.env),
            Target::Action(a) => describe_action(a).to_string(),
            Target::Ok => "Apply the changes and save them.".to_string(),
            Target::Cancel if self.env.in_game => "Throw the changes away and go back to the game.".to_string(),
            Target::Cancel => "Throw the changes away and go back to the main menu.".to_string(),
            Target::Defaults => {
                "Set every setting to its default, apart from the keys (Reset keys does those). Cancel \
                 still throws the change away."
                    .to_string()
            }
            Target::KeyRow(i) => self.key_description(i),
            Target::KeyList => self.key_description(self.keys.sel),
            Target::KeyTrack(_) | Target::PopupRow(_) => String::new(),
        }
    }

    /// What the description area says about the action in row `i` of the Keys page.
    fn key_description(&self, i: usize) -> String {
        let Some(row) = self.keys.rows.get(i) else {
            return String::new();
        };
        let (now, own) = self.key_text(i);
        let now = if own { now } else { format!("{now} (the original's)") };
        format!(
            "{}: {now}. In the original: {}. Enter or a click changes it; Backspace puts the original key back.",
            row.label, row.original
        )
    }

    /// The whole screen over `bg`, the menu picture.
    pub(crate) fn draw(&self, bg: &Picture, look: &Look, small: &Font) -> Picture {
        let mut p = Self::frame(bg, look);
        let modal = self.open.is_some();
        let state = |t: Target, enabled: bool| ButtonState {
            enabled,
            hover: !modal && self.ptr.is_hot(t),
            focused: !modal && self.focus == t,
            pressed: !modal && self.ptr.pressed(t),
            default: false,
        };
        let shown_pages = pages(&self.env);
        for &page in &shown_pages {
            let t = Target::Tab(page);
            let mut s = state(t, true);
            // The page that shows is drawn pressed, the way a tab is down.
            s.pressed |= page == self.page;
            menuscreen::draw_button(
                &mut p,
                look.font,
                look.pal,
                Some(look.faces),
                tab_rect(page, &shown_pages),
                page.label(),
                None,
                s,
            );
        }
        let lay = layout(self.page, &self.env);
        for &(t, r) in &lay.targets {
            match t {
                Target::Row(item) => {
                    let label = match item {
                        Item::Extra(id) => id.spec().label,
                        Item::Window => "Window",
                        Item::MouseLock => "Mouse lock",
                        Item::IntroVideo => "Intro video",
                        Item::UpdateCheck => "Check for updates",
                        Item::ModSlot(k) => MOD_LABELS[usize::from(k).min(MOD_LABELS.len() - 1)],
                    };
                    let b = drop_rect(r);
                    let at = (LABEL_X, b.y + (DROP_H as i32 - look.font.height as i32) / 2);
                    if read_only(item, &self.env) {
                        look.text(&mut p, at, label, menuscreen::GREY);
                    } else {
                        look.label(&mut p, at, label);
                    }
                    let cur = self.values.get(item);
                    let word = (if matches!(item, Item::ModSlot(_)) {
                        mod_options(&self.env)
                    } else {
                        options(item, cur)
                    })
                    .into_iter()
                    .find(|(n, _)| *n == cur)
                    .map(|(_, w)| w)
                    .unwrap_or_default();
                    let mut ds = DropState::of(
                        !modal && self.ptr.is_hot(t),
                        !modal && self.focus == t,
                        !modal && self.ptr.pressed(t),
                    );
                    if read_only(item, &self.env) {
                        ds = ds.dim();
                    }
                    look.drop(&mut p, b, &word, ds);
                }
                Target::Action(a) => menuscreen::draw_button(
                    &mut p,
                    look.font,
                    look.pal,
                    Some(look.faces),
                    r,
                    a.label(),
                    None,
                    state(t, true),
                ),
                _ => {}
            }
        }
        if self.page == Page::Keys {
            self.draw_keys(&mut p, look, modal);
            for (t, r) in self.key_targets() {
                if let Target::Action(a) = t {
                    menuscreen::draw_button(
                        &mut p,
                        look.font,
                        look.pal,
                        Some(look.faces),
                        r,
                        a.label(),
                        None,
                        state(t, true),
                    );
                }
            }
        }
        for (text, r) in &lay.texts {
            self.draw_lines(&mut p, look, small, text, *r);
        }
        self.draw_lines(&mut p, look, small, &self.described(), DESC);
        for (t, r, label, key) in [
            (Target::Defaults, DEFAULTS, "Defaults", 'D'),
            (Target::Cancel, CANCEL, "Cancel", 'C'),
            (Target::Ok, OK, "OK", 'O'),
        ] {
            menuscreen::draw_button(
                &mut p,
                look.font,
                look.pal,
                Some(look.faces),
                r,
                label,
                Some(key),
                state(t, true),
            );
        }
        if let Some(o) = &self.open {
            let hot = match self.ptr.hot {
                Some(Target::PopupRow(i)) => Some(i),
                _ => None,
            };
            mpwidgets::draw_popup(look, &mut p, &o.popup, hot);
        }
        p
    }

    /// The Keys page's list: a line per action with its key (the player's own in gold, the original's in grey), the
    /// selected line filled, the scroll bar, in the Select scenario dialog's look.
    fn draw_keys(&self, p: &mut Picture, look: &Look, modal: bool) {
        let rgb = |e: u8| look.pal.0[e as usize];
        hud::fill(p, KEYS_LIST, rgb(menuscreen::BLACK));
        let focused = !modal && self.focus == Target::KeyList;
        hud::outline(
            p,
            KEYS_LIST,
            rgb(if focused {
                menuscreen::FOCUS_RING
            } else {
                *menuscreen::OUTER_RING.start()
            }),
        );
        let n = self.keys.rows.len();
        for line in 0..KEYS_LINES.min(n.saturating_sub(self.keys.top)) {
            let i = self.keys.top + line;
            let r = Rect::new(
                KEYS_LIST.x + 1,
                KEYS_LIST.y + 1 + line as i32 * KEYS_LINE_H - 1,
                KEYS_LIST.w - 2,
                KEYS_LINE_H as u32,
            );
            let on = i == self.keys.sel;
            let hot = !modal && self.ptr.is_hot(Target::KeyRow(i));
            if on {
                hud::fill(p, r, rgb(162));
            }
            let y = r.y + (KEYS_LINE_H - look.font.height as i32) / 2;
            let name_map = if on || hot { menuscreen::WHITE } else { menuscreen::GOLD };
            look.text(
                p,
                (KEYS_NAME_X, y),
                &crate::customsetup::fit(
                    look.font,
                    &self.keys.rows[i].label,
                    (KEYS_KEY_X - KEYS_NAME_X - 8) as u32,
                ),
                name_map,
            );
            let (text, own) = self.key_text(i);
            let (text, map) = if on && self.keys.capturing {
                ("Press a key...".to_string(), menuscreen::WHITE)
            } else if own {
                (text, if on || hot { menuscreen::WHITE } else { menuscreen::GOLD })
            } else {
                (text, menuscreen::GREY)
            };
            look.text(
                p,
                (KEYS_KEY_X, y),
                &crate::customsetup::fit(
                    look.font,
                    &text,
                    (KEYS_LIST.x + KEYS_LIST.w as i32 - KEYS_KEY_X - 8) as u32,
                ),
                map,
            );
        }
        hud::fill(p, KEYS_TRACK, rgb(menuscreen::BLACK));
        hud::outline(p, KEYS_TRACK, rgb(*menuscreen::OUTER_RING.start()));
        let thumb = key_thumb(self.keys.top, n);
        hud::fill(
            p,
            Rect::new(thumb.x + 2, thumb.y + 2, thumb.w - 4, thumb.h.saturating_sub(4)),
            rgb(162),
        );
    }

    /// `text` wrapped in `r` in the small font, gold; what does not fit in the rectangle's height is cut with "...".
    fn draw_lines(&self, p: &mut Picture, look: &Look, small: &Font, text: &str, r: Rect) {
        let mut lines = wrap(small, text, r.w);
        let room = (r.h as i32 / TEXT_LINE).max(1) as usize;
        if lines.len() > room {
            lines.truncate(room);
            if let Some(last) = lines.last_mut() {
                *last = crate::customsetup::fit(small, &format!("{last}..."), r.w);
                if !last.ends_with("...") {
                    last.push_str("...");
                }
            }
        }
        for (i, l) in lines.iter().enumerate() {
            menuscreen::draw_text(
                p,
                small,
                look.pal,
                (r.x, r.y + i as i32 * TEXT_LINE),
                l,
                menuscreen::GOLD,
                menuscreen::SPACE_10X,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use tidewater_formats::tileset::Palette;

    use super::*;

    fn env(touch: bool) -> Env {
        Env {
            touch,
            touch_input: false,
            pad: false,
            devices: crate::controls::Devices {
                pad: false,
                touch,
                mouse: !touch,
            },
            updates: !touch,
            data: (!touch).then(|| "Game files: GOG installer, 156 files, 41 maps, imported 2026-10-23".to_string()),
            mods: Vec::new(),
            in_game: false,
        }
    }

    fn mod_info(folder: &str, title: &str) -> ModInfo {
        ModInfo {
            folder: folder.into(),
            title: title.into(),
            version: "1.0".into(),
            description: "Harder hits.".into(),
            authors: vec!["Ann".into()],
        }
    }

    fn env_with_mods() -> Env {
        Env {
            mods: vec![
                mod_info("axes", "Big Axes"),
                mod_info("sounds", ""),
                mod_info("zed", ""),
            ],
            ..env(false)
        }
    }

    fn mods_screen(enabled: &[&str]) -> Screen {
        let env = env_with_mods();
        let mut v = Values::new(&Prefs::default(), &ClientExtras::default(), false, false);
        v.show_mods(&enabled.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &env.mods);
        Screen::new(env, v)
    }

    #[test]
    fn the_mods_page_shows_only_when_there_is_a_mod_to_enable() {
        assert!(!pages(&env(false)).contains(&Page::Mods));
        assert!(!pages(&env(true)).contains(&Page::Mods));
        assert!(pages(&env_with_mods()).contains(&Page::Mods));
        assert!(layout(Page::Mods, &env(false)).targets.is_empty());
        let rows = layout(Page::Mods, &env_with_mods());
        assert_eq!(rows.targets.len(), usize::from(MOD_SLOTS));
        assert_eq!(rows.texts[0].0, MODS_NOTE);
    }

    #[test]
    fn the_mods_rows_show_the_enabled_mods_in_order_and_offer_every_mod() {
        let s = mods_screen(&["zed", "AXES", "gone"]);
        assert_eq!(s.values.get(Item::ModSlot(0)), 3);
        assert_eq!(s.values.get(Item::ModSlot(1)), 1);
        assert_eq!(
            s.values.get(Item::ModSlot(2)),
            0,
            "a mod that is not there is not shown"
        );
        let words: Vec<String> = offered(Item::ModSlot(0), 0, &s.env)
            .into_iter()
            .map(|(_, w)| w)
            .collect();
        assert_eq!(words, ["None", "Big Axes 1.0", "sounds 1.0", "zed 1.0"]);
        let d = describe_now(Item::ModSlot(1), 1, &s.env);
        assert!(d.starts_with("Big Axes: Harder hits. By Ann."), "{d}");
        assert!(
            d.contains("next time Tidewater starts") || d.contains("next time"),
            "{d}"
        );
    }

    #[test]
    fn choosing_mods_and_ok_store_the_list_in_order_once_each() {
        let mut s = mods_screen(&[]);
        click(&mut s, Target::Tab(Page::Mods));
        assert_eq!(s.page(), Page::Mods);
        pick(&mut s, Item::ModSlot(0), 2);
        pick(&mut s, Item::ModSlot(1), 1);
        pick(&mut s, Item::ModSlot(2), 2);
        let (mut stored, mut live, mut audio) = (Prefs::default(), ClientExtras::default(), Audio::default());
        commit(s.values(), &mut stored, &mut live, &mut audio);
        assert_eq!(
            stored.mods,
            ["sounds", "axes"],
            "a mod chosen twice counts once, at its first place"
        );
        assert!(Prefs::from_text(&stored.to_text()).mods == stored.mods);
        // Rows left alone leave the stored list alone; Defaults turns every mod off.
        let mut other = mods_screen(&["axes"]);
        let mut again = Prefs::default();
        again.mods = vec!["axes".into()];
        commit(other.values(), &mut again, &mut live, &mut audio);
        assert_eq!(again.mods, ["axes"]);
        click(&mut other, Target::Defaults);
        commit(other.values(), &mut again, &mut live, &mut audio);
        assert!(again.mods.is_empty());
    }

    fn screen_with(env: Env) -> Screen {
        let v = Values::new(&Prefs::default(), &ClientExtras::default(), false, false);
        Screen::new(env, v)
    }

    fn screen() -> Screen {
        screen_with(env(false))
    }

    fn click_at(s: &mut Screen, x: i32, y: i32) -> Outcome {
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

    fn click(s: &mut Screen, t: Target) -> Outcome {
        let r = s
            .targets()
            .into_iter()
            .find(|(x, _)| *x == t)
            .map(|(_, r)| r)
            .unwrap_or_else(|| panic!("{t:?} is not there"));
        let (x, y) = if let Target::Row(_) = t {
            let d = drop_rect(r);
            (d.x + 10, d.y + 10)
        } else {
            (r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
        };
        click_at(s, x, y)
    }

    fn key(s: &mut Screen, k: Key) -> Outcome {
        s.update(&Input {
            keys: vec![k],
            ..Default::default()
        })
    }

    fn pick(s: &mut Screen, item: Item, line: usize) {
        click(s, Target::Row(item));
        assert!(s.open.is_some(), "the list of {item:?} opened");
        let r = s.open.as_ref().unwrap().popup.row_rect(line);
        click_at(s, r.x + 5, r.y + 5);
        assert!(s.open.is_none());
    }

    #[test]
    fn every_option_is_on_exactly_one_page() {
        for s in &REGISTRY {
            let pages: Vec<Page> = Page::ALL
                .into_iter()
                .filter(|&p| items_on(p).contains(&Item::Extra(s.id)))
                .collect();
            assert_eq!(pages, [extra_page(s.id)], "{} is on exactly one page", s.name);
        }
        let rows: usize = Page::ALL
            .iter()
            .map(|&p| items_on(p).iter().filter(|i| matches!(i, Item::Extra(_))).count())
            .sum();
        assert_eq!(rows, REGISTRY.len());
    }

    #[test]
    fn the_pages_hold_what_the_brief_lists() {
        let names = |p: Page| -> Vec<&'static str> {
            items_on(p)
                .into_iter()
                .filter_map(|i| match i {
                    Item::Extra(id) => Some(id.spec().name),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(names(Page::General), ["scaling", "widescreen", "controls"]);
        assert_eq!(
            names(Page::Interface),
            [
                "health-bars",
                "idle-worker",
                "game-clock",
                "group-bar",
                "smart-attack-move",
                "minimap-ping",
                "auto-replay",
                "map-zoom"
            ]
        );
        assert_eq!(names(Page::Sound), ["volume-voices", "volume-effects", "volume-music"]);
        assert_eq!(
            names(Page::Gameplay),
            [
                "rally-points",
                "production-queue",
                "selection-limit",
                "speed-levels",
                "shift-queue"
            ]
        );
        assert_eq!(items_on(Page::General).first(), Some(&Item::Window));
    }

    fn in_game_env() -> Env {
        Env {
            in_game: true,
            ..env(false)
        }
    }

    /// A screen opened over a game started with rally points, a selection limit of 12 and two extra speeds.
    fn in_game_screen() -> Screen {
        let mut v = Values::new(&Prefs::default(), &ClientExtras::default(), false, false);
        v.show_game_rules(&tidewater_sim::Extras {
            rally_points: true,
            selection_limit: 12,
            extra_speeds: 2,
            ..Default::default()
        });
        Screen::new(in_game_env(), v)
    }

    #[test]
    fn in_a_game_the_gameplay_page_shows_the_games_rules_and_cannot_change_them() {
        let mut s = in_game_screen();
        click(&mut s, Target::Tab(Page::Gameplay));
        let v = &s.values;
        assert_eq!(v.get(Item::Extra(Id::RallyPoints)), 1);
        assert_eq!(v.get(Item::Extra(Id::SelectionLimit)), 1);
        assert_eq!(v.get(Item::Extra(Id::SpeedLevels)), 2);
        assert_eq!(v.get(Item::Extra(Id::ProductionQueue)), 0);
        // Clicking, Enter and the arrows do nothing to them.
        click(&mut s, Target::Row(Item::Extra(Id::RallyPoints)));
        assert!(s.open.is_none(), "no list opens");
        s.focus = Target::Row(Item::Extra(Id::SpeedLevels));
        key(&mut s, Key::Enter);
        key(&mut s, Key::Right);
        key(&mut s, Key::Left);
        assert!(s.open.is_none());
        assert_eq!(s.values.get(Item::Extra(Id::SpeedLevels)), 2);
        // Defaults leaves them (and every row of the other pages goes to its default), and OK writes none of them.
        click(&mut s, Target::Defaults);
        assert_eq!(s.values.get(Item::Extra(Id::RallyPoints)), 1);
        let (mut stored, mut live, mut audio) = (Prefs::default(), ClientExtras::default(), Audio::default());
        commit(s.values(), &mut stored, &mut live, &mut audio);
        assert!(stored.extras.sim().is_default() && live.sim().is_default());
        // The note says why, and the other pages stay editable.
        let note = layout(Page::Gameplay, &in_game_env()).texts;
        assert_eq!(note[0].0, GAMEPLAY_NOTE_IN_GAME);
        assert!(GAMEPLAY_NOTE_IN_GAME.starts_with("Fixed for this game"));
        let mut s = in_game_screen();
        s.page = Page::Interface;
        pick(&mut s, Item::Extra(Id::HealthBars), 2);
        assert_eq!(s.values.get(Item::Extra(Id::HealthBars)), 2);
        assert!(!read_only(Item::Extra(Id::HealthBars), &in_game_env()));
        assert!(read_only(Item::Extra(Id::ShiftQueue), &in_game_env()));
        assert!(!read_only(Item::Extra(Id::ShiftQueue), &env(false)));
    }

    #[test]
    fn in_a_game_import_again_is_hidden_and_everything_else_stays() {
        let actions = |e: &Env| -> Vec<Action> {
            layout(Page::General, e)
                .targets
                .into_iter()
                .filter_map(|(t, _)| if let Target::Action(a) = t { Some(a) } else { None })
                .collect()
        };
        assert_eq!(actions(&in_game_env()), [Action::OpenFolder]);
        assert_eq!(
            layout(Page::General, &in_game_env()).texts.len(),
            1,
            "the game files line stays"
        );
        let rows = |e: &Env| layout(Page::General, e).targets.len();
        assert_eq!(rows(&in_game_env()) + 1, rows(&env(false)));
        // OK, Cancel and Esc all leave; the Cancel text says where.
        let mut s = in_game_screen();
        assert_eq!(click(&mut s, Target::Ok), Outcome::Ok);
        assert_eq!(click(&mut s, Target::Cancel), Outcome::Cancel);
        assert_eq!(key(&mut s, Key::Escape), Outcome::Cancel);
        s.focus = Target::Cancel;
        assert!(s.described().contains("game"));
    }

    #[test]
    fn every_option_has_a_label_and_every_choice_has_words() {
        for s in &REGISTRY {
            assert!(!s.label.is_empty() && s.label.len() <= 24, "{}", s.name);
            let v = options(Item::Extra(s.id), s.default);
            match s.kind {
                Kind::Flag => assert_eq!(v.len(), 2),
                Kind::Choice(names) => {
                    assert_eq!(v.len(), names.len(), "{}", s.name);
                    let words = CHOICE_WORDS
                        .iter()
                        .find(|(i, _)| *i == s.id)
                        .expect("words for the choice")
                        .1;
                    assert_eq!(words.len(), names.len(), "{}", s.name);
                    // Human words, not the settings file's tokens.
                    for (w, n) in words.iter().zip(names) {
                        assert!(
                            w.chars().next().is_some_and(char::is_uppercase)
                                || w.chars().next().is_some_and(|c| c.is_ascii_digit()),
                            "{w} for {n}"
                        );
                    }
                }
                Kind::Percent => assert_eq!(v.len(), 11),
            }
        }
        let words = |id: Id| {
            options(Item::Extra(id), 0)
                .into_iter()
                .map(|(_, w)| w)
                .collect::<Vec<_>>()
        };
        assert_eq!(words(Id::HealthBars), ["Off", "Hovered", "Selected", "Always"]);
        assert_eq!(words(Id::Widescreen), ["Off", "16:10", "16:9", "Auto"]);
        assert_eq!(words(Id::SelectionLimit)[3], "Unlimited");
        assert_eq!(words(Id::IdleWorker), ["Off", "On"]);
    }

    #[test]
    fn a_volume_that_is_not_a_multiple_of_ten_is_shown_and_kept() {
        let o = options(Item::Extra(Id::VolumeMusic), 37);
        assert_eq!(o.len(), 12);
        assert!(o.iter().any(|(n, w)| *n == 37 && w == "37%"));
        assert!(o.windows(2).all(|w| w[0].0 < w[1].0), "in order");
        assert_eq!(options(Item::Extra(Id::VolumeMusic), 40).len(), 11);
        // Opening the list and closing it again keeps the odd value.
        let mut live = ClientExtras::default();
        live.set(Id::VolumeMusic, 37);
        let mut s = Screen::new(env(false), Values::new(&Prefs::default(), &live, false, false));
        s.page = Page::Sound;
        click(&mut s, Target::Row(Item::Extra(Id::VolumeMusic)));
        key(&mut s, Key::Escape);
        assert_eq!(s.values.get(Item::Extra(Id::VolumeMusic)), 37);
        assert!(!s.values.touched[index(Item::Extra(Id::VolumeMusic))]);
    }

    #[test]
    fn ok_applies_the_changed_rows_and_the_file_text_holds_them() {
        let mut s = screen();
        pick(&mut s, Item::Extra(Id::Scaling), 1);
        pick(&mut s, Item::Window, 1);
        pick(&mut s, Item::MouseLock, 0);
        pick(&mut s, Item::IntroVideo, 0);
        assert_eq!(click(&mut s, Target::Tab(Page::Sound)), Outcome::Stay);
        pick(&mut s, Item::Extra(Id::VolumeMusic), 3);
        assert_eq!(click(&mut s, Target::Ok), Outcome::Ok);
        let mut stored = Prefs::default();
        let mut live = ClientExtras::default();
        let mut audio = Audio::default();
        commit(s.values(), &mut stored, &mut live, &mut audio);
        assert_eq!(stored.extras.get(Id::Scaling), 1);
        assert_eq!(live.get(Id::Scaling), 1);
        assert!(stored.fullscreen && !stored.mouse_lock && !stored.has(bit::OPENING_FILM));
        assert_eq!(stored.extras.get(Id::VolumeMusic), 30);
        let back = Prefs::from_text(&stored.to_text());
        assert_eq!(back.extras.get(Id::VolumeMusic), 30);
        assert!(back.fullscreen && !back.mouse_lock && !back.has(bit::OPENING_FILM));
    }

    #[test]
    fn a_command_line_override_survives_ok_unless_the_row_was_chosen() {
        let stored_file = Prefs::default();
        let mut live = ClientExtras::default();
        live.set(Id::HealthBars, 3); // `--extra-health-bars always` for this run
        let mut s = Screen::new(env(false), Values::new(&stored_file, &live, false, false));
        assert_eq!(
            s.values.get(Item::Extra(Id::HealthBars)),
            3,
            "the screen shows what is in force"
        );
        pick(&mut s, Item::Window, 1);
        let (mut stored, mut l, mut audio) = (stored_file.clone(), live, Audio::default());
        commit(s.values(), &mut stored, &mut l, &mut audio);
        assert_eq!(l.get(Id::HealthBars), 3, "the override stays for the run");
        assert_eq!(stored.extras.get(Id::HealthBars), 0, "and is not written to the file");
        // Choosing the row replaces the override.
        s.page = Page::Interface;
        pick(&mut s, Item::Extra(Id::HealthBars), 1);
        commit(s.values(), &mut stored, &mut l, &mut audio);
        assert_eq!((l.get(Id::HealthBars), stored.extras.get(Id::HealthBars)), (1, 1));
    }

    #[test]
    fn cancel_and_esc_leave_without_a_change_to_apply() {
        let mut s = screen();
        pick(&mut s, Item::Window, 1);
        assert_eq!(click(&mut s, Target::Cancel), Outcome::Cancel);
        assert_eq!(key(&mut s, Key::Escape), Outcome::Cancel);
        // The settings were never touched: only `commit` writes them, and the frontend calls it for OK alone.
        let stored = Prefs::default();
        assert!(!stored.fullscreen);
    }

    #[test]
    fn defaults_resets_every_shown_row_on_every_page() {
        let mut live = ClientExtras::default();
        live.set(Id::HealthBars, 2);
        live.set(Id::RallyPoints, 1);
        live.set(Id::VolumeVoices, 20);
        let mut prefs = Prefs::default();
        prefs.fullscreen = true;
        prefs.mouse_lock = false;
        prefs.update_check = false;
        let mut s = Screen::new(env(false), Values::new(&prefs, &live, true, false));
        assert_eq!(click(&mut s, Target::Defaults), Outcome::Stay);
        for i in 0..ITEMS {
            let it = item_at(i);
            assert_eq!(s.values.get(it), default_of(it), "{it:?}");
        }
        let (mut stored, mut l, mut audio) = (prefs.clone(), live, Audio::default());
        commit(s.values(), &mut stored, &mut l, &mut audio);
        assert!(!stored.fullscreen && stored.mouse_lock && stored.update_check && stored.has(bit::OPENING_FILM));
        assert!(l.is_default());
        // On a phone the rows it cannot show are left as they are.
        let mut m = Screen::new(env(true), Values::new(&prefs, &ClientExtras::default(), false, false));
        key(&mut m, Key::Letter('D'));
        assert!(
            m.values.get(Item::Window) == 1,
            "the window row is not on a phone, so Defaults leaves it"
        );
    }

    #[test]
    fn focus_moves_with_tab_and_the_arrows_and_wraps() {
        let mut s = screen();
        assert_eq!(s.focus, Target::Tab(Page::General));
        key(&mut s, Key::Tab);
        assert_eq!(s.focus, Target::Tab(Page::Interface));
        s.update(&Input {
            keys: vec![Key::Tab],
            shift: true,
            ..Default::default()
        });
        assert_eq!(s.focus, Target::Tab(Page::General));
        // Shift+Tab from the first stop wraps to the last, Tab from the last to the first.
        s.update(&Input {
            keys: vec![Key::Tab],
            shift: true,
            ..Default::default()
        });
        assert_eq!(s.focus, Target::Defaults);
        key(&mut s, Key::Tab);
        assert_eq!(s.focus, Target::Tab(Page::General));
        // Down walks through the tabs, then the rows in order.
        for _ in 0..5 {
            key(&mut s, Key::Down);
        }
        assert_eq!(s.focus, Target::Row(Item::Window));
        key(&mut s, Key::Down);
        assert_eq!(s.focus, Target::Row(Item::Extra(Id::Scaling)));
        key(&mut s, Key::Up);
        assert_eq!(s.focus, Target::Row(Item::Window));
        // The buttons are reached after the rows.
        let stops = s.stops();
        assert_eq!(
            &stops[stops.len() - 3..],
            [Target::Ok, Target::Cancel, Target::Defaults]
        );
    }

    #[test]
    fn the_keyboard_opens_a_list_picks_and_cycles() {
        let mut s = screen();
        s.focus = Target::Row(Item::Window);
        key(&mut s, Key::Enter);
        assert!(s.open.is_some());
        key(&mut s, Key::Down);
        assert_eq!(key(&mut s, Key::Enter), Outcome::Stay);
        assert!(s.open.is_none());
        assert_eq!(s.values.get(Item::Window), 1);
        // Esc closes a list without leaving the screen.
        key(&mut s, Key::Enter);
        assert_eq!(key(&mut s, Key::Escape), Outcome::Stay);
        assert!(s.open.is_none());
        // Left and Right change the value and wrap.
        key(&mut s, Key::Right);
        assert_eq!(s.values.get(Item::Window), 0);
        key(&mut s, Key::Left);
        assert_eq!(s.values.get(Item::Window), 1);
        // Enter on a button presses it.
        s.focus = Target::Ok;
        assert_eq!(key(&mut s, Key::Enter), Outcome::Ok);
        s.focus = Target::Action(Action::OpenFolder);
        assert_eq!(key(&mut s, Key::Enter), Outcome::OpenFolder);
        s.focus = Target::Action(Action::Reimport);
        assert_eq!(key(&mut s, Key::Enter), Outcome::Reimport);
        assert_eq!(key(&mut s, Key::Letter('c')), Outcome::Cancel);
    }

    #[test]
    fn a_click_on_a_tab_shows_its_page_and_a_press_outside_a_list_closes_it() {
        let mut s = screen();
        click(&mut s, Target::Tab(Page::Gameplay));
        assert_eq!(s.page(), Page::Gameplay);
        assert_eq!(s.focus, Target::Tab(Page::Gameplay));
        click(&mut s, Target::Row(Item::Extra(Id::SelectionLimit)));
        assert!(s.open.is_some());
        s.update(&Input {
            at: Some((10, 10)),
            down: true,
            ..Default::default()
        });
        assert!(s.open.is_none());
        key(&mut s, Key::PageDown);
        assert_eq!(s.page(), Page::Keys);
        key(&mut s, Key::PageDown);
        assert_eq!(s.page(), Page::General);
        key(&mut s, Key::PageUp);
        assert_eq!(s.page(), Page::Keys);
        key(&mut s, Key::PageUp);
        assert_eq!(s.page(), Page::Gameplay);
    }

    #[test]
    fn a_release_off_the_button_chooses_nothing() {
        let mut s = screen();
        let r = OK;
        s.update(&Input {
            at: Some((r.x + 5, r.y + 5)),
            down: true,
            ..Default::default()
        });
        let out = s.update(&Input {
            at: Some((5, 5)),
            up: true,
            ..Default::default()
        });
        assert_eq!(out, Outcome::Stay);
    }

    #[test]
    fn the_platform_decides_which_rows_show() {
        let rows = |e: &Env, p: Page| -> Vec<Item> {
            layout(p, e)
                .targets
                .into_iter()
                .filter_map(|(t, _)| if let Target::Row(i) = t { Some(i) } else { None })
                .collect()
        };
        let desktop = env(false);
        let general = rows(&desktop, Page::General);
        assert_eq!(
            general,
            [
                Item::Window,
                Item::Extra(Id::Scaling),
                Item::Extra(Id::Widescreen),
                Item::Extra(Id::Controls),
                Item::MouseLock,
                Item::IntroVideo,
                Item::UpdateCheck
            ]
        );
        let phone = env(true);
        assert_eq!(
            rows(&phone, Page::General),
            [
                Item::Extra(Id::Scaling),
                Item::Extra(Id::Widescreen),
                Item::Extra(Id::Controls),
                Item::IntroVideo
            ]
        );
        // A development build never checks for updates.
        let dev = Env {
            updates: false,
            ..env(false)
        };
        assert!(!rows(&dev, Page::General).contains(&Item::UpdateCheck));
        // The data management: the file line and the button need imported files; the folder button needs a desktop.
        let actions = |e: &Env| -> Vec<Action> {
            layout(Page::General, e)
                .targets
                .into_iter()
                .filter_map(|(t, _)| if let Target::Action(a) = t { Some(a) } else { None })
                .collect()
        };
        assert_eq!(actions(&desktop), [Action::OpenFolder, Action::Reimport]);
        let on_phone = Env {
            data: Some("x".into()),
            ..env(true)
        };
        assert_eq!(actions(&on_phone), [Action::Reimport]);
        let installed_in_place = Env {
            data: None,
            ..env(false)
        };
        assert_eq!(actions(&installed_in_place), [Action::OpenFolder]);
        assert!(layout(Page::General, &installed_in_place).texts.is_empty());
    }

    #[test]
    fn every_target_fits_the_screen_and_none_overlap_on_each_page() {
        let envs = [
            env(false),
            env(true),
            Env {
                updates: false,
                data: None,
                ..env(false)
            },
            Env {
                data: Some("x".into()),
                ..env(true)
            },
            env_with_mods(),
            Env {
                pad: true,
                touch_input: true,
                ..env_with_mods()
            },
        ];
        let inside = |r: Rect, outer: Rect| {
            r.x >= outer.x
                && r.y >= outer.y
                && r.x + r.w as i32 <= outer.x + outer.w as i32
                && r.y + r.h as i32 <= outer.y + outer.h as i32
        };
        let apart = |a: Rect, b: Rect| {
            a.x + a.w as i32 <= b.x || b.x + b.w as i32 <= a.x || a.y + a.h as i32 <= b.y || b.y + b.h as i32 <= a.y
        };
        let screen = Rect::new(0, 0, 640, 480);
        assert!(inside(PANEL, screen));
        // The labels have room left of the drop-downs.
        const { assert!(DROP_X - LABEL_X >= 250) };
        for e in &envs {
            for page in pages(e) {
                let s = screen_with(e.clone());
                let mut s = s;
                s.page = page;
                let mut all: Vec<(String, Rect)> = s
                    .page_targets()
                    .into_iter()
                    .map(|(t, r)| (format!("{t:?}"), r))
                    .collect();
                let lay = layout(page, e);
                for (i, (_, r)) in lay.texts.iter().enumerate() {
                    all.push((format!("text {i}"), *r));
                }
                all.push(("description".to_string(), DESC));
                all.push(("heading".to_string(), HEADING));
                for (name, r) in &all {
                    assert!(inside(*r, PANEL), "{name} on {page:?} is inside the panel: {r:?}");
                    assert!(inside(*r, screen), "{name} on {page:?} is on the screen");
                    // The scroll bar of the Keys page is 16 wide, as the Select scenario dialog's is: a keyboard page.
                    if name != "description"
                        && name != "heading"
                        && !name.starts_with("text")
                        && !name.starts_with("KeyTrack")
                    {
                        assert!(r.h >= 24 && r.w >= 24, "{name} on {page:?} is big enough to tap: {r:?}");
                    }
                }
                for (i, (na, a)) in all.iter().enumerate() {
                    for (nb, b) in &all[i + 1..] {
                        assert!(apart(*a, *b), "{na} and {nb} overlap on {page:?}: {a:?} {b:?}");
                    }
                }
                // The drop-down boxes sit inside their rows, and the labels have room left of them.
                for (t, r) in &lay.targets {
                    if matches!(t, Target::Row(_)) {
                        assert!(inside(drop_rect(*r), *r));
                    }
                }
            }
        }
    }

    #[test]
    fn the_longest_list_fits_the_screen() {
        // The percent list has 11 lines (12 with an odd value) and opens under its box or above it.
        let mut s = screen();
        s.page = Page::Sound;
        click(&mut s, Target::Row(Item::Extra(Id::VolumeMusic)));
        let r = s.open.as_ref().unwrap().popup.rect();
        assert!(r.y >= 0 && r.y + r.h as i32 <= 480, "{r:?}");
        let mut s = screen();
        s.page = Page::Interface;
        click(&mut s, Target::Row(Item::Extra(Id::AutoReplay)));
        let r = s.open.as_ref().unwrap().popup.rect();
        assert!(r.y >= 0 && r.y + r.h as i32 <= 480, "{r:?}");
    }

    #[test]
    fn the_descriptions_say_when_a_change_takes_effect() {
        assert!(describe(Item::Extra(Id::RallyPoints)).contains("next game"));
        assert!(describe(Item::Extra(Id::Widescreen)).contains("next game"));
        assert!(describe(Item::Extra(Id::Widescreen)).contains("touch"));
        assert!(!describe(Item::Extra(Id::HealthBars)).contains("next game"));
        assert!(describe(Item::IntroVideo).contains("next time"));
        assert!(describe(Item::UpdateCheck).contains("next time"));
        assert!(describe_action(Action::OpenFolder).contains("[keys]"));
        assert!(GAMEPLAY_NOTE.contains("host") && GAMEPLAY_NOTE.contains("single-player"));
    }

    // --- The Keys page ---------------------------------------------------------------------------------------

    fn keys_screen() -> Screen {
        let mut s = screen();
        click(&mut s, Target::Tab(Page::Keys));
        s
    }

    /// A key pressed with the modifiers held.
    fn chord_key(s: &mut Screen, k: Key, ctrl: bool, alt: bool, shift: bool) -> Outcome {
        s.update(&Input {
            keys: vec![k],
            ctrl,
            alt,
            shift,
            ..Default::default()
        })
    }

    fn row_of(s: &Screen, name: &str) -> usize {
        s.keys.rows.iter().position(|r| r.name == name).expect(name)
    }

    /// Chooses the row of `name`: scrolls to it and presses Enter on the list.
    fn choose(s: &mut Screen, name: &str) {
        s.focus = Target::KeyList;
        let at = row_of(s, name);
        s.select_key(at);
        key(s, Key::Enter);
        assert!(s.keys.capturing, "{name} waits for a key");
    }

    #[test]
    fn the_keys_page_is_a_fifth_tab_that_a_phone_does_not_have() {
        assert_eq!(
            pages(&env(false)),
            [Page::General, Page::Interface, Page::Sound, Page::Gameplay, Page::Keys]
        );
        assert_eq!(
            pages(&env(true)),
            [Page::General, Page::Interface, Page::Sound, Page::Gameplay, Page::Touch]
        );
        let s = keys_screen();
        assert_eq!(s.page(), Page::Keys);
        let tabs: Vec<Target> = s
            .page_targets()
            .into_iter()
            .map(|(t, _)| t)
            .filter(|t| matches!(t, Target::Tab(_)))
            .collect();
        assert_eq!(tabs.len(), 5);
        // On a phone PageDown never reaches it.
        let mut p = screen_with(env(true));
        p.page = Page::Gameplay;
        key(&mut p, Key::PageDown);
        assert_eq!(p.page(), Page::Touch);
        assert!(
            p.page_targets()
                .iter()
                .all(|(t, _)| !matches!(t, Target::Tab(Page::Keys)))
        );
        // A computer with touch in use, a controller connected and a mod to enable has all eight, and they fit the
        // panel without touching.
        let all = pages(&Env {
            touch_input: true,
            pad: true,
            ..env_with_mods()
        });
        assert_eq!(all, Page::ALL);
        for (i, a) in all.iter().enumerate() {
            let r = tab_rect(*a, &all);
            assert!(r.x >= PANEL.x && r.x + r.w as i32 <= PANEL.x + PANEL.w as i32);
            if let Some(b) = all.get(i + 1) {
                assert!(r.x + r.w as i32 <= tab_rect(*b, &all).x);
            }
        }
    }

    #[test]
    fn enter_asks_for_a_key_and_the_next_press_binds_it() {
        let mut s = keys_screen();
        choose(&mut s, "pause");
        assert_eq!(s.described(), "Press a key... (Esc cancels)");
        // A modifier alone waits for the key.
        chord_key(&mut s, Key::ControlLeft, true, false, false);
        assert!(s.keys.capturing);
        assert_eq!(chord_key(&mut s, Key::Letter('U'), true, false, false), Outcome::Stay);
        assert!(!s.keys.capturing);
        assert_eq!(s.values.key_lines(), ["pause=ctrl+u"]);
        assert!(s.values.keys_touched);
        let i = row_of(&s, "pause");
        assert_eq!(s.key_text(i), ("Ctrl+U".to_string(), true));
        assert_eq!(s.key_text(row_of(&s, "speed-up")), ("+".to_string(), false));
        assert!(
            s.described().contains("Pause or resume is now Ctrl+U"),
            "{}",
            s.described()
        );
        // A chord with Alt and Shift, and a function key, bind the same way.
        choose(&mut s, "tips");
        chord_key(&mut s, Key::Letter('Y'), false, true, true);
        // A digit is a chord too (the digits are control groups: this one is free once group 3 gave it up).
        choose(&mut s, "group-recall-3");
        key(&mut s, Key::F(12));
        assert!(
            s.described().contains("Load game"),
            "F12 is the original's load key: {}",
            s.described()
        );
        assert_eq!(s.values.key_lines(), ["pause=ctrl+u", "tips=alt+shift+y"]);
    }

    #[test]
    fn a_click_asks_for_a_key_too_and_esc_gives_up_without_leaving() {
        let mut s = keys_screen();
        let i = row_of(&s, "speed-down");
        click(&mut s, Target::KeyRow(i));
        assert!(s.keys.capturing && s.keys.sel == i);
        assert_eq!(s.focus, Target::KeyList);
        assert_eq!(key(&mut s, Key::Escape), Outcome::Stay, "Esc only gives up the wait");
        assert!(!s.keys.capturing && s.values.key_lines().is_empty());
        assert!(!s.values.keys_touched);
        // A click anywhere else gives up too.
        click(&mut s, Target::KeyRow(i));
        click(&mut s, Target::Tab(Page::Sound));
        assert!(!s.keys.capturing);
        // And Esc with nothing to wait for leaves the screen as ever.
        assert_eq!(key(&mut s, Key::Escape), Outcome::Cancel);
    }

    #[test]
    fn a_key_another_action_has_is_not_bound_and_the_description_names_it() {
        let mut s = keys_screen();
        // Ctrl+S is the original's Sound effects on or off; F5 Options; F6 the idle worker's.
        for (name, chord, owner) in [
            ("pause", (Key::Letter('S'), true), "Sound effects on or off"),
            ("speed-up", (Key::F(5), false), "Options"),
            ("group-set-2", (Key::F(6), false), "Select the next idle worker"),
        ] {
            choose(&mut s, name);
            chord_key(&mut s, chord.0, chord.1, false, false);
            assert!(s.values.key_lines().is_empty(), "{name} was not bound");
            let d = s.described();
            assert!(d.contains(owner) && d.contains("already used"), "{d}");
            assert!(!s.keys.capturing);
        }
        // Another action's own new key is refused the same way, until that one moves away (two actions swap in two steps).
        choose(&mut s, "pause");
        chord_key(&mut s, Key::Letter('U'), true, false, false);
        choose(&mut s, "tips");
        chord_key(&mut s, Key::Letter('U'), true, false, false);
        assert!(s.described().contains("Pause or resume"), "{}", s.described());
        assert_eq!(s.values.key_lines(), ["pause=ctrl+u"]);
        choose(&mut s, "speed-up");
        key(&mut s, Key::Minus);
        assert!(
            s.described().contains("Game speed down"),
            "minus is speed-down's until it moves"
        );
        choose(&mut s, "speed-down");
        key(&mut s, Key::Equal);
        assert!(
            s.described().contains("Game speed up"),
            "so is plus speed-up's: no one-step swap"
        );
        choose(&mut s, "speed-down");
        chord_key(&mut s, Key::Letter('U'), false, true, false);
        choose(&mut s, "speed-up");
        key(&mut s, Key::Minus);
        assert!(s.described().contains("Game speed up is now -"), "{}", s.described());
        choose(&mut s, "speed-down");
        key(&mut s, Key::Equal);
        assert!(s.described().contains("Game speed down is now +"), "{}", s.described());
        assert_eq!(
            s.values.key_lines(),
            ["pause=ctrl+u", "speed-up=minus", "speed-down=equal"]
        );
    }

    #[test]
    fn a_key_with_the_wrong_shape_or_no_name_says_so_and_binds_nothing() {
        let mut s = keys_screen();
        // A letter on its own belongs to the command buttons.
        choose(&mut s, "pause");
        key(&mut s, Key::Letter('U'));
        assert!(s.described().contains("command buttons"), "{}", s.described());
        // A command button takes a bare letter or digit, not a chord.
        choose(&mut s, "button-stop");
        chord_key(&mut s, Key::Letter('X'), true, false, false);
        assert!(s.described().contains("bare letter or digit"), "{}", s.described());
        // F10 stays the game menu's, and the arrows have no name.
        choose(&mut s, "tips");
        key(&mut s, Key::F(10));
        assert!(s.described().contains("F10"), "{}", s.described());
        choose(&mut s, "tips");
        key(&mut s, Key::Up);
        assert!(s.described().contains("cannot be used"), "{}", s.described());
        assert!(s.values.key_lines().is_empty());
        // The right shape binds: a bare letter for a button.
        choose(&mut s, "button-stop");
        key(&mut s, Key::Letter('X'));
        assert_eq!(s.values.key_lines(), ["button-stop=x"]);
        assert_eq!(s.key_text(row_of(&s, "button-stop")).0, "X");
    }

    #[test]
    fn the_settings_key_is_not_free_to_bind() {
        let mut s = keys_screen();
        choose(&mut s, "pause");
        chord_key(&mut s, Key::Letter('G'), true, false, false);
        assert!(s.described().contains("settings"), "{}", s.described());
        assert!(s.values.key_lines().is_empty());
    }

    #[test]
    fn backspace_gives_a_row_its_key_back_and_reset_keys_every_row() {
        let mut s = keys_screen();
        for (name, k) in [("pause", Key::Letter('U')), ("tips", Key::Letter('Y'))] {
            choose(&mut s, name);
            chord_key(&mut s, k, true, false, false);
        }
        let i = row_of(&s, "pause");
        s.select_key(i);
        key(&mut s, Key::Backspace);
        assert_eq!(s.values.key_lines(), ["tips=ctrl+y"]);
        assert_eq!(s.key_text(i), ("Ctrl+P".to_string(), false));
        assert!(s.described().contains("original key again"));
        // Reset keys: every binding goes; a comment the player wrote and a line that names no action stay.
        let mut prefs = Prefs::default();
        prefs.keys = KeyMap::parse(&[
            "# mine".into(),
            "tips=ctrl+y".into(),
            "typo=ctrl+u".into(),
            "pause=ctrl+u".into(),
        ])
        .0;
        let mut r = Screen::new(env(false), Values::new(&prefs, &ClientExtras::default(), false, false));
        r.page = Page::Keys;
        assert_eq!(r.key_text(row_of(&r, "pause")).0, "Ctrl+U");
        assert_eq!(click(&mut r, Target::Action(Action::ResetKeys)), Outcome::Stay);
        assert_eq!(r.values.key_lines(), ["# mine", "typo=ctrl+u"]);
        assert!(r.keys.map.is_default());
        assert_eq!(r.key_text(row_of(&r, "pause")), ("Ctrl+P".to_string(), false));
        assert!(r.values.keys_touched);
        // Defaults on the other pages leaves the keys alone.
        let mut d = keys_screen();
        choose(&mut d, "pause");
        chord_key(&mut d, Key::Letter('U'), true, false, false);
        click(&mut d, Target::Defaults);
        assert_eq!(d.values.key_lines(), ["pause=ctrl+u"]);
    }

    #[test]
    fn ok_stores_the_keys_in_the_prefs_and_the_settings_text_holds_them() {
        let mut s = keys_screen();
        choose(&mut s, "pause");
        chord_key(&mut s, Key::Letter('U'), true, false, false);
        choose(&mut s, "idle-worker");
        chord_key(&mut s, Key::Letter('I'), true, false, false);
        choose(&mut s, "group-recall-1");
        key(&mut s, Key::F(6));
        assert!(s.described().contains("is now F6"), "{}", s.described());
        assert_eq!(click(&mut s, Target::Ok), Outcome::Ok);
        let mut stored = Prefs::default();
        let (mut live, mut audio) = (ClientExtras::default(), Audio::default());
        commit(s.values(), &mut stored, &mut live, &mut audio);
        let text = stored.to_text();
        assert!(
            text.ends_with("[keys]\npause=ctrl+u\nidle-worker=ctrl+i\ngroup-recall-1=f6\n"),
            "{text}"
        );
        // The file's own text reads back to the same keys, which the game acts on.
        let back = Prefs::from_text(&text);
        assert!(back.key_problems.is_empty());
        assert_eq!(back.keys, stored.keys);
        assert_eq!(back.keys.chord_of("pause").map(|c| c.text()), Some("Ctrl+U".into()));
        let ctrl = KeyMods {
            ctrl: true,
            ..Default::default()
        };
        let (k, m) = back.keys.game(Key::Letter('U'), ctrl).expect("Ctrl+U now means pause");
        assert_eq!(
            crate::mouse::key_action(k, m, false, true, false),
            Some(crate::mouse::KeyAction::Pause)
        );
        assert!(back.keys.idle_pressed(Key::Letter('I'), ctrl));
        // Opening the screen again shows them.
        let mut again = Screen::new(env(false), Values::new(&back, &ClientExtras::default(), false, false));
        again.page = Page::Keys;
        assert_eq!(again.key_text(row_of(&again, "pause")), ("Ctrl+U".to_string(), true));
        assert_eq!(again.key_text(row_of(&again, "idle-worker")).0, "Ctrl+I");
    }

    #[test]
    fn cancel_leaves_the_keys_as_they_were() {
        let mut s = keys_screen();
        choose(&mut s, "pause");
        chord_key(&mut s, Key::Letter('U'), true, false, false);
        assert_eq!(click(&mut s, Target::Cancel), Outcome::Cancel);
        // The frontend calls `commit` for OK alone, so the stored keys are untouched.
        let stored = Prefs::default();
        assert!(stored.keys.is_default());
        // And an OK without a change to the keys does not rewrite them (a typo in the file is kept as it is).
        let mut prefs = Prefs::default();
        prefs.keys = KeyMap::parse(&["pause=ctrl+nope".into()]).0;
        let mut other = Screen::new(env(false), Values::new(&prefs, &ClientExtras::default(), false, false));
        pick(&mut other, Item::Window, 1);
        let (mut stored, mut live, mut audio) = (prefs.clone(), ClientExtras::default(), Audio::default());
        commit(other.values(), &mut stored, &mut live, &mut audio);
        assert_eq!(stored.keys.lines(), ["pause=ctrl+nope"]);
    }

    #[test]
    fn the_list_scrolls_by_the_wheel_the_bar_and_the_arrows() {
        let mut s = keys_screen();
        let n = s.keys.rows.len();
        assert!(n > KEYS_LINES, "more actions than lines");
        assert_eq!(s.keys.top, 0);
        // Wheel lines, positive away from the user: down the list is negative.
        let over = |s: &mut Screen, wheel: i32| {
            let r = KEYS_LIST;
            s.update(&Input {
                at: Some((r.x + 10, r.y + 10)),
                wheel,
                ..Default::default()
            })
        };
        over(&mut s, -3);
        assert_eq!(s.keys.top, 3);
        over(&mut s, 1);
        assert_eq!(s.keys.top, 2);
        over(&mut s, 50);
        assert_eq!(s.keys.top, 0, "never above the first row");
        over(&mut s, -500);
        assert_eq!(s.keys.top, n - KEYS_LINES, "never past the last");
        // The bar's track above the thumb pages up, below it pages down.
        click(&mut s, Target::KeyTrack(false));
        assert_eq!(s.keys.top, n - 2 * KEYS_LINES);
        click(&mut s, Target::KeyTrack(true));
        assert_eq!(s.keys.top, n - KEYS_LINES);
        // The arrows move the selection and keep it in view; Home and End jump.
        s.focus = Target::KeyList;
        key(&mut s, Key::Home);
        assert_eq!((s.keys.sel, s.keys.top), (0, 0));
        for _ in 0..KEYS_LINES {
            key(&mut s, Key::Down);
        }
        assert_eq!((s.keys.sel, s.keys.top), (KEYS_LINES, 1));
        key(&mut s, Key::End);
        assert_eq!((s.keys.sel, s.keys.top), (n - 1, n - KEYS_LINES));
        key(&mut s, Key::Down);
        assert_eq!(s.keys.sel, n - 1, "stops at the last row");
        // The thumb is in the track, at the bottom now.
        let t = key_thumb(s.keys.top, n);
        assert_eq!(t.y + t.h as i32, KEYS_TRACK.y + KEYS_TRACK.h as i32);
        // A short list has no thumb to move.
        assert_eq!(key_thumb(0, KEYS_LINES), KEYS_TRACK);
        // Tab leaves the list for Reset keys.
        key(&mut s, Key::Tab);
        assert_eq!(s.focus, Target::Action(Action::ResetKeys));
    }

    #[test]
    fn the_list_and_reset_button_clear_each_other_the_description_and_the_buttons() {
        let s = keys_screen();
        let apart = |a: Rect, b: Rect| {
            a.x + a.w as i32 <= b.x || b.x + b.w as i32 <= a.x || a.y + a.h as i32 <= b.y || b.y + b.h as i32 <= a.y
        };
        for r in [KEYS_LIST, KEYS_TRACK, KEYS_RESET] {
            assert!(apart(r, DESC) && apart(r, OK) && apart(r, CANCEL) && apart(r, DEFAULTS));
        }
        assert!(apart(KEYS_LIST, KEYS_TRACK) && apart(KEYS_LIST, KEYS_RESET));
        assert!(KEYS_LIST.x + KEYS_LIST.w as i32 <= KEYS_TRACK.x);
        assert_eq!(KEYS_LIST.h, KEYS_LINES as u32 * KEYS_LINE_H as u32);
        const { assert!(KEYS_LINE_H >= 24 && KEYS_RESET.h >= 24 && KEYS_RESET.w >= 24) };
        // The Tab order: the tabs, the list, Reset keys, then the buttons.
        let stops = s.stops();
        assert_eq!(
            &stops[5..],
            [
                Target::KeyList,
                Target::Action(Action::ResetKeys),
                Target::Ok,
                Target::Cancel,
                Target::Defaults
            ]
        );
    }

    /// A synthetic font: every printable glyph 7 pixels wide, 10 high.
    fn font() -> Font {
        let g = tidewater_formats::fnt::Glyph {
            width: 6,
            height: 10,
            x: 0,
            y: 0,
            shades: vec![1; 60],
        };
        let mut glyphs = vec![None];
        glyphs.extend((33..=126).map(|_| Some(g.clone())));
        Font {
            first: 32,
            max_width: 6,
            height: 10,
            glyphs,
        }
    }

    #[test]
    fn every_description_wraps_into_the_room_it_has() {
        // At 7 pixels a letter (more than the game's small font uses) every description fits four lines.
        let f = font();
        let mut all: Vec<String> = Vec::new();
        for page in Page::ALL {
            for item in items_on(page) {
                all.push(describe(item));
            }
            all.push(page.blurb().to_string());
        }
        all.push(describe_action(Action::OpenFolder).to_string());
        all.push(describe_action(Action::Reimport).to_string());
        all.push(describe_action(Action::ResetKeys).to_string());
        // Every action of the Keys page, in the longest form its description takes (the player's own key).
        let s = screen();
        for i in 0..s.keys.rows.len() {
            all.push(s.key_description(i));
        }
        for d in all {
            let lines = wrap(&f, &d, DESC.w);
            assert!(lines.len() <= 4, "{} lines for: {d}", lines.len());
            assert!(lines.iter().all(|l| menuscreen::text_width(&f, l, 8) <= DESC.w));
        }
        assert!(wrap(&f, GAMEPLAY_NOTE, ROW_W - 16).len() <= 3);
    }

    #[test]
    fn the_screen_draws_every_page_without_leaving_the_panel() {
        let f = font();
        let mut pal = Palette([[0; 3]; 256]);
        for (i, c) in pal.0.iter_mut().enumerate() {
            *c = [i as u8, 255 - i as u8, 7];
        }
        let faces = menuscreen::ButtonFaces::default();
        let look = Look {
            font: &f,
            pal: &pal,
            faces: &faces,
        };
        let bg = Picture {
            width: 640,
            height: 480,
            rgba: vec![9; 640 * 480 * 4],
        };
        for e in [env(false), env(true)] {
            let mut s = screen_with(e);
            for page in Page::ALL {
                s.page = page;
                let p = s.draw(&bg, &look, &f);
                assert_eq!((p.width, p.height), (640, 480));
                assert_ne!(p.rgba, bg.rgba);
                // Nothing is drawn outside the panel.
                for y in 0..480usize {
                    for x in 0..640usize {
                        let outside = !(PANEL.x as usize..(PANEL.x as usize + PANEL.w as usize)).contains(&x)
                            || !(PANEL.y as usize..(PANEL.y as usize + PANEL.h as usize)).contains(&y);
                        if outside {
                            assert_eq!(p.rgba[(y * 640 + x) * 4], 9, "({x}, {y}) on {page:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_folder_becomes_a_file_url() {
        assert_eq!(
            folder_url(std::path::Path::new("/data/a b/Tide-water")),
            "file:///data/a%20b/Tide-water"
        );
        assert_eq!(folder_url(std::path::Path::new("/x/é")), "file:///x/%C3%A9");
        // A relative folder is made absolute first.
        assert!(folder_url(std::path::Path::new("some")).starts_with("file:///"));
    }

    #[test]
    fn the_data_line_names_the_source_the_files_and_the_maps() {
        use tidewater_import::manifest::FileEntry;
        let f = |p: &str| FileEntry {
            path: p.into(),
            size: 1,
            sha256: String::new(),
        };
        let mut m = tidewater_import::Manifest::new(
            "gog-installer",
            "x",
            vec![
                f("War2Dat.mpq"),
                f("Maps/A.pud"),
                f("Maps/Classic/B.PUD"),
                f("Maps/readme.txt"),
            ],
        );
        m.imported = "2026-10-23T21:18:44Z".into();
        let s = data_summary(&m);
        assert_eq!(s, "Game files: GOG installer, 4 files, 2 maps, imported 2026-10-23");
        m.source_kind = "something-new".into();
        assert!(data_summary(&m).contains("an earlier import"));
    }

    fn controls_words(e: &Env, current: u8) -> Vec<String> {
        offered(Item::Extra(Id::Controls), current, e)
            .into_iter()
            .map(|(_, w)| w)
            .collect()
    }

    fn with_devices(pad: bool, touch: bool, mouse: bool) -> Env {
        Env {
            devices: crate::controls::Devices { pad, touch, mouse },
            ..env(false)
        }
    }

    #[test]
    fn the_controls_row_is_on_the_general_page_with_its_words() {
        assert_eq!(extra_page(Id::Controls), Page::General);
        assert!(items_on(Page::General).contains(&Item::Extra(Id::Controls)));
        assert_eq!(Id::Controls.spec().label, "Controls");
        assert_eq!(
            options(Item::Extra(Id::Controls), 0)
                .into_iter()
                .map(|(_, w)| w)
                .collect::<Vec<_>>(),
            [
                "Automatic",
                "Keyboard and mouse",
                "Touch",
                "Controller",
                "Controller and touch"
            ]
        );
    }

    #[test]
    fn the_controls_drop_down_offers_only_the_devices_there_are() {
        // A computer: Automatic and Keyboard and mouse.
        assert_eq!(
            controls_words(&with_devices(false, false, true), 0),
            ["Automatic", "Keyboard and mouse"]
        );
        // A computer with a controller.
        assert_eq!(
            controls_words(&with_devices(true, false, true), 0),
            ["Automatic", "Keyboard and mouse", "Controller"]
        );
        // A computer whose touch screen has been used.
        assert_eq!(
            controls_words(&with_devices(false, true, true), 0),
            ["Automatic", "Keyboard and mouse", "Touch"]
        );
        // A handheld with a pad: no mouse until one is used.
        assert_eq!(
            controls_words(&with_devices(true, true, false), 0),
            ["Automatic", "Touch", "Controller", "Controller and touch"]
        );
        // Every device.
        assert_eq!(controls_words(&with_devices(true, true, true), 0).len(), 5);
        // Other rows offer everything.
        assert_eq!(
            offered(Item::Extra(Id::Scaling), 0, &with_devices(false, false, false)).len(),
            3
        );
    }

    #[test]
    fn left_and_right_skip_the_controls_that_are_not_there() {
        let mut s = screen_with(with_devices(true, false, true));
        let item = Item::Extra(Id::Controls);
        let cur = |s: &Screen| s.values.get(item);
        assert_eq!(cur(&s), 0);
        s.cycle(item, 1);
        assert_eq!(cur(&s), 1, "keyboard and mouse");
        s.cycle(item, 1);
        assert_eq!(cur(&s), 3, "controller, past touch");
        s.cycle(item, 1);
        assert_eq!(cur(&s), 0, "wraps to Automatic");
        s.cycle(item, -1);
        assert_eq!(cur(&s), 3);
        // A stored choice that is not there is shown as itself, and a step leaves it for an offered one.
        s.values.set(item, 2);
        let word = options(item, 2).into_iter().find(|(n, _)| *n == 2).map(|(_, w)| w);
        assert_eq!(word.as_deref(), Some("Touch"));
        assert!(describe_now(item, 2, &s.env).contains("Automatic is in use"));
        assert!(!describe_now(item, 3, &s.env).contains("Automatic is in use"));
        s.cycle(item, 1);
        assert!(s.env.devices.available(crate::controls::Mode::from_index(cur(&s))));
    }
}
