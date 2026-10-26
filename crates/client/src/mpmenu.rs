//! The multiplayer menus, from the main menu's Multi Player Game to the game setup lobby (spec
//! `docs/specs/ui/multiplayer-menus.md`): the Enhanced or Classic choice, the connection method list, the Direct IP
//! setup screen, name entry, the connecting box and its error box, the game list and the join wait. SDL-free and
//! engine-free: `frontend` opens the screens, feeds them the frame's input ([`Input`]) and shows what [`Mp::draw`]
//! gives, like [`customsetup`](crate::customsetup).
//!
//! The dialog files (`rez\multmode.bin` and the rest) aren't read; every layout is the spec's, in screen pixels.
//!
//! Where the spec leaves a gap (each marked `// spec gap:` where used):
//! - **Tidewater's rows** are the spec's "Tidewater slots" section: Battle.net drawn disabled, Direct IP in Direct
//!   Connect's place and Local Network in IPX's; no Modem row.
//! - **Direct IP setup** replaces COM and Baud with an address box and a port box in their places.
//! - **The panels** of the small dialogs (`Art\o144x64.bmp`) aren't read; ours are black with the two rings.
//! - **The join wait** and the game list's scroll bar are drawn plainly; the list scrolls with the wheel and the keys.

use std::net::TcpListener;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use crate::customsetup::{DropState, Input, Look};
use crate::hud::{self, Rect};
use crate::input::Key;
use crate::menuscreen::{self, ButtonState, FocusKey, GOLD, GREY, WHITE};
use crate::mpwidgets::{Charset, EditBox, Popup, PopupKey, Ptr, draw_edit, draw_panel, draw_popup};
use crate::net::beacon::{self, Finder};
use crate::net::proto::{DEFAULT_PORT, Identity};
use crate::net::room::Handshake;
use crate::scene::Picture;

/// How long a join waits for the creator before "Unable to join - timed out".
pub(crate) const JOIN_TIMEOUT: Duration = Duration::from_secs(10);

/// A connection method of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Method {
    /// Battle.net: drawn disabled, it cannot be chosen.
    Battle,
    /// Direct IP, in Direct Connect's place.
    DirectIp,
    /// Local Network, in IPX's place.
    Local,
}

impl Method {
    /// The list's rows, top to bottom: Enhanced shows Battle.net (disabled) first.
    pub(crate) fn rows(enhanced: bool) -> &'static [Method] {
        if enhanced {
            &[Method::Battle, Method::DirectIp, Method::Local]
        } else {
            &[Method::DirectIp, Method::Local]
        }
    }

    fn label(self) -> &'static str {
        match self {
            Method::Battle => "Battle.net",
            Method::DirectIp => "Direct IP",
            Method::Local => "Local Network",
        }
    }

    /// The description under the list: its lines.
    fn description(self) -> Vec<&'static str> {
        match self {
            Method::Battle => vec!["Battle.net is not available."],
            // spec gap: the description of Direct IP is ours (the spec gives Local Network's only).
            Method::DirectIp => vec![
                "Players supported: 8",
                "Requirements: The creator's address, and its port open to the other players.",
            ],
            Method::Local => vec![
                "Players supported: 8",
                "Requirements: All computers must be on the same local network.",
            ],
        }
    }

    fn enabled(self) -> bool {
        self != Method::Battle
    }
}

/// What the screens remember between visits (the player name, the selected row, the Mode and the address): the
/// frontend keeps it for the run and the preferences keep it between runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Memory {
    /// The player's name; empty until one is entered (the box then opens on "Anonymous").
    pub name: String,
    pub method: Method,
    /// The Direct IP setup's Mode: Answer rather than Call.
    pub answer: bool,
    pub address: String,
    pub port: String,
}

impl Default for Memory {
    fn default() -> Self {
        Memory {
            name: String::new(),
            method: Method::DirectIp,
            answer: false,
            address: String::new(),
            port: DEFAULT_PORT.to_string(),
        }
    }
}

impl Memory {
    /// What the stored settings remember (`Prefs::mp_name` and the rest).
    pub(crate) fn from_prefs(p: &crate::prefs::Prefs) -> Memory {
        Memory {
            name: p.mp_name.clone(),
            method: match p.mp_method {
                0 => Method::Battle,
                2 => Method::Local,
                _ => Method::DirectIp,
            },
            answer: p.mp_answer,
            address: p.mp_address.clone(),
            port: p.mp_port.clone(),
        }
    }

    /// Puts this memory into the stored settings.
    pub(crate) fn store(&self, p: &mut crate::prefs::Prefs) {
        p.mp_name.clone_from(&self.name);
        p.mp_method = match self.method {
            Method::Battle => 0,
            Method::DirectIp => 1,
            Method::Local => 2,
        };
        p.mp_answer = self.answer;
        p.mp_address.clone_from(&self.address);
        p.mp_port.clone_from(&self.port);
    }
}

/// The table strings the screens show, from `network.tbl` where the install has them.
#[derive(Clone, Debug)]
pub(crate) struct Strings {
    pub anonymous: String,
    pub establishing: String,
    pub contacting: String,
    pub timed_out: String,
}

impl Strings {
    /// Entries 82, 145, 146, 84 and 85 through `entry`, with the spec's words where the table gives none.
    pub(crate) fn from(entry: &dyn Fn(u16) -> String) -> Strings {
        let get = |n: u16, fallback: &str| {
            let s = entry(n);
            if s.is_empty() { fallback.to_string() } else { s }
        };
        Strings {
            anonymous: get(82, "Anonymous"),
            establishing: get(145, "Establishing connection..."),
            contacting: get(146, "Contacting remote computer..."),
            timed_out: get(84, "Unable to join - timed out"),
        }
    }
}

impl Default for Strings {
    fn default() -> Self {
        Strings::from(&|_| String::new())
    }
}

/// The beacon port the menus listen on: the real one, unless tests or scripted runs (`TIDEWATER_NO_BEACON`) ask for
/// none.
fn default_beacon_port() -> Option<u16> {
    if cfg!(test) || std::env::var_os("TIDEWATER_NO_BEACON").is_some() {
        None
    } else {
        Some(beacon::PORT)
    }
}

/// A game on the game list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListedGame {
    /// Where to join it: `HOST:PORT`.
    pub addr: String,
    /// The creator's name.
    pub owner: String,
    pub players: u8,
    pub max: u8,
    pub map: String,
    /// How many observers can still join: `None` when the list cannot tell (a Direct IP game is only known by its
    /// address; the creator answers a Watch Game with a refusal if it has no place), `Some(0)` when it allows none.
    pub watch: Option<u8>,
}

/// What a frame of the screens asks of the program.
#[derive(Debug)]
pub(crate) enum Outcome {
    Stay,
    /// Previous Menu, or Disconnect: back to the main menu.
    Leave,
    /// Create Game (or Direct IP's Call): the lobby opens as the creator's, on this socket.
    Host(TcpListener),
    /// The creator has welcomed this machine: if it has the map, the lobby opens as a joiner's.
    Join(Box<Handshake>),
}

impl PartialEq for Outcome {
    fn eq(&self, other: &Outcome) -> bool {
        matches!(
            (self, other),
            (Outcome::Stay, Outcome::Stay) | (Outcome::Leave, Outcome::Leave)
        )
    }
}

/// The screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Choice,
    Methods,
    Direct,
    Games,
    Waiting,
}

/// What name entry leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterName {
    /// Classic: the connection list follows.
    List,
    /// Enhanced: the connection is made.
    Connect,
}

/// A connection being made: the connecting box shows it.
struct Conn {
    started: Instant,
    rx: Mutex<Receiver<Result<(), String>>>,
    /// Cancel was pressed: the box says so and closes when the attempt ends.
    canceling: bool,
}

enum Overlay {
    None,
    Name(AfterName),
    Connecting(Conn),
    Notice(String),
}

/// A control the pointer or the focus can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum T {
    Enhanced,
    Classic,
    Previous,
    List,
    Row(usize),
    Connect,
    Address,
    Mode,
    Port,
    PopRow(usize),
    NameBox,
    Ok,
    Cancel,
    NoticeOk,
    GameList,
    GameRow(usize),
    Join,
    /// Watch Game: our own addition, offered only for a game that may have a place for an observer.
    Watch,
    Create,
    Disconnect,
    WaitCancel,
}

// The screens' layout: the spec's coordinates.
const CHOICE: [(T, i32, &str, char); 3] = [
    (T::Enhanced, 276, "Enhanced", 'E'),
    (T::Classic, 312, "Classic", 'C'),
    (T::Previous, 348, "Previous Menu", 'P'),
];
const CHOICE_X: i32 = 208;
const METHOD_HEADING: (i32, i32) = (62, 168);
const METHOD_LIST: Rect = Rect::new(60, 196, 240, 176);
const METHOD_ROW_H: i32 = 18;
const METHOD_DESCRIPTION: (i32, i32) = (80, 380);
const METHOD_WIDTH: u32 = 500;
const METHOD_BUTTONS: [(T, i32, &str, char); 2] = [
    (T::Connect, 302, "Connect", 'C'),
    (T::Previous, 336, "Previous Menu", 'P'),
];
const METHOD_BUTTON_X: i32 = 360;
const SETUP_HEADING: Rect = Rect::new(106, 206, 400, 18);
const ADDRESS_LABEL: (i32, i32) = (20, 336);
const ADDRESS_BOX: Rect = Rect::new(90, 330, 296, 24);
const MODE_LABEL: (i32, i32) = (20, 370);
const MODE_BOX: Rect = Rect::new(90, 364, 96, 24);
const PORT_LABEL: (i32, i32) = (218, 370);
const PORT_BOX: Rect = Rect::new(288, 364, 96, 24);
const SETUP_BUTTONS: [(T, i32, &str, char); 2] = [
    (T::Connect, 336, "Connect", 'C'),
    (T::Previous, 376, "Previous Menu", 'P'),
];
const SETUP_BUTTON_X: i32 = 406;
const SMALL_PANEL: Rect = Rect::new(176, 256, 288, 128);
const NAME_LABEL: (i32, i32) = (184, 268);
const NAME_BOX: Rect = Rect::new(214, 294, 190, 18);
const NAME_OK: Rect = Rect::new(198, 340, 106, 28);
const NAME_CANCEL: Rect = Rect::new(332, 340, 106, 28);
const BOX_TEXT: (i32, i32) = (188, 264);
const BOX_CANCEL: Rect = Rect::new(268, 348, 106, 28);
const GAMES_HEADING: (i32, i32) = (38, 186);
const GAMES_LIST: Rect = Rect::new(34, 210, 240, 142);
const GAMES_LINE_H: i32 = 20;
const GAMES_LINES: usize = 7;
const GAMES_INFO: (i32, i32) = (34, 368);
const WAITING_LABEL: (i32, i32) = (390, 230);
const GAMES_BUTTONS: [(T, i32, &str, char); 3] = [
    (T::Join, 262, "Join Game", 'J'),
    (T::Create, 294, "Create Game", 'C'),
    (T::Disconnect, 326, "Disconnect", 'D'),
];
const GAMES_BUTTON_X: i32 = 378;
/// Our own button under the original's three, shown only when the selected game may take an observer.
const WATCH_BUTTON: (T, i32, &str, char) = (T::Watch, 358, "Watch Game", 'W');
const WAIT_PANEL: Rect = Rect::new(40, 316, 540, 148);
const WAIT_TEXT: (i32, i32) = (52, 332);
const WAIT_CANCEL: Rect = Rect::new(198, 420, 224, 28);

fn button_rect(x: i32, y: i32) -> Rect {
    Rect::new(x, y, menuscreen::BUTTON_W, menuscreen::BUTTON_H)
}

/// The multiplayer menus' state.
pub(crate) struct Mp {
    stage: Stage,
    overlay: Overlay,
    /// The rules chosen: Enhanced (the default) or Classic mode (opt-in).
    enhanced: bool,
    pub mem: Memory,
    focus: T,
    ptr: Ptr<T>,
    name: EditBox,
    address: EditBox,
    port: EditBox,
    popup: Option<Popup>,
    games: Vec<ListedGame>,
    /// Listens for the beacons that fill the Local Network game list, while that list shows.
    finder: Option<Finder>,
    /// The UDP port the beacons use (none: no listening, as in tests and scripted runs).
    pub(crate) beacon_port: Option<u16>,
    game_sel: usize,
    game_top: usize,
    /// The attempt a join makes, while the wait shows.
    join: Option<Join>,
    /// What the creator of a game checks a joiner against: this program's identity.
    identity: Identity,
    strings: Strings,
    changed: bool,
}

impl Mp {
    /// The screens as Multi Player Game opens them: the choice, Enhanced focused. The rules are asked every time;
    /// `mem` is what the last visit left.
    pub(crate) fn new(mem: Memory, strings: Strings) -> Mp {
        let name = EditBox::new("", 11, Charset::Name);
        Mp {
            stage: Stage::Choice,
            overlay: Overlay::None,
            enhanced: true,
            focus: T::Enhanced,
            ptr: Ptr::default(),
            address: EditBox::new(&mem.address, 63, Charset::Address),
            port: EditBox::new(&mem.port, 5, Charset::Digits),
            name,
            mem,
            popup: None,
            games: Vec::new(),
            finder: None,
            beacon_port: default_beacon_port(),
            game_sel: 0,
            game_top: 0,
            join: None,
            identity: Identity::ours(0),
            strings,
            changed: true,
        }
    }

    /// Sets the identity a join says hello with (the installation's data decides it).
    pub(crate) fn with_identity(mut self, identity: Identity) -> Mp {
        self.identity = identity;
        self
    }

    /// The rules chosen: Enhanced (the default) or Classic mode.
    pub(crate) fn enhanced(&self) -> bool {
        self.enhanced
    }

    /// The lobby has closed: back to the screen it was opened from, with the reason when there is one.
    pub(crate) fn lobby_closed(&mut self, notice: Option<String>) {
        self.join = None;
        self.overlay = Overlay::None;
        if self.mem.method == Method::Local || (self.mem.method == Method::DirectIp && self.mem.answer) {
            self.go(Stage::Games, T::GameList);
            self.watch_lan();
        } else {
            self.go(Stage::Methods, T::List);
        }
        if let Some(text) = notice {
            self.overlay = Overlay::Notice(text);
            self.focus = T::NoticeOk;
        }
    }

    /// How many games the game list shows (a scripted run waits on it).
    pub(crate) fn listed_games(&self) -> usize {
        if self.stage == Stage::Games {
            self.games.len()
        } else {
            0
        }
    }

    /// Whether the picture needs drawing again, and forget it does.
    pub(crate) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Starts listening for the beacons of Local Network games (when this method is Local Network and the list shows).
    fn watch_lan(&mut self) {
        self.finder = match (self.mem.method, self.beacon_port) {
            (Method::Local, Some(port)) => Finder::bind(port).ok(),
            _ => None,
        };
    }

    /// Whether this machine's games are announced on the network: the method is Local Network.
    pub(crate) fn lan(&self) -> bool {
        self.mem.method == Method::Local
    }

    /// Takes the beacons heard into the game list, keeping the selection on the same game.
    fn poll_finder(&mut self) {
        let Some(f) = &mut self.finder else { return };
        if self.stage != Stage::Games {
            return;
        }
        let list: Vec<ListedGame> = f
            .poll(Instant::now())
            .into_iter()
            .map(|g| ListedGame {
                addr: g.addr,
                owner: g.announce.owner,
                players: g.announce.players,
                max: g.announce.max,
                map: g.announce.map,
                watch: Some(g.announce.watch),
            })
            .collect();
        if list != self.games {
            let was = self.games.get(self.game_sel).map(|g| g.addr.clone());
            self.games = list;
            self.game_sel = was
                .and_then(|a| self.games.iter().position(|g| g.addr == a))
                .unwrap_or(0)
                .min(self.games.len().saturating_sub(1));
            self.game_top = self.game_top.min(self.games.len().saturating_sub(GAMES_LINES));
            self.changed = true;
        }
    }

    fn rows(&self) -> &'static [Method] {
        Method::rows(self.enhanced)
    }

    fn selected_row(&self) -> usize {
        self.rows().iter().position(|&m| m == self.mem.method).unwrap_or(0)
    }

    // --- Layout ------------------------------------------------------------------------------------------------

    fn buttons(&self) -> Vec<(T, Rect, &'static str, char, bool)> {
        let mk = |list: &[(T, i32, &'static str, char)], x: i32| {
            list.iter()
                .map(|&(t, y, label, key)| (t, button_rect(x, y), label, key, true))
                .collect::<Vec<_>>()
        };
        match self.stage {
            Stage::Choice => mk(&CHOICE, CHOICE_X),
            Stage::Methods => {
                let mut v = mk(&METHOD_BUTTONS, METHOD_BUTTON_X);
                v[0].4 = self.mem.method.enabled();
                v
            }
            Stage::Direct => mk(&SETUP_BUTTONS, SETUP_BUTTON_X),
            Stage::Games => {
                let mut v = mk(&GAMES_BUTTONS, GAMES_BUTTON_X);
                v[0].4 = !self.games.is_empty();
                if self.watch_offered() {
                    v.extend(mk(&[WATCH_BUTTON], GAMES_BUTTON_X));
                }
                v
            }
            Stage::Waiting => vec![(T::WaitCancel, WAIT_CANCEL, "Cancel", '\0', true)],
        }
    }

    /// Whether Watch Game shows: the selected game may have a place for an observer.
    pub(crate) fn watch_offered(&self) -> bool {
        self.games.get(self.game_sel).is_some_and(|g| g.watch != Some(0))
    }

    fn game_row_rect(&self, i: usize) -> Rect {
        Rect::new(
            GAMES_LIST.x + 1,
            GAMES_LIST.y + 1 + (i - self.game_top) as i32 * GAMES_LINE_H,
            GAMES_LIST.w - 2,
            GAMES_LINE_H as u32,
        )
    }

    /// The overlay's buttons: its text box and the buttons under it.
    fn overlay_targets(&self) -> Vec<(T, Rect)> {
        match &self.overlay {
            Overlay::None => Vec::new(),
            Overlay::Name(_) => vec![(T::Ok, NAME_OK), (T::Cancel, NAME_CANCEL), (T::NameBox, NAME_BOX)],
            Overlay::Connecting(_) => vec![(T::Cancel, BOX_CANCEL)],
            Overlay::Notice(_) => vec![(T::NoticeOk, BOX_CANCEL)],
        }
    }

    /// The targets of the layer that takes the pointer, topmost first.
    fn targets(&self) -> Vec<(T, Rect)> {
        let mut v = Vec::new();
        if let Some(p) = &self.popup {
            v.extend((0..p.labels.len()).map(|i| (T::PopRow(i), p.row_rect(i))));
            return v;
        }
        if !matches!(self.overlay, Overlay::None) {
            return self.overlay_targets();
        }
        for (t, r, _, _, on) in self.buttons() {
            if on {
                v.push((t, r));
            }
        }
        match self.stage {
            Stage::Methods => {
                for i in 0..self.rows().len() {
                    v.push((
                        T::Row(i),
                        Rect::new(
                            METHOD_LIST.x + 1,
                            METHOD_LIST.y + 1 + i as i32 * METHOD_ROW_H,
                            METHOD_LIST.w - 2,
                            METHOD_ROW_H as u32,
                        ),
                    ));
                }
            }
            Stage::Direct => {
                v.push((T::Address, ADDRESS_BOX));
                v.push((T::Mode, MODE_BOX));
                v.push((T::Port, PORT_BOX));
            }
            Stage::Games => {
                for i in self.game_top..self.games.len().min(self.game_top + GAMES_LINES) {
                    v.push((T::GameRow(i), self.game_row_rect(i)));
                }
            }
            _ => {}
        }
        v
    }

    /// The Tab stops of the layer that has the focus.
    fn stops(&self) -> Vec<T> {
        match &self.overlay {
            Overlay::Name(_) => return vec![T::Ok, T::Cancel, T::NameBox],
            Overlay::Connecting(_) => return vec![T::Cancel],
            Overlay::Notice(_) => return vec![T::NoticeOk],
            Overlay::None => {}
        }
        match self.stage {
            Stage::Choice => vec![T::Enhanced, T::Classic, T::Previous],
            Stage::Methods => {
                if self.mem.method.enabled() {
                    vec![T::List, T::Connect, T::Previous]
                } else {
                    vec![T::List, T::Previous]
                }
            }
            Stage::Direct => vec![T::Address, T::Mode, T::Port, T::Connect, T::Previous],
            Stage::Games => {
                let mut v = vec![T::GameList];
                if !self.games.is_empty() {
                    v.push(T::Join);
                }
                if self.watch_offered() {
                    v.push(T::Watch);
                }
                v.extend([T::Create, T::Disconnect]);
                v
            }
            Stage::Waiting => vec![T::WaitCancel],
        }
    }

    // --- Input -------------------------------------------------------------------------------------------------

    /// One frame: the pointer, the left button, the keys and what the background work has done.
    pub(crate) fn update(&mut self, input: &Input) -> Outcome {
        let outcome = self.step(input);
        if outcome != Outcome::Stay {
            // Leaving, or the lobby opening: nothing listens for games any more.
            self.finder = None;
        }
        outcome
    }

    fn step(&mut self, input: &Input) -> Outcome {
        if let Some(o) = self.poll_background() {
            return o;
        }
        self.poll_finder();
        let targets = self.targets();
        let ev = self.ptr.update(&targets, input);
        if ev.changed {
            self.changed = true;
        }
        if let Some(t) = ev.down {
            // Pressing a control gives it the focus; a press outside an open list closes it.
            if self.popup.is_some() && !matches!(t, T::PopRow(_)) {
                self.popup = None;
            }
            if self.stops().contains(&t) {
                self.focus = t;
            }
        } else if input.down && self.popup.is_some() {
            self.popup = None;
            self.ptr.reset();
            self.changed = true;
        }
        if let Some(t) = ev.clicked {
            self.changed = true;
            if let Some(o) = self.activate(t) {
                return o;
            }
        }
        if input.wheel != 0 && self.stage == Stage::Games && matches!(self.overlay, Overlay::None) {
            self.game_top = (self.game_top as i32 - input.wheel)
                .clamp(0, self.games.len().saturating_sub(GAMES_LINES) as i32) as usize;
            self.changed = true;
        }
        self.keys(input)
    }

    fn keys(&mut self, input: &Input) -> Outcome {
        let mut pad = input.pad_focus;
        // A text box takes the typed events; every other control takes the keys that went down.
        if self.popup.is_none() && self.text_focused() {
            for e in &input.typed {
                self.changed = true;
                if e.pressed && self.pad_step(&mut pad, e.key) {
                    continue;
                }
                let key = match self.focused_box() {
                    Some(b) => b.feed(e, input.command_held),
                    None => None,
                };
                if let Some(k) = key
                    && let Some(o) = self.key(k, input.shift)
                {
                    return o;
                }
            }
            return Outcome::Stay;
        }
        for &k in &input.keys {
            self.changed = true;
            if self.pad_step(&mut pad, k) {
                continue;
            }
            if let Some(o) = self.key(k, input.shift) {
                return o;
            }
        }
        Outcome::Stay
    }

    /// A pad's d-pad up or down moves the focus, as Tab and Shift+Tab do, except in an open pop-up and in the game
    /// list, where it moves the line as the arrow keys do.
    fn pad_step(&mut self, pad: &mut Option<FocusKey>, k: Key) -> bool {
        if self.popup.is_some() || matches!(self.focus, T::GameList | T::GameRow(_)) {
            return false;
        }
        let Some(step) = menuscreen::pad_step(pad, k) else {
            return false;
        };
        self.focus = menuscreen::step_focus(&self.stops(), self.focus, step);
        true
    }

    fn text_focused(&self) -> bool {
        match &self.overlay {
            // The caret is in the name box whichever button has the ring (traced).
            Overlay::Name(_) => true,
            Overlay::None => self.stage == Stage::Direct && matches!(self.focus, T::Address | T::Port),
            _ => false,
        }
    }

    fn focused_box(&mut self) -> Option<&mut EditBox> {
        match (&self.overlay, self.focus) {
            (Overlay::Name(_), _) => Some(&mut self.name),
            (Overlay::None, T::Address) => Some(&mut self.address),
            (Overlay::None, T::Port) => Some(&mut self.port),
            _ => None,
        }
    }

    fn key(&mut self, k: Key, shift: bool) -> Option<Outcome> {
        if let Some(p) = &mut self.popup {
            match p.key(k) {
                PopupKey::Open => {}
                PopupKey::Closed => self.popup = None,
                PopupKey::Picked(i) => self.pick_popup(i),
            }
            return None;
        }
        let stops = self.stops();
        let back = |s: bool| if s { FocusKey::Back } else { FocusKey::Next };
        match k {
            Key::Tab => {
                self.focus = menuscreen::step_focus(&stops, self.focus, back(shift));
                None
            }
            Key::Escape => self.escape(),
            Key::Enter | Key::NumpadEnter => self.enter(),
            Key::Space => match self.focus {
                T::Mode => self.activate(T::Mode),
                T::NameBox | T::Address | T::Port => None,
                _ => self.activate(self.focus),
            },
            Key::Letter(c) => {
                let c = c.to_ascii_uppercase();
                if matches!(self.overlay, Overlay::None) {
                    let hit = self
                        .buttons()
                        .into_iter()
                        .find(|b| b.4 && b.3.to_ascii_uppercase() == c)
                        .map(|b| b.0);
                    if let Some(t) = hit {
                        self.focus = t;
                        return self.activate(t);
                    }
                } else {
                    let hit = match (&self.overlay, c) {
                        (Overlay::Connecting(_), 'C') => Some(T::Cancel),
                        (Overlay::Notice(_), 'O') => Some(T::NoticeOk),
                        _ => None,
                    };
                    if let Some(t) = hit {
                        return self.activate(t);
                    }
                }
                None
            }
            Key::Up | Key::Down | Key::Home | Key::End | Key::PageUp | Key::PageDown => {
                // spec gap: the arrow keys do nothing on the menu screens (`ui/screen-and-cursors.md`; the spec's
                // trace request MM8 is open), except in the game list, which scrolls.
                if self.stage == Stage::Games && matches!(self.overlay, Overlay::None) {
                    self.move_game(k);
                }
                None
            }
            _ => None,
        }
    }

    /// Esc presses the screen's Previous Menu or Cancel.
    fn escape(&mut self) -> Option<Outcome> {
        match &self.overlay {
            Overlay::Name(_) => return self.activate(T::Cancel),
            Overlay::Connecting(_) => return self.activate(T::Cancel),
            Overlay::Notice(_) => return self.activate(T::NoticeOk),
            Overlay::None => {}
        }
        match self.stage {
            Stage::Choice | Stage::Methods | Stage::Direct => self.activate(T::Previous),
            // spec gap: Esc does nothing on the game list (trace request MM5, default).
            Stage::Games => None,
            Stage::Waiting => self.activate(T::WaitCancel),
        }
    }

    /// Enter presses the focused button, or the screen's default when a list or a box has the focus.
    fn enter(&mut self) -> Option<Outcome> {
        let t = match (&self.overlay, self.focus) {
            (Overlay::Name(_), T::NameBox) => T::Ok,
            (Overlay::None, T::List | T::Row(_) | T::Address | T::Port | T::Mode) => T::Connect,
            (Overlay::None, T::GameList | T::GameRow(_)) => T::Join,
            (_, t) => t,
        };
        self.activate(t)
    }

    fn move_game(&mut self, k: Key) {
        let n = self.games.len();
        if n == 0 {
            return;
        }
        let at = self.game_sel as i32;
        let to = match k {
            Key::Up => at - 1,
            Key::Down => at + 1,
            Key::PageUp => at - GAMES_LINES as i32,
            Key::PageDown => at + GAMES_LINES as i32,
            Key::Home => 0,
            _ => n as i32 - 1,
        };
        self.select_game(to.clamp(0, n as i32 - 1) as usize);
    }

    fn select_game(&mut self, i: usize) {
        self.game_sel = i;
        if i < self.game_top {
            self.game_top = i;
        } else if i >= self.game_top + GAMES_LINES {
            self.game_top = i + 1 - GAMES_LINES;
        }
    }

    /// A control was used: clicked, or its key pressed.
    fn activate(&mut self, t: T) -> Option<Outcome> {
        self.changed = true;
        match t {
            T::PopRow(i) => self.pick_popup(i),
            T::Enhanced => self.choose_rules(true),
            T::Classic => self.choose_rules(false),
            T::Previous => match self.stage {
                Stage::Direct => self.go(Stage::Methods, T::List),
                _ => return Some(Outcome::Leave),
            },
            T::List => {}
            T::Row(i) => {
                if let Some(&m) = self.rows().get(i) {
                    self.mem.method = m;
                    self.focus = T::List;
                }
            }
            T::Connect => return self.connect_pressed(),
            T::Mode => {
                let at = usize::from(self.mem.answer);
                self.popup = Some(Popup::new(0, MODE_BOX, vec!["Call".into(), "Answer".into()], at));
            }
            T::Address => self.focus = T::Address,
            T::Port => self.focus = T::Port,
            T::NameBox => self.focus = T::NameBox,
            T::Ok => return self.name_ok(),
            T::Cancel => return self.cancel(),
            T::NoticeOk => self.overlay = Overlay::None,
            T::GameList => {}
            T::GameRow(i) => {
                self.select_game(i);
                self.focus = T::GameList;
            }
            T::Join => self.join_pressed(false),
            T::Watch => self.join_pressed(true),
            T::Create => return self.create_game(),
            T::Disconnect => {
                self.close_connection();
                return Some(Outcome::Leave);
            }
            T::WaitCancel => {
                self.join = None;
                self.go(Stage::Games, T::GameList);
            }
        }
        None
    }

    fn go(&mut self, stage: Stage, focus: T) {
        if stage != Stage::Games {
            self.finder = None;
        }
        self.stage = stage;
        self.focus = focus;
        self.ptr.reset();
        self.changed = true;
    }

    fn pick_popup(&mut self, i: usize) {
        if self.popup.take().is_some() {
            self.mem.answer = i == 1;
        }
    }

    fn choose_rules(&mut self, enhanced: bool) {
        self.enhanced = enhanced;
        // Remapping between rules: the same method's row, else the first (Battle.net moving to Classic).
        if !self.rows().contains(&self.mem.method) {
            self.mem.method = Method::DirectIp;
        }
        if enhanced {
            self.go(Stage::Methods, T::List);
        } else {
            // Classic mode asks for the name first, before the connection list.
            self.open_name(AfterName::List);
        }
    }

    fn open_name(&mut self, then: AfterName) {
        let start = if self.mem.name.is_empty() {
            self.strings.anonymous.clone()
        } else {
            self.mem.name.clone()
        };
        self.name = EditBox::new(&start, 11, Charset::Name);
        self.overlay = Overlay::Name(then);
        self.focus = T::Ok;
        self.ptr.reset();
    }

    fn name_ok(&mut self) -> Option<Outcome> {
        let Overlay::Name(then) = self.overlay else { return None };
        if self.name.is_empty() {
            return None;
        }
        self.mem.name = self.name.text.clone();
        self.overlay = Overlay::None;
        match then {
            AfterName::List => self.go(Stage::Methods, T::List),
            AfterName::Connect => return self.begin_connect(),
        }
        None
    }

    fn cancel(&mut self) -> Option<Outcome> {
        match &mut self.overlay {
            // Enhanced goes back to the screen the box opened over; Classic leaves for the main menu.
            Overlay::Name(then) => {
                let then = *then;
                self.overlay = Overlay::None;
                if then == AfterName::List {
                    return Some(Outcome::Leave);
                }
                self.close_connection();
                self.go(Stage::Methods, T::List);
            }
            Overlay::Connecting(c) => c.canceling = true,
            _ => {}
        }
        None
    }

    fn connect_pressed(&mut self) -> Option<Outcome> {
        if !self.mem.method.enabled() {
            return None;
        }
        if self.mem.method == Method::DirectIp && self.stage == Stage::Methods {
            self.go(Stage::Direct, T::Connect);
            return None;
        }
        if self.stage == Stage::Direct {
            self.mem.address = self.address.text.clone();
            self.mem.port = self.port.text.clone();
        }
        if self.enhanced {
            self.open_name(AfterName::Connect);
            None
        } else {
            self.begin_connect()
        }
    }

    // --- Connecting --------------------------------------------------------------------------------------------

    /// Makes the connection the method and the Mode ask for.
    fn begin_connect(&mut self) -> Option<Outcome> {
        match (self.mem.method, self.mem.answer) {
            // Call: this machine creates the game and goes straight to the lobby.
            (Method::DirectIp, false) => return self.create_game(),
            // Answer: the game at the address is looked for.
            (Method::DirectIp, true) => {
                let addr = self.target_address();
                self.overlay = Overlay::Connecting(spawn_connect(addr));
                self.focus = T::Cancel;
            }
            // Local Network always opens the game list.
            (Method::Local, _) => {
                self.games.clear();
                self.go(Stage::Games, T::Create);
                self.watch_lan();
            }
            (Method::Battle, _) => {}
        }
        None
    }

    fn target_address(&self) -> String {
        let port = if self.mem.port.is_empty() {
            DEFAULT_PORT.to_string()
        } else {
            self.mem.port.clone()
        };
        format!("{}:{}", self.mem.address, port)
    }

    /// Creates a game: the socket is opened and the lobby follows.
    fn create_game(&mut self) -> Option<Outcome> {
        let port = self.mem.port.parse::<u16>().unwrap_or(DEFAULT_PORT);
        let port = if self.mem.method == Method::Local {
            DEFAULT_PORT
        } else {
            port
        };
        match crate::net::tcp::listen(port) {
            Ok(l) => return Some(Outcome::Host(l)),
            Err(e) => {
                self.overlay = Overlay::Notice(format!("{e:#}"));
                self.focus = T::NoticeOk;
            }
        }
        None
    }

    fn join_pressed(&mut self, observer: bool) {
        let Some(g) = self.games.get(self.game_sel) else { return };
        if observer && !self.watch_offered() {
            return;
        }
        self.join = Some(spawn_join(
            g.addr.clone(),
            self.identity.clone(),
            self.mem.name.clone(),
            observer,
        ));
        self.go(Stage::Waiting, T::WaitCancel);
    }

    fn close_connection(&mut self) {
        self.join = None;
        self.overlay = Overlay::None;
    }

    /// The connecting box and the join wait, one frame.
    fn poll_background(&mut self) -> Option<Outcome> {
        if let Overlay::Connecting(c) = &mut self.overlay {
            match poll_conn(c) {
                ConnState::Pending => self.changed = true,
                ConnState::Failed(why) => {
                    let canceled = c.canceling;
                    self.overlay = if canceled { Overlay::None } else { Overlay::Notice(why) };
                    self.focus = if canceled { T::Connect } else { T::NoticeOk };
                    self.changed = true;
                }
                ConnState::Done => {
                    let canceled = c.canceling;
                    self.overlay = Overlay::None;
                    if canceled {
                        self.focus = T::Connect;
                    } else {
                        // The creator answered: its game is the list's.
                        self.games = vec![ListedGame {
                            addr: self.target_address(),
                            owner: self.mem.address.clone(),
                            players: 1,
                            max: 8,
                            map: String::new(),
                            watch: None,
                        }];
                        self.game_sel = 0;
                        self.go(Stage::Games, T::Join);
                    }
                }
            }
        }
        if let Some(j) = &mut self.join {
            let got = j.rx.lock().map_or(Err(TryRecvError::Disconnected), |r| r.try_recv());
            match got {
                Err(TryRecvError::Empty) => {
                    if j.started.elapsed() > JOIN_TIMEOUT {
                        self.join = None;
                        self.overlay = Overlay::Notice(self.strings.timed_out.clone());
                        self.go(Stage::Games, T::NoticeOk);
                    }
                }
                Ok(Err(why)) => {
                    self.join = None;
                    self.overlay = Overlay::Notice(why);
                    self.go(Stage::Games, T::NoticeOk);
                }
                Err(TryRecvError::Disconnected) => {
                    self.join = None;
                    self.overlay = Overlay::Notice("Connection terminated.".into());
                    self.go(Stage::Games, T::NoticeOk);
                }
                Ok(Ok(hs)) => {
                    self.join = None;
                    return Some(Outcome::Join(Box::new(hs)));
                }
            }
        }
        None
    }

    // --- Drawing -----------------------------------------------------------------------------------------------

    fn button_state(&self, t: T, enabled: bool, modal: bool, default: bool) -> ButtonState {
        ButtonState {
            enabled,
            hover: !modal && self.ptr.is_hot(t),
            focused: !modal && self.focus == t,
            pressed: !modal && self.ptr.pressed(t),
            default,
        }
    }

    /// The screen over `bg`, the menu picture.
    pub(crate) fn draw(&self, bg: &Picture, look: &Look) -> Picture {
        let mut p = Picture {
            width: bg.width,
            height: bg.height,
            rgba: bg.rgba.clone(),
        };
        let modal = !matches!(self.overlay, Overlay::None) || self.popup.is_some();
        match self.stage {
            Stage::Choice => {}
            Stage::Methods => self.draw_methods(&mut p, look, modal),
            Stage::Direct => self.draw_direct(&mut p, look, modal),
            Stage::Games => self.draw_games(&mut p, look, modal),
            Stage::Waiting => {
                draw_panel(look, &mut p, WAIT_PANEL);
                look.text(&mut p, WAIT_TEXT, "Waiting for response from remote system...", GOLD);
            }
        }
        for (t, r, label, key, enabled) in self.buttons() {
            let default = matches!(
                (self.stage, t),
                (Stage::Methods | Stage::Direct, T::Connect) | (Stage::Games, T::Join)
            );
            let s = self.button_state(t, enabled, modal, default);
            let key = (key != '\0').then_some(key);
            menuscreen::draw_button(&mut p, look.font, look.pal, Some(look.faces), r, label, key, s);
        }
        match &self.overlay {
            Overlay::None => {}
            Overlay::Name(_) => self.draw_name(&mut p, look),
            Overlay::Connecting(c) => self.draw_connecting(&mut p, look, c),
            Overlay::Notice(text) => self.draw_notice(&mut p, look, text),
        }
        if let Some(pop) = &self.popup {
            let hot = match self.ptr.hot {
                Some(T::PopRow(i)) => Some(i),
                _ => None,
            };
            draw_popup(look, &mut p, pop, hot);
        }
        p
    }

    fn draw_methods(&self, p: &mut Picture, look: &Look, modal: bool) {
        look.text(p, METHOD_HEADING, "Select connection method:", WHITE);
        let rgb = |e: u8| look.pal.0[e as usize];
        hud::fill(p, METHOD_LIST, rgb(menuscreen::BLACK));
        let ring = if !modal && self.focus == T::List {
            menuscreen::FOCUS_RING
        } else {
            *menuscreen::OUTER_RING.start()
        };
        hud::outline(p, METHOD_LIST, rgb(ring));
        let sel = self.selected_row();
        for (i, m) in self.rows().iter().enumerate() {
            let map = if !m.enabled() {
                GREY
            } else if i == sel {
                WHITE
            } else {
                GOLD
            };
            let y = METHOD_LIST.y + 1 + i as i32 * METHOD_ROW_H;
            look.text(p, (METHOD_LIST.x + 6, y + 1), m.label(), map);
        }
        for (i, line) in self.mem.method.description().iter().enumerate() {
            for (j, wrapped) in wrap(look.font, line, METHOD_WIDTH).iter().enumerate() {
                let y = METHOD_DESCRIPTION.1 + (i + j) as i32 * 17;
                look.text(p, (METHOD_DESCRIPTION.0, y), wrapped, GOLD);
            }
        }
    }

    fn draw_direct(&self, p: &mut Picture, look: &Look, modal: bool) {
        menuscreen::draw_heading(p, look.font, look.pal, SETUP_HEADING, "Direct Connection Setup");
        look.label(p, ADDRESS_LABEL, "Address:");
        look.label(p, MODE_LABEL, "Mode:");
        look.label(p, PORT_LABEL, "Port:");
        draw_edit(look, p, ADDRESS_BOX, &self.address, !modal && self.focus == T::Address);
        draw_edit(look, p, PORT_BOX, &self.port, !modal && self.focus == T::Port);
        let mode = if self.mem.answer { "Answer" } else { "Call" };
        look.drop(
            p,
            MODE_BOX,
            mode,
            DropState::of(
                !modal && self.ptr.is_hot(T::Mode),
                !modal && self.focus == T::Mode,
                !modal && self.ptr.pressed(T::Mode),
            ),
        );
    }

    fn draw_games(&self, p: &mut Picture, look: &Look, modal: bool) {
        look.text(p, GAMES_HEADING, "Current games:", WHITE);
        let rgb = |e: u8| look.pal.0[e as usize];
        hud::fill(p, GAMES_LIST, rgb(menuscreen::BLACK));
        let ring = if !modal && self.focus == T::GameList {
            menuscreen::FOCUS_RING
        } else {
            *menuscreen::OUTER_RING.start()
        };
        hud::outline(p, GAMES_LIST, rgb(ring));
        for i in self.game_top..self.games.len().min(self.game_top + GAMES_LINES) {
            let r = self.game_row_rect(i);
            let on = i == self.game_sel;
            if on {
                hud::fill(p, r, rgb(162));
            }
            let map = if on || self.ptr.is_hot(T::GameRow(i)) {
                WHITE
            } else {
                GOLD
            };
            let g = &self.games[i];
            let name = crate::customsetup::fit(look.font, &g.owner, r.w - 8);
            look.text(p, (r.x + 4, r.y + 2), &name, map);
        }
        match self.games.get(self.game_sel) {
            Some(g) => {
                let lines = [
                    format!("game owner: {}", g.owner),
                    format!("players: {}/{}", g.players, g.max),
                    if g.map.is_empty() {
                        String::new()
                    } else {
                        format!("map: {}", g.map)
                    },
                    // Our own line: only a game that has places for observers says so.
                    match g.watch {
                        Some(n) if n > 0 => format!("places to watch: {n}"),
                        _ => String::new(),
                    },
                ];
                for (i, l) in lines.iter().enumerate() {
                    look.text(
                        p,
                        (GAMES_INFO.0, GAMES_INFO.1 + i as i32 * 19),
                        // The first line stops short of the Watch Game button.
                        &crate::customsetup::fit(look.font, l, if i == 0 { 330 } else { 566 }),
                        GOLD,
                    );
                }
            }
            None => look.text(p, WAITING_LABEL, "Waiting for response", GOLD),
        }
    }

    fn draw_name(&self, p: &mut Picture, look: &Look) {
        draw_panel(look, p, SMALL_PANEL);
        look.text(p, NAME_LABEL, "Enter your name:", GOLD);
        draw_edit(look, p, NAME_BOX, &self.name, self.focus == T::NameBox);
        for (t, r, label, key, enabled) in [
            (T::Ok, NAME_OK, "OK", 'O', !self.name.is_empty()),
            (T::Cancel, NAME_CANCEL, "Cancel", 'C', true),
        ] {
            let s = self.button_state(t, enabled, false, t == T::Ok);
            menuscreen::draw_button(p, look.font, look.pal, Some(look.faces), r, label, Some(key), s);
        }
    }

    fn draw_connecting(&self, p: &mut Picture, look: &Look, c: &Conn) {
        draw_panel(look, p, SMALL_PANEL);
        let head = if c.canceling { "Canceling..." } else { "Connecting..." };
        look.text(p, BOX_TEXT, head, WHITE);
        let progress = if c.started.elapsed() < Duration::from_millis(1500) {
            &self.strings.establishing
        } else {
            &self.strings.contacting
        };
        look.text(p, (BOX_TEXT.0, BOX_TEXT.1 + 30), progress, GOLD);
        let s = self.button_state(T::Cancel, !c.canceling, false, false);
        menuscreen::draw_button(
            p,
            look.font,
            look.pal,
            Some(look.faces),
            BOX_CANCEL,
            "Cancel",
            Some('C'),
            s,
        );
    }

    fn draw_notice(&self, p: &mut Picture, look: &Look, text: &str) {
        draw_panel(look, p, SMALL_PANEL);
        for (i, l) in wrap(look.font, text, 264).iter().take(4).enumerate() {
            look.text(p, (BOX_TEXT.0, BOX_TEXT.1 + i as i32 * 18), l, GOLD);
        }
        let s = self.button_state(T::NoticeOk, true, false, true);
        menuscreen::draw_button(p, look.font, look.pal, Some(look.faces), BOX_CANCEL, "OK", Some('O'), s);
    }
}

/// `s` broken into lines no wider than `max` pixels, at spaces.
pub(crate) fn wrap(font: &tidewater_formats::fnt::Font, s: &str, max: u32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let try_line = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if line.is_empty() || menuscreen::label_width(font, &try_line) <= max {
            line = try_line;
        } else {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// A join being made: the connection, the hello and the creator's welcome, on a thread.
struct Join {
    started: Instant,
    rx: Mutex<Receiver<Result<Handshake, String>>>,
}

fn spawn_join(addr: String, identity: Identity, name: String, observer: bool) -> Join {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let r = crate::net::room::handshake_as(&addr, identity, &name, observer).map_err(|e| format!("{e:#}"));
        let _ = tx.send(r);
    });
    Join {
        started: Instant::now(),
        rx: Mutex::new(rx),
    }
}

enum ConnState {
    Pending,
    Done,
    Failed(String),
}

/// Starts connecting to `addr` on a thread, so the screen stays alive and Cancel works.
fn spawn_connect(addr: String) -> Conn {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(crate::net::tcp::probe(&addr).map_err(|e| format!("{e:#}")));
    });
    Conn {
        started: Instant::now(),
        rx: Mutex::new(rx),
        canceling: false,
    }
}

fn poll_conn(c: &mut Conn) -> ConnState {
    let got = c.rx.lock().map_or(Err(TryRecvError::Disconnected), |r| r.try_recv());
    match got {
        Ok(Ok(())) => ConnState::Done,
        Ok(Err(e)) => ConnState::Failed(e),
        Err(TryRecvError::Empty) => ConnState::Pending,
        Err(TryRecvError::Disconnected) => ConnState::Failed("Connection terminated.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::KeyEvent;

    fn key(k: Key) -> Input {
        Input {
            keys: vec![k],
            typed: vec![KeyEvent {
                key: k,
                pressed: true,
                text: None,
            }],
            ..Default::default()
        }
    }

    fn type_text(s: &str) -> Input {
        Input {
            typed: s
                .chars()
                .map(|c| KeyEvent {
                    key: Key::Other(0),
                    pressed: true,
                    text: Some(c.to_string()),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn click(mp: &mut Mp, x: i32, y: i32) -> Outcome {
        mp.update(&Input {
            at: Some((x, y)),
            down: true,
            ..Default::default()
        });
        mp.update(&Input {
            at: Some((x, y)),
            up: true,
            ..Default::default()
        })
    }

    fn fresh() -> Mp {
        Mp::new(Memory::default(), Strings::default())
    }

    fn at_methods(enhanced: bool) -> Mp {
        let mut mp = fresh();
        mp.update(&key(if enhanced { Key::Enter } else { Key::Letter('C') }));
        if !enhanced {
            mp.update(&key(Key::Enter));
        }
        assert_eq!(mp.stage, Stage::Methods);
        mp
    }

    #[test]
    fn the_choice_opens_on_enhanced_and_tab_goes_round() {
        let mut mp = fresh();
        assert_eq!((mp.stage, mp.focus, mp.enhanced()), (Stage::Choice, T::Enhanced, true));
        mp.update(&key(Key::Tab));
        assert_eq!(mp.focus, T::Classic);
        mp.update(&key(Key::Tab));
        assert_eq!(mp.focus, T::Previous);
        mp.update(&key(Key::Tab));
        assert_eq!(mp.focus, T::Enhanced);
    }

    #[test]
    fn the_pads_down_moves_the_focus_and_the_keyboards_does_not() {
        let mut mp = fresh();
        mp.update(&key(Key::Down));
        assert_eq!(mp.focus, T::Enhanced);
        mp.update(&Input {
            pad_focus: Some(FocusKey::Next),
            ..key(Key::Down)
        });
        assert_eq!(mp.focus, T::Classic);
        mp.update(&Input {
            pad_focus: Some(FocusKey::Back),
            ..key(Key::Up)
        });
        assert_eq!(mp.focus, T::Enhanced);
    }

    #[test]
    fn escape_and_previous_menu_leave_the_choice() {
        assert_eq!(fresh().update(&key(Key::Escape)), Outcome::Leave);
        assert_eq!(fresh().update(&key(Key::Letter('P'))), Outcome::Leave);
        assert_eq!(click(&mut fresh(), 300, 360), Outcome::Leave);
    }

    #[test]
    fn enhanced_lists_battle_net_disabled_and_selects_direct_ip_first() {
        let mp = at_methods(true);
        assert_eq!(Method::rows(true), &[Method::Battle, Method::DirectIp, Method::Local]);
        assert_eq!(mp.mem.method, Method::DirectIp);
        assert_eq!(mp.selected_row(), 1);
        assert!(!Method::Battle.enabled());
        assert_eq!(Method::Battle.description(), vec!["Battle.net is not available."]);
    }

    #[test]
    fn classic_asks_for_the_name_first_and_lists_two_rows() {
        let mut mp = fresh();
        mp.update(&key(Key::Letter('C')));
        assert!(matches!(mp.overlay, Overlay::Name(AfterName::List)));
        assert_eq!(mp.name.text, "Anonymous", "the first time the box opens on the default");
        mp.update(&type_text("x"));
        assert_eq!(mp.name.text, "Anonymousx", "not selected: typing adds to the end");
        // Cancel in Classic leaves for the main menu.
        let mut other = fresh();
        other.update(&key(Key::Letter('C')));
        assert_eq!(other.update(&key(Key::Escape)), Outcome::Leave);
        mp.update(&key(Key::Enter));
        assert_eq!(mp.stage, Stage::Methods);
        assert_eq!(mp.mem.name, "Anonymousx");
        assert_eq!(Method::rows(false), &[Method::DirectIp, Method::Local]);
    }

    #[test]
    fn a_name_is_one_to_eleven_characters_and_ok_is_off_while_empty() {
        let mut mp = fresh();
        mp.update(&key(Key::Letter('C')));
        for _ in 0..20 {
            mp.update(&key(Key::Backspace));
        }
        assert!(mp.name.is_empty());
        mp.update(&key(Key::Enter));
        assert!(matches!(mp.overlay, Overlay::Name(_)), "Enter does nothing while empty");
        mp.update(&type_text(" abcdefghijklmnop"));
        assert_eq!(mp.name.text, "abcdefghijk");
    }

    #[test]
    fn the_battle_net_row_selects_but_connect_does_nothing() {
        let mut mp = at_methods(true);
        click(&mut mp, 100, 206);
        assert_eq!(mp.mem.method, Method::Battle);
        assert!(!mp.stops().contains(&T::Connect));
        mp.update(&key(Key::Enter));
        assert_eq!(mp.stage, Stage::Methods);
        // The remapping: Classic has no Battle.net row, so it opens on Direct IP.
        let mem = Memory {
            method: Method::Battle,
            ..Memory::default()
        };
        let mut mp = Mp::new(mem, Strings::default());
        mp.update(&key(Key::Letter('C')));
        mp.update(&key(Key::Enter));
        assert_eq!(mp.mem.method, Method::DirectIp);
    }

    #[test]
    fn enhanced_direct_ip_goes_through_setup_then_the_name_then_connects() {
        let mut mp = at_methods(true);
        mp.update(&key(Key::Enter));
        assert_eq!(mp.stage, Stage::Direct);
        assert!(!mp.mem.answer, "Mode opens on Call");
        mp.update(&key(Key::Letter('C')));
        assert!(matches!(mp.overlay, Overlay::Name(AfterName::Connect)));
        // Call hosts at once: the socket is opened and the lobby follows.
        mp.port = EditBox::new("0", 5, Charset::Digits);
        mp.mem.port = "0".into();
        let o = mp.update(&key(Key::Enter));
        assert!(matches!(o, Outcome::Host(_)), "{o:?}");
        // The lobby closing returns to the connection list.
        mp.lobby_closed(None);
        assert_eq!(mp.stage, Stage::Methods);
    }

    #[test]
    fn the_name_box_comes_after_connect_on_every_press_in_enhanced() {
        let mut mp = at_methods(true);
        mp.update(&key(Key::Down)); // does nothing: the arrow keys are inert on the menus
        assert_eq!(mp.mem.method, Method::DirectIp);
        mp.update(&key(Key::Enter));
        mp.update(&key(Key::Escape));
        assert_eq!(
            mp.stage,
            Stage::Methods,
            "Previous Menu on the setup returns to the list"
        );
        click(&mut mp, 100, 242);
        assert_eq!(mp.mem.method, Method::Local);
        mp.update(&key(Key::Letter('C')));
        assert!(matches!(mp.overlay, Overlay::Name(AfterName::Connect)));
        mp.update(&key(Key::Escape));
        assert_eq!(mp.stage, Stage::Methods, "Cancel in Enhanced goes back to the list");
    }

    #[test]
    fn local_network_opens_the_game_list_and_disconnect_leaves() {
        let mut mp = at_methods(false);
        click(&mut mp, 100, 224);
        mp.update(&key(Key::Letter('C')));
        assert_eq!(mp.stage, Stage::Games, "Classic has asked for the name already");
        assert!(mp.games.is_empty());
        assert_eq!(
            mp.update(&key(Key::Escape)),
            Outcome::Stay,
            "Esc does nothing on the game list"
        );
        assert!(!mp.stops().contains(&T::Join), "nothing to join");
        assert_eq!(mp.update(&key(Key::Letter('D'))), Outcome::Leave);
    }

    #[test]
    fn the_mode_drop_down_is_remembered() {
        let mut mp = at_methods(true);
        mp.update(&key(Key::Enter));
        click(&mut mp, 120, 376);
        assert!(mp.popup.is_some());
        click(&mut mp, 120, 410);
        assert!(mp.mem.answer, "Answer picked from the list");
        assert!(mp.popup.is_none());
    }

    #[test]
    fn a_failed_connection_shows_a_box_and_ok_returns() {
        let mut mp = at_methods(false);
        mp.mem.answer = true;
        mp.mem.address = "127.0.0.1".into();
        mp.mem.port = "1".into();
        mp.update(&key(Key::Enter));
        assert_eq!(mp.stage, Stage::Direct);
        mp.update(&key(Key::Letter('C')));
        assert!(matches!(mp.overlay, Overlay::Connecting(_)));
        let end = Instant::now() + Duration::from_secs(15);
        while matches!(mp.overlay, Overlay::Connecting(_)) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
            mp.update(&Input::default());
        }
        assert!(matches!(mp.overlay, Overlay::Notice(_)), "the refusal is shown");
        mp.update(&key(Key::Letter('O')));
        assert!(matches!(mp.overlay, Overlay::None));
        assert_eq!(mp.stage, Stage::Direct);
    }

    #[test]
    fn a_game_found_at_the_address_joins_through_the_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut mp = at_methods(false);
        mp.mem.answer = true;
        mp.mem.address = "127.0.0.1".into();
        mp.mem.port = port.to_string();
        mp.update(&key(Key::Enter));
        mp.address = EditBox::new("127.0.0.1", 63, Charset::Address);
        mp.port = EditBox::new(&port.to_string(), 5, Charset::Digits);
        mp.update(&key(Key::Letter('C')));
        let end = Instant::now() + Duration::from_secs(10);
        while mp.stage != Stage::Games && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
            mp.update(&Input::default());
        }
        assert_eq!(mp.stage, Stage::Games);
        assert_eq!(mp.games.len(), 1);
        mp.update(&key(Key::Letter('J')));
        assert_eq!(mp.stage, Stage::Waiting);
        assert_eq!(mp.update(&key(Key::Escape)), Outcome::Stay);
        assert_eq!(mp.stage, Stage::Games, "Cancel returns to the list");
    }

    fn at_games(games: Vec<ListedGame>) -> Mp {
        let mut mp = at_methods(false);
        mp.games = games;
        mp.go(Stage::Games, T::GameList);
        mp
    }

    fn listed(addr: &str, watch: Option<u8>) -> ListedGame {
        ListedGame {
            addr: addr.into(),
            owner: "Ann".into(),
            players: 1,
            max: 4,
            map: String::new(),
            watch,
        }
    }

    #[test]
    fn watch_game_is_offered_only_for_a_game_that_may_take_an_observer() {
        // The original's three buttons only, for a game that says it has no places.
        let mp = at_games(vec![listed("127.0.0.1:1", Some(0))]);
        assert!(!mp.watch_offered());
        assert_eq!(mp.buttons().len(), 3);
        assert!(!mp.stops().contains(&T::Watch));
        // A game with places, or one whose list can't tell (Direct IP), offers it.
        for watch in [Some(2), None] {
            let mp = at_games(vec![listed("127.0.0.1:1", watch)]);
            assert!(mp.watch_offered());
            assert!(mp.buttons().iter().any(|b| b.0 == T::Watch && b.4));
            assert!(mp.stops().contains(&T::Watch));
        }
        // It follows the selection.
        let mut mp = at_games(vec![listed("127.0.0.1:1", Some(0)), listed("127.0.0.1:2", Some(1))]);
        assert!(!mp.watch_offered());
        mp.select_game(1);
        assert!(mp.watch_offered());
        // No games, no button.
        assert!(!at_games(Vec::new()).watch_offered());
    }

    #[test]
    fn watching_a_game_joins_as_an_observer_and_is_refused_where_there_is_no_place() {
        use crate::net::room::{HostRoom, Texts, new_view};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut room = HostRoom::new(
            listener,
            Identity::ours(0),
            Texts::default(),
            new_view(true, "Host", tidewater_sim::setup::race::NEUTRAL),
        )
        .unwrap();
        let addr = format!("127.0.0.1:{port}");
        // No places: the host says so, in a box.
        let mut mp = at_games(vec![listed(&addr, None)]);
        mp.mem.name = "Eve".into();
        mp.update(&key(Key::Letter('W')));
        assert_eq!(mp.stage, Stage::Waiting);
        let end = Instant::now() + Duration::from_secs(10);
        while mp.stage == Stage::Waiting && Instant::now() < end {
            room.poll();
            std::thread::sleep(Duration::from_millis(10));
            mp.update(&Input::default());
        }
        assert!(
            matches!(&mp.overlay, Overlay::Notice(t) if t.contains("no place for an observer")),
            "the refusal is shown"
        );
        // With a place, Join Game still plays and Watch Game watches.
        room.set_observers(1);
        let mut mp = at_games(vec![listed(&addr, Some(1))]);
        mp.mem.name = "Eve".into();
        mp.update(&key(Key::Letter('W')));
        let end = Instant::now() + Duration::from_secs(10);
        let hs = loop {
            room.poll();
            match mp.update(&Input::default()) {
                Outcome::Join(hs) => break hs,
                _ if Instant::now() > end => panic!("never welcomed"),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        assert!(hs.observer, "welcomed as an observer");
        assert!(hs.slot >= crate::net::proto::OBSERVER_BASE);
    }

    #[test]
    fn the_local_network_list_fills_from_beacons_and_stops_listening_on_leaving() {
        let finder = Finder::bind(0).unwrap();
        let port = finder.port();
        drop(finder);
        let mut mp = at_methods(false);
        mp.beacon_port = Some(port);
        click(&mut mp, 100, 224);
        mp.update(&key(Key::Letter('C')));
        assert_eq!(mp.stage, Stage::Games);
        let target = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
        let mut a = beacon::Announcer::to(target).unwrap();
        let mut b = beacon::Announcer::to(target).unwrap();
        let (one, two) = (
            beacon::Announce::ours(7001, 1, 4, "Ann", "Gold River"),
            beacon::Announce::ours(7002, 2, 3, "Bob", "Dark Paths").with_watch(2),
        );
        let end = Instant::now() + Duration::from_secs(10);
        while mp.games.len() < 2 && Instant::now() < end {
            a.tick(Instant::now(), &one);
            b.tick(Instant::now(), &two);
            std::thread::sleep(Duration::from_millis(20));
            mp.update(&Input::default());
        }
        assert_eq!(mp.games.len(), 2);
        assert!(mp.stops().contains(&T::Join), "a game to join");
        let first = &mp.games[0];
        assert_eq!((first.owner.as_str(), first.players, first.max), ("Ann", 1, 4));
        assert_eq!(first.map, "Gold River");
        assert!(first.addr.ends_with(":7001"));
        assert_eq!(
            (first.watch, mp.games[1].watch),
            (Some(0), Some(2)),
            "the beacon says where one can watch"
        );
        assert_eq!(mp.update(&key(Key::Letter('D'))), Outcome::Leave);
        assert!(mp.finder.is_none(), "leaving the list stops the listening");
    }
}
