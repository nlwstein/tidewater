//! Rebindable keys (the `[keys]` section of `settings.txt`, `docs/design/extras.md`). With no `[keys]` section the
//! keymap is empty and every key does what the original's does: [`KeyMap::game`] hands the pressed key on unchanged,
//! [`KeyMap::button`] finds a command button by the hotkey of its string-table entry, and the idle-worker key is F6.
//!
//! **What can be rebound.**
//! - **Game keys**: every action of the in-game key table (`mouse::key_action`), by name: `speed-up`, `speed-down`,
//!   `pause`, `centre-primary`, `group-set-0` to `group-set-9`, `group-recall-0` to `group-recall-9`, `group-show-0` to
//!   `group-show-9` (the Alt+digit recall, which also shows the group), `goto-view-1` to `goto-view-3`,
//!   `remember-view-1` to `remember-view-3`, and the menus and dialogs: `game-menu`, `options`, `help-menu`,
//!   `keyboard-help`, `sound-dialog`, `speed-dialog`, `preferences`, `tips`, `save-game`, `load-game`, `restart`, `quit`,
//!   `quit-dialog`, `toggle-music`, `toggle-sounds`. Also `idle-worker` (the key of option `idle-worker`; F6 by default) and `select-screen` and
//!   `select-map` (option `quick-select`; Ctrl+A and Alt+A by default).
//! - **Command buttons**: `button-move`, `button-attack`, `button-patrol`, `button-stop`, `button-stand-ground`,
//!   `button-harvest`, `button-repair`, `button-return-goods`, `button-unload`, `button-attack-ground`,
//!   `button-demolish`, `button-build-basic` and `button-build-advanced` each give the button with that command its
//!   own key (its original hotkey stops working). Every other button (training, building, research, spells) is reached
//!   by place with `slot-1` to `slot-9`: the button in that place of the 3 × 3 panel, whatever it is, so a keyboard grid
//!   works. A slot key wins over a button's own hotkey letter.
//!
//! **The file.** One `name=key` line each, below a `[keys]` line. A key is a chord: optional `ctrl+`, `alt+`, `shift+`,
//! then a key name: a letter `a` to `z`, a digit `0` to `9`, `f1` to `f12` (not `f10`), `minus`, `equal` (also called
//! `plus` or `numpad-add`) or `numpad-subtract`. Escape, Enter, Space, the arrow keys and F10 stay as they are.
//! Game keys need ctrl or alt with a letter (a plain letter belongs to the command buttons), and button and slot keys
//! are a bare letter or digit.
//!
//! **Conflicts.** The same key on two actions is refused with a message that names both; so is a key that an action you
//! did not rebind still uses (a bare digit is a control group, F2 to F4 are the views, Ctrl+G opens the Settings screen, and so on). The refused line is
//! left out and the first line wins. Rebinding an action frees its original keys, so two actions can swap. Whatever
//! the file holds is written back as it is, so a typo is never lost. Two command buttons that show at once with the
//! same hotkey letter are not detected here (which buttons show depends on the selection); the first in panel order acts.

use std::collections::{HashMap, HashSet};

use crate::input::Key;
use crate::mouse::{self, GroupKey, KeyAction};
use crate::panel::{Act, Entry, Hotkey, Label, Menu, Target};

/// A key and its modifiers as the in-game key table names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: mouse::Key,
    pub mods: mouse::KeyMods,
}

impl Chord {
    /// The chord as the player reads it: `Ctrl+Shift+P`, `F6`.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (on, name) in [
            (self.mods.ctrl, "Ctrl+"),
            (self.mods.alt, "Alt+"),
            (self.mods.shift, "Shift+"),
        ] {
            if on {
                out.push_str(name);
            }
        }
        out.push_str(&match self.key {
            mouse::Key::Letter(c) => c.to_string(),
            mouse::Key::Digit(d) => d.to_string(),
            mouse::Key::Plus => "+".into(),
            mouse::Key::Minus => "-".into(),
            mouse::Key::F(n) => format!("F{n}"),
        });
        out
    }
}

impl Chord {
    /// The chord as the settings file writes it (`ctrl+alt+x`, `f6`, `minus`, `equal`), what [`parse_chord`] reads.
    pub fn file_text(&self) -> String {
        let mut out = String::new();
        for (on, name) in [
            (self.mods.ctrl, "ctrl+"),
            (self.mods.alt, "alt+"),
            (self.mods.shift, "shift+"),
        ] {
            if on {
                out.push_str(name);
            }
        }
        out.push_str(&match self.key {
            mouse::Key::Letter(c) => c.to_ascii_lowercase().to_string(),
            mouse::Key::Digit(d) => d.to_string(),
            mouse::Key::Plus => "equal".into(),
            mouse::Key::Minus => "minus".into(),
            mouse::Key::F(n) => format!("f{n}"),
        });
        out
    }

    /// The chord a pressed key with its modifiers makes, or `None` for a key the key table has no name for (the arrow
    /// keys, Enter, Escape, the keypad's digits and so on).
    pub fn pressed(key: Key, mods: mouse::KeyMods) -> Option<Chord> {
        Some(Chord {
            key: crate::game::game_key(key)?,
            mods,
        })
    }
}

// --- the original's table ------------------------------------------------------------------------------------------

/// The keys the in-game table knows, to list its default chords.
fn key_universe() -> Vec<mouse::Key> {
    let mut v: Vec<mouse::Key> = ('A'..='Z').map(mouse::Key::Letter).collect();
    v.extend((0..=9).map(mouse::Key::Digit));
    v.extend([mouse::Key::Plus, mouse::Key::Minus]);
    v.extend((1..=12).map(mouse::Key::F));
    v
}

/// All eight modifier combinations.
fn mod_combos() -> Vec<mouse::KeyMods> {
    (0..8u8)
        .map(|b| mouse::KeyMods {
            ctrl: b & 1 != 0,
            alt: b & 2 != 0,
            shift: b & 4 != 0,
        })
        .collect()
}

/// Every chord of the original's table and the action it gives (Enhanced, one human player, no chat line).
pub fn default_chords() -> Vec<(Chord, KeyAction)> {
    let mut out = Vec::new();
    for key in key_universe() {
        for mods in mod_combos() {
            if let Some(a) = mouse::key_action(key, mods, false, true, false) {
                out.push((Chord { key, mods }, a));
            }
        }
    }
    out
}

/// The action's own chord, the simplest of its default chords (fewest modifiers, Ctrl before Alt before Shift): what
/// a rebound key is turned into before the table is asked.
fn canonical(action: KeyAction, table: &[(Chord, KeyAction)]) -> Option<Chord> {
    table
        .iter()
        .filter(|(_, a)| *a == action)
        .map(|(c, _)| *c)
        .min_by_key(|c| {
            (
                c.mods.ctrl as u8 + c.mods.alt as u8 + c.mods.shift as u8,
                !c.mods.ctrl,
                !c.mods.alt,
            )
        })
}

/// The name of a game action in the settings file.
pub fn action_name(a: KeyAction) -> String {
    use KeyAction::*;
    match a {
        Speed(true) => "speed-up".into(),
        Speed(false) => "speed-down".into(),
        Group(GroupKey::Set, d) => format!("group-set-{d}"),
        Group(GroupKey::Recall, d) => format!("group-recall-{d}"),
        Group(GroupKey::AltRecall, d) => format!("group-show-{d}"),
        CentrePrimary => "centre-primary".into(),
        SoundDialog => "sound-dialog".into(),
        HelpMenu => "help-menu".into(),
        KeyboardHelp => "keyboard-help".into(),
        LoadGame => "load-game".into(),
        ToggleMusic => "toggle-music".into(),
        GameMenu => "game-menu".into(),
        Options => "options".into(),
        Pause => "pause".into(),
        Quit => "quit".into(),
        Restart => "restart".into(),
        ToggleSounds => "toggle-sounds".into(),
        SaveGame => "save-game".into(),
        Tips => "tips".into(),
        GotoView(n) => format!("goto-view-{}", n + 1),
        RememberView(n) => format!("remember-view-{}", n + 1),
        SpeedDialog => "speed-dialog".into(),
        Preferences => "preferences".into(),
        QuitDialog => "quit-dialog".into(),
    }
}

/// Every game action with a name (the ones that have a default chord).
pub fn game_actions() -> Vec<(String, KeyAction)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (_, a) in default_chords() {
        if seen.insert(action_name(a)) {
            out.push((action_name(a), a));
        }
    }
    out
}

// --- command buttons -----------------------------------------------------------------------------------------------

/// The buttons that can be given a key of their own (`button-NAME`), by their command.
pub const BUTTONS: &[&str] = &[
    "move",
    "attack",
    "patrol",
    "stop",
    "stand-ground",
    "harvest",
    "repair",
    "return-goods",
    "unload",
    "attack-ground",
    "demolish",
    "build-basic",
    "build-advanced",
];

/// The `button-NAME` a command's button answers to, if it has one.
pub fn button_name(act: Act) -> Option<&'static str> {
    Some(match act {
        Act::Target(Target::Move) => "move",
        Act::Target(Target::Attack) => "attack",
        Act::Target(Target::Patrol) => "patrol",
        Act::Target(Target::Harvest) => "harvest",
        Act::Target(Target::Repair) => "repair",
        Act::Target(Target::AttackGround) => "attack-ground",
        Act::Target(Target::Demolish) => "demolish",
        Act::Target(Target::Unload) | Act::Unload => "unload",
        Act::Stop => "stop",
        Act::StandGround => "stand-ground",
        Act::ReturnGoods => "return-goods",
        Act::Menu(Menu::Basic) => "build-basic",
        Act::Menu(Menu::Advanced) => "build-advanced",
        _ => return None,
    })
}

// --- key names -----------------------------------------------------------------------------------------------------

/// What a name in a chord stands for: the key as the in-game table has it.
fn key_by_name(name: &str) -> Result<mouse::Key, String> {
    let n = name.to_ascii_lowercase();
    let mut chars = n.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() {
            return Ok(mouse::Key::Letter(c.to_ascii_uppercase()));
        }
        if let Some(d) = c.to_digit(10) {
            return Ok(mouse::Key::Digit(d as u8));
        }
    }
    if let Some(f) = n.strip_prefix('f')
        && let Ok(f) = f.parse::<u8>()
    {
        return match f {
            10 => Err("F10 stays the game menu's key".into()),
            1..=12 => Ok(mouse::Key::F(f)),
            _ => Err(format!("\"{name}\" is not a key name")),
        };
    }
    match n.as_str() {
        "minus" | "numpad-subtract" => Ok(mouse::Key::Minus),
        "equal" | "plus" | "numpad-add" => Ok(mouse::Key::Plus),
        "escape" | "esc" | "enter" | "return" | "space" | "tab" | "up" | "down" | "left" | "right" | "backspace" => {
            Err(format!("{name} stays as it is and can't be rebound"))
        }
        _ => Err(format!(
            "\"{name}\" is not a key name (a letter, a digit, f1 to f12, minus or equal)"
        )),
    }
}

/// Reads `ctrl+alt+x` and the like.
pub fn parse_chord(text: &str) -> Result<Chord, String> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    // "+" alone or "ctrl++" is not how we write the plus key: say so.
    let (key, mods) = parts.split_last().ok_or_else(|| "no key".to_string())?;
    if key.is_empty() {
        return Err("no key name (write the plus key as \"equal\")".into());
    }
    let mut m = mouse::KeyMods::default();
    for part in mods {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => m.ctrl = true,
            "alt" => m.alt = true,
            "shift" => m.shift = true,
            other => return Err(format!("\"{other}\" is not ctrl, alt or shift")),
        }
    }
    Ok(Chord {
        key: key_by_name(key)?,
        mods: m,
    })
}

// --- the rows of the Keys page ---------------------------------------------------------------------------------------

/// One action the Keys page lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The name in the settings file.
    pub name: String,
    /// What it is called on the page.
    pub label: String,
    /// The key it has in the original, short enough for the list ("Ctrl+P", "F6", or "-" for an action with none).
    pub default: String,
    /// What the description says about the original's key.
    pub original: String,
}

/// The name of a command button as the page words it.
fn button_label(name: &str) -> &'static str {
    match name {
        "move" => "Move",
        "attack" => "Attack",
        "patrol" => "Patrol",
        "stop" => "Stop",
        "stand-ground" => "Stand ground",
        "harvest" => "Harvest",
        "repair" => "Repair",
        "return-goods" => "Return goods",
        "unload" => "Unload",
        "attack-ground" => "Attack ground",
        "demolish" => "Demolish",
        "build-basic" => "Build basic structure",
        "build-advanced" => "Build advanced structure",
        _ => "",
    }
}

/// What an action is called on the Keys page.
pub fn label_of(name: &str) -> String {
    let fixed = match name {
        "speed-up" => "Game speed up",
        "speed-down" => "Game speed down",
        "pause" => "Pause or resume",
        "centre-primary" => "Centre on the selected unit",
        "game-menu" => "Game menu",
        "options" => "Options",
        "help-menu" => "Help menu",
        "keyboard-help" => "Keystroke help",
        "sound-dialog" => "Sound dialog",
        "speed-dialog" => "Speeds dialog",
        "preferences" => "Preferences dialog",
        "tips" => "Tips",
        "save-game" => "Save game",
        "load-game" => "Load game",
        "restart" => "Restart the scenario",
        "quit" => "Quit to the main menu",
        "quit-dialog" => "Exit the program",
        "toggle-music" => "Music on or off",
        "toggle-sounds" => "Sound effects on or off",
        "idle-worker" => "Select the next idle worker",
        "select-screen" => "Select your fighting units on screen",
        "select-map" => "Select your fighting units on the map",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    for (prefix, text) in [
        ("group-set-", "Make group"),
        ("group-recall-", "Select group"),
        ("group-show-", "Select and show group"),
        ("goto-view-", "Go to view"),
        ("remember-view-", "Remember view"),
    ] {
        if let Some(n) = name.strip_prefix(prefix) {
            return format!("{text} {n}");
        }
    }
    if let Some(n) = name.strip_prefix("slot-") {
        return format!("Panel button {n}");
    }
    if let Some(b) = name.strip_prefix("button-") {
        return format!("Command: {}", button_label(b));
    }
    name.to_string()
}

/// The names of every rebindable action, in the order the Keys page lists them: the game keys, the menus and dialogs,
/// the remembered views, the control groups, then the command buttons and the panel's places.
fn row_names() -> Vec<String> {
    let mut v: Vec<String> = [
        "speed-up",
        "speed-down",
        "pause",
        "centre-primary",
        "idle-worker",
        "select-screen",
        "select-map",
        "game-menu",
        "options",
        "help-menu",
        "keyboard-help",
        "sound-dialog",
        "speed-dialog",
        "preferences",
        "tips",
        "save-game",
        "load-game",
        "restart",
        "quit",
        "quit-dialog",
        "toggle-music",
        "toggle-sounds",
    ]
    .map(String::from)
    .to_vec();
    for kind in ["goto-view-", "remember-view-"] {
        v.extend((1..=3).map(|n| format!("{kind}{n}")));
    }
    for kind in ["group-recall-", "group-set-", "group-show-"] {
        v.extend((0..=9).map(|n| format!("{kind}{n}")));
    }
    v.extend(BUTTONS.iter().map(|b| format!("button-{b}")));
    v.extend((1..=9).map(|n| format!("slot-{n}")));
    v
}

/// Every action that can be rebound, with the original's key for it. A test checks the list against the names the file
/// reads.
pub fn rows() -> Vec<Row> {
    let table = default_chords();
    let actions = game_actions();
    row_names()
        .into_iter()
        .map(|name| {
            let (default, original) = match bind_by_name(&name, &actions) {
                Some(Bind::Game(a)) => {
                    let mut all: Vec<Chord> = table.iter().filter(|(_, x)| *x == a).map(|(c, _)| *c).collect();
                    // F5 works with any modifiers held; the page names only the plainest chord of each key.
                    let within = |a: mouse::KeyMods, b: mouse::KeyMods| {
                        (!a.ctrl || b.ctrl) && (!a.alt || b.alt) && (!a.shift || b.shift)
                    };
                    let plain = all.clone();
                    all.retain(|c| {
                        !plain
                            .iter()
                            .any(|o| o.key == c.key && o.mods != c.mods && within(o.mods, c.mods))
                    });
                    all.sort_by_key(|c| (c.mods.ctrl as u8 + c.mods.alt as u8 + c.mods.shift as u8, !c.mods.ctrl));
                    let first = all.first().map_or_else(|| "-".to_string(), Chord::text);
                    let every: Vec<String> = all.iter().map(Chord::text).collect();
                    (first, every.join(" or "))
                }
                Some(Bind::Idle) => (
                    format!("F{}", crate::idle::KEY),
                    format!("F{} (with or without Ctrl, Alt or Shift)", crate::idle::KEY),
                ),
                // The original leaves both keys unbound.
                Some(Bind::Select(n)) => (SELECT_DEFAULTS[n].text(), "no key".to_string()),
                Some(Bind::Button(_)) => ("own letter".to_string(), "the letter on the button".to_string()),
                Some(Bind::Slot(_)) | None => ("-".to_string(), "none".to_string()),
            };
            Row {
                label: label_of(&name),
                name,
                default,
                original,
            }
        })
        .collect()
}

/// The `[keys]` lines with the action `name` given `chord`, or why not: the message names the other action that has the
/// key, or says what is wrong with the key's shape. The rules are the file's own (`KeyMap::parse`): a key is refused
/// when another action has it or when an action that was not rebound still uses it, so two actions swap keys by moving
/// one of them away first. Rebinding an action frees its original keys. Comments and the other lines stay as they are,
/// and the new line goes last so it is the one that is refused.
pub fn with_binding(lines: &[String], name: &str, chord: Chord) -> Result<Vec<String>, String> {
    let mut out = without_binding(lines, name);
    out.push(format!("{name}={}", chord.file_text()));
    let before = KeyMap::parse(&without_binding(lines, name)).1;
    let (_, after) = KeyMap::parse(&out);
    let prefix = format!("{name}=");
    let Some(problem) = after.iter().find(|p| p.starts_with(&prefix) && !before.contains(p)) else {
        return Ok(out);
    };
    // "pause=Ctrl+S: Ctrl+S is already used by toggle-sounds; the line is ignored"
    let msg = problem
        .split_once(": ")
        .map_or(problem.as_str(), |(_, m)| m)
        .trim_end_matches("; the line is ignored");
    if let Some((key, owner)) = msg.split_once(" is already used by ") {
        let owner = label_of(owner);
        return Err(format!(
            "{key} is already used by {owner}, so it was not changed. Give {owner} another key first, or choose              another key."
        ));
    }
    let mut msg = msg.to_string();
    if let Some(first) = msg.get(..1) {
        msg.replace_range(..1, &first.to_uppercase());
    }
    Err(format!("{msg}."))
}

/// The `[keys]` lines without any line for the action `name` (its own key again).
pub fn without_binding(lines: &[String], name: &str) -> Vec<String> {
    lines.iter().filter(|l| line_name(l) != Some(name)).cloned().collect()
}

/// The `[keys]` lines with every line for an action removed: the original's keys. Comments and lines that name no
/// action (a typo the player may want to read again) stay.
pub fn reset_lines(lines: &[String]) -> Vec<String> {
    let actions = game_actions();
    lines
        .iter()
        .filter(|l| line_name(l).is_none_or(|n| bind_by_name(n, &actions).is_none()))
        .cloned()
        .collect()
}

/// The action name a `name=key` line is for; `None` for a comment or a line without an equals sign.
fn line_name(line: &str) -> Option<&str> {
    let l = line.trim();
    if l.starts_with('#') {
        return None;
    }
    l.split_once('=').map(|(n, _)| n.trim())
}

// --- the keymap ----------------------------------------------------------------------------------------------------

/// What a name in `[keys]` binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bind {
    Game(KeyAction),
    Idle,
    /// A quick-select key (option `quick-select`): 0 on screen, 1 on the map.
    Select(usize),
    Slot(usize),
    Button(&'static str),
}

fn bind_by_name(name: &str, actions: &[(String, KeyAction)]) -> Option<Bind> {
    if name == "idle-worker" {
        return Some(Bind::Idle);
    }
    match name {
        "select-screen" => return Some(Bind::Select(0)),
        "select-map" => return Some(Bind::Select(1)),
        _ => {}
    }
    if let Some(a) = actions.iter().find(|(n, _)| n == name) {
        return Some(Bind::Game(a.1));
    }
    if let Some(n) = name.strip_prefix("slot-")
        && let Ok(n) = n.parse::<usize>()
        && (1..=9).contains(&n)
    {
        return Some(Bind::Slot(n - 1));
    }
    let b = name.strip_prefix("button-")?;
    BUTTONS.iter().find(|&&x| x == b).map(|&x| Bind::Button(x))
}

const SELECT_NAMES: [&str; 2] = ["select-screen", "select-map"];

/// The idle-worker key's default, F6 (any modifiers).
const IDLE_DEFAULT: mouse::Key = mouse::Key::F(crate::idle::KEY);

/// The quick-select keys' defaults (option `quick-select`): Ctrl+A for the units on screen, Alt+A for all on the map.
/// The original's table leaves both unbound.
const SELECT_DEFAULTS: [Chord; 2] = [
    Chord {
        key: mouse::Key::Letter('A'),
        mods: mouse::KeyMods {
            shift: false,
            ctrl: true,
            alt: false,
        },
    },
    Chord {
        key: mouse::Key::Letter('A'),
        mods: mouse::KeyMods {
            shift: false,
            ctrl: false,
            alt: true,
        },
    },
];

/// The keys of one run: the original's, with what the `[keys]` section changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyMap {
    /// The section's lines as the file has them, written back unchanged.
    raw: Vec<String>,
    /// A key a player gave an action, to the action's own chord in the original's table.
    game: HashMap<Chord, Chord>,
    /// The original chords of the actions that were given other keys: they do nothing now.
    unbound: HashSet<Chord>,
    /// The idle-worker key, when rebound.
    idle: Option<Chord>,
    /// The quick-select keys, when rebound.
    select: [Option<Chord>; 2],
    slots: [Option<char>; 9],
    buttons: Vec<(&'static str, char)>,
    /// The lines that were accepted: the action's name and the key the file gives it.
    bound: Vec<(String, Chord)>,
}

/// The messages of the lines a section had wrong.
pub type Problems = Vec<String>;

impl KeyMap {
    /// Reads the lines below `[keys]`. Lines that can't be used are left out and described in the second value.
    pub fn parse(lines: &[String]) -> (KeyMap, Problems) {
        let mut map = KeyMap {
            raw: lines
                .iter()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            ..KeyMap::default()
        };
        let mut problems = Vec::new();
        let table = default_chords();
        let actions = game_actions();

        // Pass 1: names and key texts.
        let mut wanted: Vec<(String, Bind, Chord)> = Vec::new();
        for line in map.raw.iter().filter(|l| !l.starts_with('#')) {
            let Some((name, value)) = line.split_once('=') else {
                problems.push(format!("\"{line}\" is not name=key"));
                continue;
            };
            let (name, value) = (name.trim(), value.trim());
            let Some(bind) = bind_by_name(name, &actions) else {
                problems.push(format!("\"{name}\" is not an action name (see docs/design/extras.md)"));
                continue;
            };
            if wanted.iter().any(|(n, ..)| n == name) {
                problems.push(format!("{name} is set twice; the first line is used"));
                continue;
            }
            let chord = match parse_chord(value) {
                Ok(c) => c,
                Err(e) => {
                    problems.push(format!("{name}={value}: {e}"));
                    continue;
                }
            };
            if let Err(e) = check_form(bind, chord) {
                problems.push(format!("{name}={value}: {e}"));
                continue;
            }
            wanted.push((name.to_string(), bind, chord));
        }

        // Pass 2: the keys the table uses, minus the ones of actions that are being rebound. A line that is refused
        // leaves its action on its original keys, so the others are checked again until nothing more is refused.
        loop {
            let mut taken: HashMap<Chord, String> = HashMap::new();
            for (c, a) in &table {
                taken.insert(*c, action_name(*a));
            }
            for m in mod_combos() {
                taken.insert(
                    Chord {
                        key: IDLE_DEFAULT,
                        mods: m,
                    },
                    "idle-worker".into(),
                );
            }
            for (n, c) in SELECT_DEFAULTS.iter().enumerate() {
                taken.insert(*c, SELECT_NAMES[n].into());
            }
            // Ours: the key that opens the Settings screen in a game belongs to no action and cannot be given to one.
            let (key, mods) = crate::gamemenu::SETTINGS_KEY;
            taken.insert(Chord { key, mods }, "settings".into());
            for (_, bind, _) in &wanted {
                match bind {
                    Bind::Game(a) => taken.retain(|_, owner| *owner != action_name(*a)),
                    Bind::Idle => taken.retain(|_, owner| owner != "idle-worker"),
                    Bind::Select(n) => taken.retain(|_, owner| owner != SELECT_NAMES[*n]),
                    _ => {}
                }
            }
            let mut refused = Vec::new();
            for (i, (name, _, chord)) in wanted.iter().enumerate() {
                if let Some(owner) = taken.get(chord) {
                    problems.push(format!(
                        "{name}={0}: {0} is already used by {owner}; the line is ignored",
                        chord.text()
                    ));
                    refused.push(i);
                } else {
                    taken.insert(*chord, name.clone());
                }
            }
            if refused.is_empty() {
                break;
            }
            for i in refused.into_iter().rev() {
                wanted.remove(i);
            }
        }
        for (name, bind, chord) in wanted {
            map.bound.push((name, chord));
            match bind {
                Bind::Game(a) => {
                    if let Some(canon) = canonical(a, &table) {
                        map.game.insert(chord, canon);
                        map.unbound
                            .extend(table.iter().filter(|(_, x)| *x == a).map(|(c, _)| *c));
                    }
                }
                Bind::Idle => map.idle = Some(chord),
                Bind::Select(n) => map.select[n] = Some(chord),
                Bind::Slot(n) => map.slots[n] = char_of(chord),
                Bind::Button(b) => {
                    if let Some(c) = char_of(chord) {
                        map.buttons.push((b, c));
                    }
                }
            }
        }
        (map, problems)
    }

    /// The lines of the `[keys]` section as the file has them (comments and lines that could not be used too).
    pub fn lines(&self) -> &[String] {
        &self.raw
    }

    /// The key the file gives the action `name`, if a line for it was accepted.
    pub fn chord_of(&self, name: &str) -> Option<Chord> {
        self.bound.iter().find(|(n, _)| n == name).map(|(_, c)| *c)
    }

    /// Whether the player has rebound anything.
    pub fn is_default(&self) -> bool {
        self.game.is_empty() && self.idle.is_none() && self.select.iter().all(Option::is_none) && !self.has_buttons()
    }

    fn has_buttons(&self) -> bool {
        self.slots.iter().any(Option::is_some) || !self.buttons.is_empty()
    }

    /// The `[keys]` section for the settings file: nothing when the file had none.
    pub fn section_text(&self) -> String {
        if self.raw.is_empty() {
            return String::new();
        }
        format!("[keys]\n{}\n", self.raw.join("\n"))
    }

    /// The key table's key for a pressed key: the pressed key itself unless the player rebound it (then the chord of the
    /// action it now stands for) or took it away from its action (then `None`). `None` also for a key the table
    /// doesn't know.
    pub fn game(&self, key: Key, mods: mouse::KeyMods) -> Option<(mouse::Key, mouse::KeyMods)> {
        let base = crate::game::game_key(key)?;
        if self.game.is_empty() {
            return Some((base, mods));
        }
        let pressed = Chord { key: base, mods };
        if let Some(c) = self.game.get(&pressed) {
            return Some((c.key, c.mods));
        }
        if self.unbound.contains(&pressed) {
            return None;
        }
        Some((base, mods))
    }

    /// Whether the idle-worker key is down: F6 with any modifiers, or the player's chord.
    pub fn idle_pressed(&self, key: Key, mods: mouse::KeyMods) -> bool {
        match self.idle {
            None => key == Key::F(crate::idle::KEY),
            Some(c) => crate::game::game_key(key).is_some_and(|k| Chord { key: k, mods } == c),
        }
    }

    /// Whether a quick-select key is down: `which` 0 for the units on screen, 1 for the map. Without a binding it is
    /// Ctrl+A or Alt+A exactly.
    pub fn select_pressed(&self, which: usize, key: Key, mods: mouse::KeyMods) -> bool {
        let want = self.select[which].unwrap_or(SELECT_DEFAULTS[which]);
        crate::game::game_key(key).is_some_and(|k| Chord { key: k, mods } == want)
    }

    /// The idle-worker key as the button shows it: `F6` or the player's chord.
    pub fn idle_label(&self) -> String {
        match self.idle {
            None => format!("F{}", crate::idle::KEY),
            Some(c) => c.text(),
        }
    }

    /// The panel slot a pressed key (with no Ctrl or Alt held) works: by slot binding, then each shown button by its
    /// own binding or, if it has none, the hotkey of its string-table entry. With no button bindings this is the
    /// original's search.
    pub fn button(&self, key: Key, slots: &[Option<&Entry>; 9], label: &dyn Fn(u16) -> Label) -> Option<usize> {
        if !self.has_buttons() {
            let hk = if key == Key::Escape {
                Hotkey::Escape
            } else {
                Hotkey::Char(crate::game::key_char(key)?)
            };
            return crate::panel::hotkey_slot(slots, label, hk);
        }
        if key == Key::Escape {
            return crate::panel::hotkey_slot(slots, label, Hotkey::Escape);
        }
        let c = crate::game::key_char(key)?;
        if let Some(n) = self.slots.iter().position(|s| *s == Some(c)) {
            return slots[n].map(|_| n);
        }
        slots.iter().position(|en| {
            en.is_some_and(
                |en| match button_name(en.act).and_then(|n| self.buttons.iter().find(|(b, _)| *b == n)) {
                    Some(&(_, bound)) => bound == c,
                    None => label(en.string).hotkey == Some(Hotkey::Char(c)),
                },
            )
        })
    }
}

/// A bare letter or digit of a button or slot binding, as the character `crate::game::key_char` gives.
fn char_of(c: Chord) -> Option<char> {
    match c.key {
        mouse::Key::Letter(l) => Some(l),
        mouse::Key::Digit(d) => Some((b'0' + d) as char),
        _ => None,
    }
}

/// Whether a chord has the shape its action takes.
fn check_form(bind: Bind, c: Chord) -> Result<(), String> {
    let any_mod = c.mods.ctrl || c.mods.alt || c.mods.shift;
    match bind {
        Bind::Slot(_) | Bind::Button(_) => {
            if any_mod || char_of(c).is_none() {
                return Err("a command button takes a bare letter or digit".into());
            }
        }
        Bind::Game(_) | Bind::Idle | Bind::Select(_) => {
            if matches!(c.key, mouse::Key::Letter(_)) && !(c.mods.ctrl || c.mods.alt) {
                return Err("a letter on its own belongs to the command buttons; add ctrl or alt".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(String::from).collect()
    }

    fn mods(ctrl: bool, alt: bool, shift: bool) -> mouse::KeyMods {
        mouse::KeyMods { shift, ctrl, alt }
    }

    const NONE: mouse::KeyMods = mouse::KeyMods {
        shift: false,
        ctrl: false,
        alt: false,
    };

    fn parse(text: &str) -> (KeyMap, Problems) {
        KeyMap::parse(&lines(text))
    }

    #[test]
    fn every_game_action_has_a_unique_name_and_a_default_chord() {
        let actions = game_actions();
        let names: HashSet<&String> = actions.iter().map(|(n, _)| n).collect();
        assert_eq!(names.len(), actions.len(), "names are unique");
        // Every variant of the table is reachable: 2 speeds, 30 group keys, 3 + 3 views, 17 others.
        assert_eq!(actions.len(), 2 + 30 + 6 + 17);
        let table = default_chords();
        for (_, a) in &actions {
            assert!(canonical(*a, &table).is_some());
        }
        // The names in the doc comment's list are the real ones.
        for n in [
            "speed-up",
            "pause",
            "group-recall-3",
            "group-show-0",
            "goto-view-3",
            "toggle-sounds",
            "quit-dialog",
        ] {
            assert!(actions.iter().any(|(x, _)| x == n), "{n}");
        }
    }

    #[test]
    fn an_empty_keymap_hands_every_key_on_unchanged() {
        let map = KeyMap::default();
        assert!(map.is_default());
        for key in [
            Key::Letter('P'),
            Key::Digit(3),
            Key::F(5),
            Key::Equal,
            Key::NumpadAdd,
            Key::Minus,
        ] {
            for m in mod_combos() {
                assert_eq!(
                    map.game(key, m),
                    crate::game::game_key(key).map(|k| (k, m)),
                    "{key:?} {m:?}"
                );
            }
        }
        assert!(map.idle_pressed(Key::F(6), NONE));
        assert!(
            map.idle_pressed(Key::F(6), mods(true, false, true)),
            "F6 with any modifiers, as before"
        );
        assert!(!map.idle_pressed(Key::F(7), NONE));
        assert_eq!(map.idle_label(), "F6");
        assert_eq!(
            map.section_text(),
            "",
            "nothing is written when the file had no section"
        );
    }

    #[test]
    fn the_default_chords_are_the_original_tables() {
        let t = default_chords();
        let find = |key, m| t.iter().find(|(c, _)| c.key == key && c.mods == m).map(|(_, a)| *a);
        assert_eq!(find(mouse::Key::Plus, NONE), Some(KeyAction::Speed(true)));
        assert_eq!(
            find(mouse::Key::Letter('P'), mods(true, false, false)),
            Some(KeyAction::Pause)
        );
        assert_eq!(
            find(mouse::Key::Digit(4), NONE),
            Some(KeyAction::Group(GroupKey::Recall, 4))
        );
        assert_eq!(
            find(mouse::Key::F(6), NONE),
            None,
            "F6 is the idle key's, not the table's"
        );
        assert_eq!(find(mouse::Key::F(10), NONE), None);
    }

    #[test]
    fn a_rebound_game_key_acts_and_the_old_one_does_not() {
        let (map, problems) = parse("pause=ctrl+u");
        assert!(problems.is_empty(), "{problems:?}");
        // Ctrl+U now gives what Ctrl+P (the action's own chord) gives to the key table.
        let (key, m) = map.game(Key::Letter('U'), mods(true, false, false)).unwrap();
        assert_eq!(mouse::key_action(key, m, false, true, false), Some(KeyAction::Pause));
        // Ctrl+P and Alt+P (the original's two) no longer pause.
        assert_eq!(map.game(Key::Letter('P'), mods(true, false, false)), None);
        assert_eq!(map.game(Key::Letter('P'), mods(false, true, false)), None);
        // Other keys are untouched.
        assert_eq!(
            map.game(Key::Letter('S'), mods(true, false, false)),
            Some((mouse::Key::Letter('S'), mods(true, false, false)))
        );
        assert!(!map.is_default());
    }

    #[test]
    fn two_actions_can_swap_keys() {
        let (map, problems) = parse("speed-up=minus\nspeed-down=equal");
        assert!(problems.is_empty(), "{problems:?}");
        let act = |k| {
            let (key, m) = map.game(k, NONE)?;
            mouse::key_action(key, m, false, true, false)
        };
        assert_eq!(act(Key::Minus), Some(KeyAction::Speed(true)));
        assert_eq!(act(Key::Equal), Some(KeyAction::Speed(false)));
        assert_eq!(
            act(Key::NumpadAdd),
            Some(KeyAction::Speed(false)),
            "the keypad's plus is the same key"
        );
    }

    #[test]
    fn a_group_digit_can_move_to_a_function_key() {
        let (map, problems) = parse("group-recall-1=f6\nidle-worker=ctrl+i");
        assert!(problems.is_empty(), "{problems:?}");
        let (key, m) = map.game(Key::F(6), NONE).unwrap();
        assert_eq!(
            mouse::key_action(key, m, false, true, false),
            Some(KeyAction::Group(GroupKey::Recall, 1))
        );
        assert_eq!(map.game(Key::Digit(1), NONE), None, "the digit gave up its group");
        assert!(map.game(Key::Digit(2), NONE).is_some());
        assert!(map.idle_pressed(Key::Letter('I'), mods(true, false, false)));
        assert!(!map.idle_pressed(Key::F(6), NONE), "F6 is the group's now");
        assert_eq!(map.idle_label(), "Ctrl+I");
    }

    #[test]
    fn conflicts_are_refused_with_a_message_naming_both() {
        // Ctrl+S is toggle-sounds; Alt+S is save-game; F5 is options; F6 is the idle key.
        let (map, problems) = parse("pause=ctrl+s\nspeed-up=f5\ngroup-set-2=f6\ngoto-view-1=alt+k");
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(
            problems[0].contains("pause=Ctrl+S") && problems[0].contains("toggle-sounds"),
            "{}",
            problems[0]
        );
        assert!(problems[1].contains("options"), "{}", problems[1]);
        assert!(problems[2].contains("idle-worker"), "{}", problems[2]);
        assert!(problems[3].contains("keyboard-help"), "{}", problems[3]);
        assert!(map.is_default(), "nothing was taken");
    }

    #[test]
    fn the_settings_key_cannot_be_given_to_an_action() {
        let (map, problems) = parse("pause=ctrl+g");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("Ctrl+G") && problems[0].contains("settings"),
            "{}",
            problems[0]
        );
        assert!(map.is_default());
        // Alt+G is free.
        assert!(parse("pause=alt+g").1.is_empty());
    }

    #[test]
    fn two_lines_with_one_key_keep_the_first() {
        let (map, problems) = parse("pause=ctrl+u\ntips=ctrl+u");
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].contains("tips=Ctrl+U") && problems[0].contains("pause"),
            "{}",
            problems[0]
        );
        assert!(map.game(Key::Letter('U'), mods(true, false, false)).is_some());
        let (_, problems) = parse("pause=ctrl+u\npause=ctrl+y");
        assert!(problems[0].contains("set twice"), "{problems:?}");
    }

    #[test]
    fn bad_lines_say_what_is_wrong_and_are_kept_in_the_file() {
        let text = "# my keys\nbogus=ctrl+a\npause=ctrl+nope\npause=\nspeed-up=escape\ntips=f10\nquit=p\nno equals sign\nrestart=ctrl+q+";
        let (map, problems) = parse(text);
        assert_eq!(problems.len(), 8, "{problems:?}");
        assert!(problems[0].contains("not an action name"));
        assert!(problems[1].contains("not a key name"));
        assert!(problems[3].contains("stays as it is"));
        assert!(problems[4].contains("F10"));
        assert!(problems[5].contains("command buttons"), "{}", problems[5]);
        assert!(problems[6].contains("not name=key"));
        assert!(problems[7].contains("equal"), "{}", problems[7]);
        assert!(map.is_default());
        // Every line, comments included, goes back to the file.
        assert_eq!(map.section_text(), format!("[keys]\n{text}\n"));
    }

    #[test]
    fn button_keys_default_to_the_hotkey_in_the_entry() {
        use crate::panel::{Cond, Entry};
        let entry = |slot, string, act| Entry {
            slot,
            icon: 0,
            string,
            cond: Cond::Always,
            act,
        };
        let stop = entry(1, 11, Act::Stop);
        let attack = entry(2, 12, Act::Target(Target::Attack));
        let train = entry(0, 13, Act::Train(0));
        let slots = [
            Some(&train),
            Some(&stop),
            Some(&attack),
            None,
            None,
            None,
            None,
            None,
            None,
        ];
        let label = |n: u16| Label {
            hotkey: Some(Hotkey::Char(match n {
                11 => 'S',
                12 => 'A',
                _ => 'F',
            })),
            cost_kind: 0,
            text: String::new(),
        };
        let none = KeyMap::default();
        assert_eq!(none.button(Key::Letter('S'), &slots, &label), Some(1));
        assert_eq!(none.button(Key::Letter('A'), &slots, &label), Some(2));
        assert_eq!(none.button(Key::Letter('Z'), &slots, &label), None);

        // Stop moves to X (S is no longer its key), Attack stays on A; the third slot gets Q.
        let (map, problems) = parse("button-stop=x\nslot-1=q\nslot-9=k");
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(map.button(Key::Letter('X'), &slots, &label), Some(1));
        assert_eq!(map.button(Key::Letter('S'), &slots, &label), None);
        assert_eq!(map.button(Key::Letter('A'), &slots, &label), Some(2));
        assert_eq!(map.button(Key::Letter('Q'), &slots, &label), Some(0), "slot 1");
        assert_eq!(
            map.button(Key::Letter('K'), &slots, &label),
            None,
            "slot 9 is empty here"
        );
    }

    #[test]
    fn a_slot_key_beats_a_buttons_own_letter() {
        use crate::panel::{Cond, Entry};
        let stop = Entry {
            slot: 1,
            icon: 0,
            string: 11,
            cond: Cond::Always,
            act: Act::Stop,
        };
        let move_ = Entry {
            slot: 0,
            icon: 0,
            string: 12,
            cond: Cond::Always,
            act: Act::Target(Target::Move),
        };
        let slots = [Some(&move_), Some(&stop), None, None, None, None, None, None, None];
        let label = |n: u16| Label {
            hotkey: Some(Hotkey::Char(if n == 11 { 'S' } else { 'M' })),
            cost_kind: 0,
            text: String::new(),
        };
        let (map, _) = parse("slot-1=s");
        assert_eq!(map.button(Key::Letter('S'), &slots, &label), Some(0));
        assert_eq!(map.button(Key::Escape, &slots, &label), None);
    }

    #[test]
    fn button_and_slot_keys_conflict_with_the_tables_digits_and_each_other() {
        let (_, problems) = parse("button-stop=3\nslot-1=x\nbutton-move=x\nbutton-attack=ctrl+a\nbutton-patrol=f2");
        assert_eq!(problems.len(), 4, "{problems:?}");
        let has = |t: &str| problems.iter().any(|p| p.contains(t));
        assert!(has("button-stop=3") && has("group-recall-3"), "{problems:?}");
        assert!(has("button-move=X") && has("slot-1"), "{problems:?}");
        assert!(has("button-attack=ctrl+a") && has("button-patrol=f2"), "{problems:?}");
    }

    #[test]
    fn the_keys_page_lists_every_action_the_file_reads_once() {
        let rows = rows();
        let names: HashSet<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names.len(), rows.len(), "unique");
        assert_eq!(
            rows.len(),
            game_actions().len() + 1 + SELECT_NAMES.len() + BUTTONS.len() + 9
        );
        let actions = game_actions();
        for r in &rows {
            assert!(
                bind_by_name(&r.name, &actions).is_some(),
                "{} is read by the file",
                r.name
            );
            assert!(!r.label.is_empty() && r.label != r.name, "{}", r.name);
            assert!(!r.default.is_empty() && !r.original.is_empty());
        }
        for (n, _) in &actions {
            assert!(names.contains(n.as_str()), "{n} is listed");
        }
        assert!(names.contains("idle-worker") && names.contains("slot-9") && names.contains("button-stop"));
        let by = |n: &str| rows.iter().find(|r| r.name == n).unwrap();
        assert_eq!(
            (by("pause").default.as_str(), by("pause").original.as_str()),
            ("Ctrl+P", "Ctrl+P or Alt+P")
        );
        assert_eq!(by("idle-worker").default, "F6");
        assert_eq!(by("group-recall-3").default, "3");
        assert_eq!(by("slot-1").default, "-");
        assert_eq!(by("button-move").label, "Command: Move");
        assert_eq!(by("goto-view-2").label, "Go to view 2");
    }

    #[test]
    fn a_chord_prints_as_the_file_reads_it() {
        for key in key_universe() {
            for mods in mod_combos() {
                let c = Chord { key, mods };
                match parse_chord(&c.file_text()) {
                    Ok(back) => assert_eq!(back, c, "{}", c.file_text()),
                    Err(e) => assert!(matches!(key, mouse::Key::F(10)), "{}: {e}", c.file_text()),
                }
            }
        }
        let c = Chord::pressed(Key::Letter('P'), mods(true, true, false)).unwrap();
        assert_eq!(
            (c.file_text().as_str(), c.text().as_str()),
            ("ctrl+alt+p", "Ctrl+Alt+P")
        );
        assert_eq!(Chord::pressed(Key::NumpadAdd, NONE).unwrap().file_text(), "equal");
        assert_eq!(Chord::pressed(Key::Minus, NONE).unwrap().file_text(), "minus");
        assert_eq!(Chord::pressed(Key::Up, NONE), None);
        assert_eq!(Chord::pressed(Key::Enter, NONE), None);
    }

    #[test]
    fn binding_a_key_edits_the_lines_by_the_files_own_rules() {
        let ctrl = |c: char| Chord::pressed(Key::Letter(c), mods(true, false, false)).unwrap();
        let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        // Bound; the comment stays and the action's earlier line is replaced, not repeated.
        let l = with_binding(&lines(&["# mine", "pause=ctrl+u"]), "pause", ctrl('Y')).unwrap();
        assert_eq!(l, ["# mine", "pause=ctrl+y"]);
        let (map, problems) = KeyMap::parse(&l);
        assert!(problems.is_empty());
        assert_eq!(map.chord_of("pause"), Some(ctrl('Y')));
        assert_eq!(map.chord_of("tips"), None);
        // Taken by an action that was not rebound, by another rebound one, and the settings key.
        let e = with_binding(&[], "pause", ctrl('S')).unwrap_err();
        assert!(e.contains("Ctrl+S") && e.contains("Sound effects on or off"), "{e}");
        let e = with_binding(&lines(&["tips=ctrl+y"]), "pause", ctrl('Y')).unwrap_err();
        assert!(e.contains("Tips"), "{e}");
        let e = with_binding(&[], "pause", ctrl('G')).unwrap_err();
        assert!(e.contains("settings"), "{e}");
        // The shape.
        let bare = Chord::pressed(Key::Letter('U'), NONE).unwrap();
        let e = with_binding(&[], "pause", bare).unwrap_err();
        assert!(e.starts_with("A letter on its own") && e.ends_with('.'), "{e}");
        // A refused line that is already in the file does not hide a new refusal of the same text.
        let e = with_binding(&lines(&["pause=ctrl+s"]), "pause", ctrl('S')).unwrap_err();
        assert!(e.contains("already used"), "{e}");
        // Rebinding frees the original keys, so a key the old binding had is free again for another action.
        let l = with_binding(&[], "toggle-sounds", ctrl('U')).unwrap();
        assert!(with_binding(&l, "pause", ctrl('S')).is_ok());
        // Removing one and resetting all.
        let all = lines(&["# mine", "pause=ctrl+u", "tips=ctrl+y", "typo=ctrl+u", "bogus"]);
        assert_eq!(
            without_binding(&all, "pause"),
            ["# mine", "tips=ctrl+y", "typo=ctrl+u", "bogus"]
        );
        assert_eq!(reset_lines(&all), ["# mine", "typo=ctrl+u", "bogus"]);
        assert!(KeyMap::parse(&reset_lines(&all)).0.is_default());
    }

    #[test]
    fn chords_read_and_print() {
        let c = parse_chord("Ctrl + Alt+f7").unwrap();
        assert_eq!(c.text(), "Ctrl+Alt+F7");
        assert_eq!(parse_chord("shift+3").unwrap().text(), "Shift+3");
        assert!(parse_chord("hyper+x").is_err());
        assert!(parse_chord("").is_err());
    }
}
