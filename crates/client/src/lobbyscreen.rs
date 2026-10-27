//! The game setup lobby (spec `docs/specs/ui/game-setup-lobby.md`): a column of eight player rows, six option boxes, the
//! scenario box and the Select Scenario, Start Game and Cancel Game buttons, for the creator and for a joiner.
//! SDL-free like [`mpmenu`](crate::mpmenu): `frontend` feeds it the frame's input and shows what [`Lobby::draw`] gives.
//!
//! The creator's lobby ([`HostRoom`]) is the true one; a joiner's ([`GuestRoom`]) shows the last lobby the creator sent
//! and changes only its own row's race, Ready box and chat line. The Select scenario dialog is the Custom Scenario
//! screen's ([`customsetup::Screen`]), borrowed through its picker interface.
//!
//! Where the spec leaves a gap (each marked `// spec gap:` where used):
//! - **Row positions.** The spec lists the names' y positions (48, 68, 92 ...) and the chat lines' (48, 72, 96 ...)
//!   separately; both are used as written.
//! - **Game Speed** is two arrow boxes round a track with a knob, the speed's name after the label; the slider's range
//!   was not decoded (the nine Enhanced speeds), and it opens at the middle.
//! - **Start Game** is grey with nobody joined (trace request LB1 is open: Tidewater starts a network game with another
//!   human in it) and with a map that is missing; pressing it with more rows filled than the map has slots shows
//!   network entry 21.
//! - **Chat** is sent with Enter and shown in the sender's row; the sender's own line clears when they start a new
//!   message. The keys E, S and C are the buttons' only while no chat line has the focus.
//! - **Cheat Codes** (Classic mode) is a box with no effect: multiplayer games run with cheats off.

use tidewater_sim::setup::{Custom, race};

use crate::customsetup::{self, DropState, Input, Look, MapInfo, Opt, fit};
use crate::hud::{self, Rect};
use crate::input::Key;
use crate::menuscreen::{self, ButtonState, FocusKey, GOLD, GREY, WHITE};
use crate::mpwidgets::{Charset, EditBox, Popup, PopupKey, Ptr, draw_edit, draw_popup};
use crate::net::NetStart;
use crate::net::beacon::{Announce, Announcer};
use crate::net::proto::{LobbyView, MAX_OBSERVERS, RowState};
use crate::net::room::{GuestEvent, GuestRoom, HostRoom, ROWS, RowChoice, SAY_MAX, StartError, Texts};
use crate::scene::Picture;

/// A scenario reader: the file as the game would read it, and whether it is the user's own.
pub(crate) type ReadMap<'a> = dyn FnMut(&str) -> Option<(Vec<u8>, bool)> + 'a;

/// Keeps a map received from the creator (its name, hash and bytes) in the user's own folder: where it is now.
pub(crate) type SaveMap<'a> = dyn FnMut(&str, u64, &[u8]) -> Option<String> + 'a;

/// What the screen needs from the program beyond its input: map files and the clock.
pub(crate) struct Ctx<'a> {
    /// A scenario's file as the game would read it, and whether it is the user's own: for its hash.
    pub read: &'a mut ReadMap<'a>,
    /// Where a received map is kept (none: maps can't be received).
    pub save: Option<&'a mut SaveMap<'a>>,
    /// The wall clock in seconds: the game's seed.
    pub wall: u64,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(read: &'a mut ReadMap<'a>, wall: u64) -> Ctx<'a> {
        Ctx { read, save: None, wall }
    }

    pub(crate) fn with_save(mut self, save: &'a mut SaveMap<'a>) -> Ctx<'a> {
        self.save = Some(save);
        self
    }
}

/// What a frame of the lobby asks of the program.
pub(crate) enum Outcome {
    Stay,
    /// Cancel Game, a kick, or the end of the game: back to the menus, with the words to show if there are any.
    Cancel(Option<String>),
    /// Start Game, or the creator's start: play.
    Start(Box<NetStart>),
}

enum Backend {
    Host(Box<HostRoom>),
    Guest(Box<GuestRoom>),
}

/// A control the pointer or the focus can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum T {
    Row(usize),
    Ready(usize),
    Say,
    Race,
    Fog,
    Cheats,
    SpeedLess,
    SpeedMore,
    SpeedTrack,
    Resources,
    Tileset,
    Location,
    Units,
    /// The optional improvements (`docs/design/extras.md`): the ones that change the simulation and the client-only ones
    /// that give an edge, shown only once any is on.
    Extras,
    /// How many observers the creator lets in (our own addition: the original has none).
    Observers,
    Select,
    Start,
    Cancel,
    PopRow(usize),
    NoticeOk,
}

/// What an open list changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PopFor {
    Row(usize),
    Control(T),
}

const TITLE: Rect = Rect::new(160, 8, 320, 20);
const NAME_X: i32 = 36;
const NAME_W: u32 = 156;
const NAME_Y: [i32; ROWS] = [48, 68, 92, 116, 140, 164, 188, 212];
const CHAT_X: i32 = 200;
const CHAT_W: u32 = 388;
const CHAT_Y: [i32; ROWS] = [48, 72, 96, 120, 144, 168, 192, 216];
const LINE_H: u32 = 18;
const CLASSIC_LINE: (i32, i32) = (4, 26);
const SCENARIO_LABEL: (i32, i32) = (16, 402);
const SCENARIO_TEXT: (i32, i32) = (8, 424);
const SCENARIO_WIDTH: u32 = 380;
const BUTTON_X: i32 = 400;
const BUTTONS: [(T, i32, &str, char); 3] = [
    (T::Select, 368, "Select Scenario", 'E'),
    (T::Start, 404, "Start Game", 'S'),
    (T::Cancel, 440, "Cancel Game", 'C'),
];
const NOTICE: Rect = Rect::new(176, 256, 288, 128);
const NOTICE_TEXT: (i32, i32) = (188, 264);
const NOTICE_OK: Rect = Rect::new(268, 348, 106, 28);

const LABELS: [(T, &str, i32, i32); 6] = [
    (T::Race, "Your Race:", 40, 256),
    (T::Fog, "Fog of War:", 40, 306),
    (T::Resources, "Resources:", 220, 256),
    (T::Tileset, "Map Tileset:", 220, 306),
    (T::Location, "Location:", 220, 356),
    (T::Units, "Units:", 400, 256),
];
const EXTRAS_LABEL: (i32, i32) = (400, 306);
/// Our own small control under the player rows: the creator's places for observers, and who is watching.
const OBSERVERS_BOX: Rect = Rect::new(404, 236, 180, 20);
const WATCHERS_TEXT: (i32, i32) = (36, 236);
const WATCHERS_WIDTH: u32 = 360;
const SPEED_LABEL: (i32, i32) = (40, 356);
const CHEATS_LABEL: (i32, i32) = (40, 356);
const SPEED_LESS: Rect = Rect::new(44, 374, 22, 24);
const SPEED_TRACK: Rect = Rect::new(68, 374, 76, 24);
const SPEED_MORE: Rect = Rect::new(146, 374, 22, 24);

const RACES: [&str; 3] = ["Human", "Orc", "Random"];
const ON_OFF: [&str; 2] = ["On", "Off"];
const CHEAT_CHOICES: [&str; 2] = ["Allowed", "Disabled"];
const LOCATIONS: [&str; 2] = ["Random", "Fixed Order"];

fn drop_rect(t: T) -> Rect {
    match t {
        T::Race => Rect::new(44, 274, 124, 24),
        T::Fog => Rect::new(44, 324, 124, 24),
        T::Cheats => Rect::new(44, 374, 124, 24),
        T::Resources => Rect::new(224, 274, 144, 24),
        T::Tileset => Rect::new(224, 324, 144, 24),
        T::Location => Rect::new(224, 374, 144, 24),
        T::Extras => Rect::new(404, 324, 180, 24),
        T::Observers => OBSERVERS_BOX,
        _ => Rect::new(404, 274, 180, 24),
    }
}

/// Where a box's list opens: the Extras list is wider than its box, so a row and its value both fit.
fn popup_anchor(t: T) -> Rect {
    let r = drop_rect(t);
    if t == T::Extras {
        Rect::new(r.x - 100, r.y, r.w + 100, r.h)
    } else {
        r
    }
}

fn button_rect(y: i32) -> Rect {
    Rect::new(BUTTON_X, y, menuscreen::BUTTON_W, menuscreen::BUTTON_H)
}

fn name_rect(row: usize) -> Rect {
    Rect::new(NAME_X, NAME_Y[row], NAME_W, LINE_H)
}

fn chat_rect(row: usize) -> Rect {
    Rect::new(CHAT_X, CHAT_Y[row], CHAT_W, LINE_H)
}

fn ready_rect(row: usize) -> Rect {
    Rect::new(10, CHAT_Y[row], 18, 20)
}

/// The lobby screen.
pub(crate) struct Lobby {
    back: Option<Backend>,
    /// The lobby as drawn: the creator's own, or the last one the creator sent.
    view: LobbyView,
    /// Whether a lobby has arrived (a joiner draws only the title before).
    have_view: bool,
    /// The Select scenario dialog (the creator's).
    picker: customsetup::Screen,
    texts: Texts,
    me: usize,
    focus: T,
    ptr: Ptr<T>,
    popup: Option<(PopFor, Popup)>,
    /// What the player is typing in their own chat line.
    draft: EditBox,
    notice: Option<String>,
    /// The scenario the lobby shows, to notice a change of it.
    shown_map: u64,
    /// Tells the local network about this game (the creator's, Local Network only).
    beacon: Option<Announcer>,
    /// A joiner: the map the creator sent, by hash and the path it was kept at.
    received: Option<(u64, String)>,
    /// A joiner: the path to play the creator's map from, when it is the received one (the creator's name for it
    /// means nothing here).
    local_map: Option<String>,
    /// A joiner: how much of the creator's map has arrived, of how much (size 0: asked for).
    transfer: Option<(usize, usize)>,
    /// The creator keeps the Extras box once it shows (a joiner sees it while the lobby has an option on).
    extras_kept: bool,
    changed: bool,
}

impl Lobby {
    /// The creator's lobby over `room`, offering `maps`. `last` is the scenario picked last in this session.
    pub(crate) fn new_host(
        mut room: HostRoom,
        maps: Vec<MapInfo>,
        last: Option<&str>,
        texts: Texts,
        ctx: &mut Ctx,
    ) -> Lobby {
        let picker = customsetup::Screen::new(maps, last);
        let enhanced = room.view().enhanced;
        let mut l = Lobby {
            view: room.view().clone(),
            back: None,
            have_view: true,
            picker,
            texts,
            me: 0,
            focus: T::Select,
            ptr: Ptr::default(),
            popup: None,
            draft: EditBox::new("", SAY_MAX, Charset::Name),
            notice: None,
            shown_map: 0,
            beacon: None,
            received: None,
            local_map: None,
            transfer: None,
            extras_kept: !room.view().extras.is_default() || !room.view().client_extras.is_empty(),
            changed: true,
        };
        if let Some(m) = l.picker.map().cloned()
            && let Some((bytes, user)) = (ctx.read)(&m.path)
        {
            room.set_map(
                &tidewater_replay::start::recorded_map_name(&m.path, user),
                tidewater_replay::start::map_hash(&bytes),
                scenario_lines(&m),
                m.players,
            );
            room.set_map_data(bytes);
            l.view = room.view().clone();
        }
        l.shown_map = l.view.map_hash;
        l.view.enhanced = enhanced;
        l.back = Some(Backend::Host(Box::new(room)));
        l
    }

    /// A joiner's lobby over `room`.
    pub(crate) fn new_guest(room: GuestRoom, enhanced: bool) -> Lobby {
        // An observer has no row: its number is out of the rows' range.
        let me = if room.observer { ROWS } else { usize::from(room.slot) };
        let mut view = crate::net::room::new_view(enhanced, "", race::NEUTRAL);
        let have_view = room.view.is_some();
        if let Some(v) = &room.view {
            view = v.clone();
        }
        Lobby {
            view,
            back: Some(Backend::Guest(Box::new(room))),
            have_view,
            picker: customsetup::Screen::new(Vec::new(), None),
            texts: Texts::default(),
            me,
            focus: if me < ROWS { T::Say } else { T::Cancel },
            ptr: Ptr::default(),
            popup: None,
            draft: EditBox::new("", SAY_MAX, Charset::Name),
            notice: None,
            shown_map: 0,
            beacon: None,
            received: None,
            local_map: None,
            transfer: None,
            extras_kept: false,
            changed: true,
        }
    }

    /// From now on this game is announced on the local network while it has a free row.
    pub(crate) fn set_beacon(&mut self, a: Announcer) {
        self.beacon = Some(a);
    }

    /// One beacon, when it is due.
    fn announce(&mut self) {
        let (Some(b), Some(Backend::Host(h))) = (&mut self.beacon, &self.back) else {
            return;
        };
        if let Some(a) = announce_of(h.view(), h.port()) {
            b.tick(std::time::Instant::now(), &a);
        }
    }

    /// Where the lobby stands, for a scripted run to wait on (`script`).
    pub(crate) fn progress(&self) -> crate::script::LobbyProgress {
        crate::script::LobbyProgress {
            humans: self.view.rows.iter().filter(|r| r.state == RowState::Human).count(),
            watchers: self.view.watchers.len(),
            startable: self.is_host() && self.button_enabled(T::Start),
        }
    }

    fn is_host(&self) -> bool {
        matches!(self.back, Some(Backend::Host(_)))
    }

    /// The scenario the creator picked, to be remembered for the session.
    pub(crate) fn picked_map(&self) -> Option<String> {
        self.picker.map().map(|m| m.path.clone())
    }

    pub(crate) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed) | self.picker.take_changed()
    }

    fn room_enhanced(&self) -> bool {
        self.view.enhanced
    }

    /// Whether this machine only watches: an observer has no row, race, Ready box or chat line.
    fn watching(&self) -> bool {
        !self.is_host() && self.me >= ROWS
    }

    /// Whether the observers control shows: always for the creator, who decides; for a joiner only when the game has
    /// places for observers (a lobby without any looks as the original's does).
    fn observers_box(&self) -> bool {
        self.is_host() || self.view.observers > 0
    }

    // --- Input -------------------------------------------------------------------------------------------------

    pub(crate) fn update(&mut self, input: &Input, ctx: &mut Ctx) -> Outcome {
        if let Some(o) = self.poll_room(ctx) {
            return o;
        }
        self.announce();
        if !self.have_view {
            // A joiner waits for the creator's lobby; Escape leaves.
            if input.keys.contains(&Key::Escape) {
                return self.cancel();
            }
            return Outcome::Stay;
        }
        // The Select scenario dialog takes everything while it is open.
        if self.is_host() && self.picker.picker_open() {
            self.picker.update(input);
            self.sync_map(ctx);
            self.changed = true;
            return Outcome::Stay;
        }
        let targets = self.targets();
        let ev = self.ptr.update(&targets, input);
        self.changed |= ev.changed;
        if ev.down.is_some() || (input.down && self.popup.is_some()) {
            if let Some(t) = ev.down {
                if self.popup.is_some() && !matches!(t, T::PopRow(_)) {
                    self.popup = None;
                }
                if self.stops().contains(&t) {
                    self.focus = t;
                }
                if t == T::SpeedTrack {
                    self.set_speed_from_pointer(input);
                }
            } else {
                self.popup = None;
                self.ptr.reset();
            }
            self.changed = true;
        }
        if let Some(t) = ev.clicked {
            self.changed = true;
            if let Some(o) = self.activate(t, ctx) {
                return o;
            }
        }
        self.keys(input, ctx)
    }

    fn poll_room(&mut self, ctx: &mut Ctx) -> Option<Outcome> {
        match &mut self.back {
            Some(Backend::Host(h)) => {
                if h.poll() {
                    self.view = h.view().clone();
                    self.changed = true;
                }
                None
            }
            Some(Backend::Guest(g)) => {
                let events = g.poll();
                if let Some(v) = &g.view
                    && events.iter().any(|e| matches!(e, GuestEvent::View))
                {
                    self.view = v.clone();
                    self.have_view = true;
                    self.changed = true;
                }
                for e in events {
                    match e {
                        GuestEvent::View => {}
                        GuestEvent::Go(mut n) => {
                            // The creator's own name for a map that was sent here means nothing on this machine.
                            if let Some(path) = &self.local_map
                                && n.config.map_hash == self.view.map_hash
                            {
                                n.config.map_name = path.clone();
                            }
                            return Some(Outcome::Start(n));
                        }
                        GuestEvent::Ended(why) => return Some(Outcome::Cancel(Some(why))),
                        GuestEvent::MapProgress { got, size } => {
                            self.transfer = Some((got, size));
                            self.changed = true;
                        }
                        GuestEvent::MapReceived { hash, bytes } => {
                            self.transfer = None;
                            self.changed = true;
                            let saved = ctx
                                .save
                                .as_mut()
                                .and_then(|save| save(&self.view.map_name, hash, &bytes));
                            match saved {
                                Some(path) => {
                                    self.received = Some((hash, path.clone()));
                                    self.local_map = Some(path);
                                    if let Some(g) = self.guest() {
                                        g.set_have_map(hash, true);
                                    }
                                }
                                None => {
                                    self.notice = Some("The map could not be kept on this machine.".into());
                                    self.focus = T::NoticeOk;
                                }
                            }
                        }
                        GuestEvent::MapFailed(why) => {
                            self.transfer = None;
                            self.notice = Some(why);
                            self.focus = T::NoticeOk;
                            self.changed = true;
                        }
                    }
                }
                // The creator changed the scenario: this machine says whether it has it, and asks for it if not.
                if self.have_view && self.view.map_hash != self.shown_map {
                    self.shown_map = self.view.map_hash;
                    self.transfer = None;
                    self.settle_map(ctx);
                }
                None
            }
            None => None,
        }
    }

    fn guest(&mut self) -> Option<&mut GuestRoom> {
        match &mut self.back {
            Some(Backend::Guest(g)) => Some(g),
            _ => None,
        }
    }

    /// Whether this machine has the creator's map, from the installation, the user's own files or the map sent
    /// earlier; then `local_map` is where to play it from when that is the sent one.
    fn have_map(&mut self, ctx: &mut Ctx) -> bool {
        let (name, hash) = (self.view.map_name.clone(), self.view.map_hash);
        if name.is_empty() {
            return true;
        }
        if let Some((bytes, _)) = (ctx.read)(&name)
            && tidewater_replay::start::map_hash(&bytes) == hash
        {
            self.local_map = None;
            return true;
        }
        if let Some((h, path)) = &self.received
            && *h == hash
        {
            self.local_map = Some(path.clone());
            return true;
        }
        false
    }

    /// Tells the creator whether this machine has its map and, when it lacks it, asks for it.
    fn settle_map(&mut self, ctx: &mut Ctx) {
        let have = self.have_map(ctx);
        let hash = self.view.map_hash;
        let can_keep = ctx.save.is_some();
        let mut asked = false;
        if let Some(g) = self.guest() {
            g.set_have_map(hash, have);
            if !have && can_keep {
                g.request_map(hash);
                asked = true;
            }
        }
        if asked {
            self.transfer = Some((0, 0));
        } else if !have {
            self.notice = Some("This machine doesn't have the creator's scenario.".into());
            self.focus = T::NoticeOk;
        }
        self.changed = true;
    }

    /// The creator picked another scenario in the dialog: the room is told.
    fn sync_map(&mut self, ctx: &mut Ctx) {
        let Some(m) = self.picker.map().cloned() else { return };
        let Some((bytes, user)) = (ctx.read)(&m.path) else {
            return;
        };
        let name = tidewater_replay::start::recorded_map_name(&m.path, user);
        let hash = tidewater_replay::start::map_hash(&bytes);
        if let Some(Backend::Host(h)) = &mut self.back {
            h.set_map(&name, hash, scenario_lines(&m), m.players);
            h.set_map_data(bytes);
            self.view = h.view().clone();
            self.shown_map = hash;
        }
    }

    fn targets(&self) -> Vec<(T, Rect)> {
        let mut v = Vec::new();
        if let Some((_, p)) = &self.popup {
            v.extend((0..p.labels.len()).map(|i| (T::PopRow(i), p.row_rect(i))));
        }
        if self.notice.is_some() {
            v.push((T::NoticeOk, NOTICE_OK));
            return v;
        }
        let host = self.is_host();
        for row in 0..ROWS {
            let r = &self.view.rows[row];
            if host && row > 0 {
                v.push((T::Row(row), name_rect(row)));
            }
            if row == self.me && r.state == RowState::Human {
                v.push((T::Say, chat_rect(row)));
            }
            if !self.room_enhanced() && !host && row == self.me && row > 0 {
                v.push((T::Ready(row), ready_rect(row)));
            }
        }
        for t in self.option_stops() {
            if t == T::Cheats || t == T::SpeedTrack {
                continue;
            }
            v.push((t, drop_rect(t)));
        }
        if self.room_enhanced() {
            v.push((T::SpeedLess, SPEED_LESS));
            v.push((T::SpeedTrack, SPEED_TRACK));
            v.push((T::SpeedMore, SPEED_MORE));
        } else {
            v.push((T::Cheats, drop_rect(T::Cheats)));
        }
        for (t, y, _, _) in BUTTONS {
            if self.button_visible(t) {
                v.push((t, button_rect(y)));
            }
        }
        v
    }

    /// The option boxes that exist on this screen, in Tab order.
    fn option_stops(&self) -> Vec<T> {
        let mut v = if self.watching() { Vec::new() } else { vec![T::Race] };
        if self.is_host() {
            v.extend([T::Fog, T::Resources, T::Tileset, T::Location, T::Units]);
            if self.extras_box() {
                v.push(T::Extras);
            }
            v.push(T::Observers);
        }
        v
    }

    fn stops(&self) -> Vec<T> {
        let host = self.is_host();
        let mut v = Vec::new();
        if host {
            v.extend((1..ROWS).map(T::Row));
        } else if !self.room_enhanced() && self.me > 0 && self.me < ROWS {
            v.push(T::Ready(self.me));
        }
        if !self.watching() {
            v.push(T::Say);
        }
        v.extend(self.option_stops());
        if host {
            if self.room_enhanced() {
                v.extend([T::SpeedLess, T::SpeedMore]);
            } else {
                v.push(T::Cheats);
            }
        }
        for (t, _, _, _) in BUTTONS {
            if self.button_visible(t) && self.button_enabled(t) {
                v.push(t);
            }
        }
        v
    }

    fn button_visible(&self, t: T) -> bool {
        // Start Game is not shown on a joiner's screen.
        t != T::Start || self.is_host()
    }

    fn button_enabled(&self, t: T) -> bool {
        match t {
            T::Select => self.is_host(),
            T::Start => {
                matches!(&self.back, Some(Backend::Host(h)) if matches!(h.can_start(), Ok(()) | Err(StartError::TooMany)))
            }
            _ => true,
        }
    }

    /// Whether the player's chat line has the focus, so typing goes into it.
    pub(crate) fn text_focused(&self) -> bool {
        self.notice.is_none() && self.popup.is_none() && self.focus == T::Say
    }

    /// The player's chat line while it has the focus.
    pub(crate) fn text_rect(&self) -> Option<Rect> {
        self.text_focused().then(|| chat_rect(self.me))
    }

    fn keys(&mut self, input: &Input, ctx: &mut Ctx) -> Outcome {
        let mut pad = input.pad_focus;
        if self.text_focused() {
            for e in &input.typed {
                self.changed = true;
                if e.pressed && self.pad_step(&mut pad, e.key) {
                    continue;
                }
                if let Some(k) = self.draft.feed(e, input.command_held)
                    && let Some(o) = self.key(k, input.shift, ctx)
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
            if let Some(o) = self.key(k, input.shift, ctx) {
                return o;
            }
        }
        Outcome::Stay
    }

    /// A pad's d-pad up or down moves the focus, as Tab and Shift+Tab do, except in an open pop-up (where it moves the
    /// line) and under a notice.
    fn pad_step(&mut self, pad: &mut Option<FocusKey>, k: Key) -> bool {
        if self.popup.is_some() || self.notice.is_some() {
            return false;
        }
        let Some(step) = menuscreen::pad_step(pad, k) else {
            return false;
        };
        self.focus = menuscreen::step_focus(&self.stops(), self.focus, step);
        true
    }

    fn key(&mut self, k: Key, shift: bool, ctx: &mut Ctx) -> Option<Outcome> {
        if let Some((_, p)) = &mut self.popup {
            match p.key(k) {
                PopupKey::Open => {}
                PopupKey::Closed => self.popup = None,
                PopupKey::Picked(i) => self.pick(i),
            }
            return None;
        }
        if self.notice.is_some() {
            if matches!(k, Key::Enter | Key::NumpadEnter | Key::Escape | Key::Space)
                || k == Key::Letter('O')
                || k == Key::Letter('o')
            {
                self.notice = None;
                self.focus = T::Start;
            }
            return None;
        }
        let stops = self.stops();
        match k {
            Key::Tab => {
                let back = if shift { FocusKey::Back } else { FocusKey::Next };
                self.focus = menuscreen::step_focus(&stops, self.focus, back);
                None
            }
            Key::Escape => {
                if self.focus == T::Say {
                    self.focus = T::Cancel;
                    None
                } else {
                    self.activate(T::Cancel, ctx)
                }
            }
            Key::Enter | Key::NumpadEnter => {
                if self.focus == T::Say {
                    self.send_say();
                    None
                } else {
                    self.activate(self.focus, ctx)
                }
            }
            Key::Space => match self.focus {
                T::Say => None,
                f => self.activate(f, ctx),
            },
            Key::Left if self.room_enhanced() && self.is_host() => self.activate(T::SpeedLess, ctx),
            Key::Right if self.room_enhanced() && self.is_host() => self.activate(T::SpeedMore, ctx),
            Key::Letter(c) => {
                let c = c.to_ascii_uppercase();
                let hit = BUTTONS
                    .iter()
                    .find(|b| b.3 == c && self.button_visible(b.0) && self.button_enabled(b.0))
                    .map(|b| b.0);
                if let Some(t) = hit {
                    self.focus = t;
                    return self.activate(t, ctx);
                }
                None
            }
            _ => None,
        }
    }

    fn send_say(&mut self) {
        let text = std::mem::replace(&mut self.draft, EditBox::new("", SAY_MAX, Charset::Name)).text;
        match &mut self.back {
            Some(Backend::Host(h)) => {
                h.say(&text);
                self.view = h.view().clone();
            }
            Some(Backend::Guest(g)) => g.say(&text),
            None => {}
        }
    }

    fn cancel(&mut self) -> Outcome {
        match self.back.take() {
            Some(Backend::Host(h)) => h.cancel(),
            Some(Backend::Guest(_)) | None => {}
        }
        Outcome::Cancel(None)
    }

    fn activate(&mut self, t: T, ctx: &mut Ctx) -> Option<Outcome> {
        self.changed = true;
        match t {
            T::PopRow(i) => self.pick(i),
            T::Cancel => return Some(self.cancel()),
            T::NoticeOk => {
                self.notice = None;
                self.focus = T::Start;
            }
            T::Say => self.focus = T::Say,
            T::Select if self.is_host() => {
                self.picker.open_picker();
                self.focus = T::Select;
            }
            T::Start => return self.start(ctx),
            T::Row(row) => {
                let human = self.view.rows[row].state == RowState::Human;
                let mut labels = vec!["Available".to_string(), "Computer".into(), "Closed".into()];
                if human {
                    labels.push("Kick Out".into());
                }
                self.popup = Some((PopFor::Row(row), Popup::new(row, name_rect(row), labels, 0)));
            }
            T::Ready(row) => {
                let on = !self.view.rows[row].ready;
                if let Some(Backend::Guest(g)) = &mut self.back {
                    g.set_ready(on);
                }
            }
            T::Race
            | T::Fog
            | T::Cheats
            | T::Resources
            | T::Tileset
            | T::Location
            | T::Units
            | T::Extras
            | T::Observers => self.open_list(t),
            T::SpeedLess => self.step_speed(-1),
            T::SpeedMore => self.step_speed(1),
            T::SpeedTrack => {}
            T::Select => {}
        }
        None
    }

    fn start(&mut self, ctx: &mut Ctx) -> Option<Outcome> {
        let Some(Backend::Host(h)) = &self.back else {
            return None;
        };
        match h.can_start() {
            Ok(()) => {}
            Err(StartError::TooMany) => {
                self.notice = Some(self.texts.too_many.clone());
                self.focus = T::NoticeOk;
                return None;
            }
            Err(_) => return None,
        }
        let Some(Backend::Host(h)) = self.back.take() else {
            return None;
        };
        match h.start(ctx.wall as u32) {
            Ok(n) => Some(Outcome::Start(Box::new(n))),
            Err(e) => Some(Outcome::Cancel(Some(format!("{e:#}")))),
        }
    }

    // --- The options -------------------------------------------------------------------------------------------

    fn choices(&self, t: T) -> Vec<String> {
        let classic = !self.view.enhanced;
        let opt = |o: Opt| {
            let labels = o.labels();
            // Enhanced rules add Random to Resources and Map Tileset; Classic mode lists the others.
            let n = if classic && matches!(o, Opt::Resources | Opt::Tileset) {
                labels.len() - 1
            } else {
                labels.len()
            };
            labels[..n].iter().map(|s| (*s).to_string()).collect()
        };
        match t {
            T::Race => RACES.iter().map(|s| (*s).to_string()).collect(),
            T::Fog => ON_OFF.iter().map(|s| (*s).to_string()).collect(),
            T::Cheats => CHEAT_CHOICES.iter().map(|s| (*s).to_string()).collect(),
            T::Location => LOCATIONS.iter().map(|s| (*s).to_string()).collect(),
            T::Extras => extras_labels(&self.view.extras, &self.view.client_extras),
            T::Observers => observer_labels(),
            T::Resources => opt(Opt::Resources),
            T::Tileset => opt(Opt::Tileset),
            _ => opt(Opt::Units),
        }
    }

    /// The choice a box shows: an index into [`Self::choices`].
    fn current(&self, t: T) -> usize {
        let v = &self.view;
        match t {
            T::Race => usize::from(v.rows[self.me.min(ROWS - 1)].race.min(2)),
            T::Fog => usize::from(!v.custom.fog_of_war),
            T::Cheats => usize::from(!v.cheats_allowed),
            T::Location => usize::from(v.custom.fixed_starts),
            T::Extras => 0,
            T::Observers => usize::from(v.observers),
            T::Resources => customsetup::choice(&v.custom, Opt::Resources),
            T::Tileset => customsetup::choice(&v.custom, Opt::Tileset),
            _ => customsetup::choice(&v.custom, Opt::Units),
        }
    }

    /// Whether the Extras box shows: once an optional improvement that changes the simulation is on in this lobby, or
    /// was on in the creator's settings. A lobby with none looks as it always has.
    fn extras_box(&self) -> bool {
        self.extras_kept || !self.view.extras.is_default() || !self.view.client_extras.is_empty()
    }

    fn open_list(&mut self, t: T) {
        if t != T::Race && !self.is_host() {
            return;
        }
        let at = self.current(t);
        self.popup = Some((
            PopFor::Control(t),
            Popup::new(100, popup_anchor(t), self.choices(t), at),
        ));
    }

    fn pick(&mut self, i: usize) {
        let Some((what, _)) = self.popup.take() else { return };
        match what {
            PopFor::Row(row) => {
                let choice = [
                    RowChoice::Available,
                    RowChoice::Computer,
                    RowChoice::Closed,
                    RowChoice::KickOut,
                ]
                .get(i)
                .copied();
                if let (Some(c), Some(Backend::Host(h))) = (choice, &mut self.back) {
                    h.set_row(row, c);
                    self.view = h.view().clone();
                }
            }
            PopFor::Control(t) => {
                self.set_option(t, i);
                // The Extras list stays open for the next change; Escape or a click outside closes it.
                if t == T::Extras {
                    self.popup = Some((PopFor::Control(t), Popup::new(100, popup_anchor(t), self.choices(t), i)));
                }
            }
        }
    }

    fn set_option(&mut self, t: T, i: usize) {
        if t == T::Race {
            let race = i.min(2) as u8;
            match &mut self.back {
                Some(Backend::Host(h)) => {
                    h.set_host_race(race);
                    self.view = h.view().clone();
                }
                Some(Backend::Guest(g)) => g.set_race(race),
                None => {}
            }
            return;
        }
        let Some(Backend::Host(h)) = &mut self.back else { return };
        if t == T::Extras {
            if i < SIM_EXTRAS {
                h.set_extras(cycle_extra(self.view.extras, i));
            } else {
                h.set_client_extras(cycle_lobby_extra(&self.view.client_extras, i - SIM_EXTRAS));
            }
            self.view = h.view().clone();
            self.extras_kept = true;
            return;
        }
        if t == T::Observers {
            h.set_observers(i.min(MAX_OBSERVERS) as u8);
            self.view = h.view().clone();
            return;
        }
        let mut custom: Custom = self.view.custom;
        let (mut speed, mut cheats) = (self.view.speed, self.view.cheats_allowed);
        match t {
            T::Fog => custom.fog_of_war = i == 0,
            T::Cheats => cheats = i == 0,
            T::Location => custom.fixed_starts = i == 1,
            T::Resources => customsetup::set_choice(&mut custom, Opt::Resources, i),
            T::Tileset => customsetup::set_choice(&mut custom, Opt::Tileset, i),
            T::Units => customsetup::set_choice(&mut custom, Opt::Units, i),
            _ => speed = i.min(8) as u8,
        }
        h.set_options(custom, speed, cheats);
        self.view = h.view().clone();
    }

    fn step_speed(&mut self, by: i32) {
        if !self.is_host() {
            return;
        }
        let to = (i32::from(self.view.speed) + by).clamp(0, 8) as usize;
        self.set_option(T::SpeedTrack, to);
    }

    fn set_speed_from_pointer(&mut self, input: &Input) {
        if !self.is_host() {
            return;
        }
        let Some((x, _)) = input.at else { return };
        let into = (x - SPEED_TRACK.x).clamp(0, SPEED_TRACK.w as i32 - 1) as usize;
        let to = into * 9 / SPEED_TRACK.w as usize;
        self.set_option(T::SpeedTrack, to);
    }

    // --- Drawing -----------------------------------------------------------------------------------------------

    fn drop_state(&self, t: T, modal: bool) -> DropState {
        DropState::of(
            !modal && self.ptr.is_hot(t),
            !modal && self.focus == t,
            !modal && self.ptr.pressed(t),
        )
    }

    /// The screen over `bg`, the menu picture.
    pub(crate) fn draw(&self, bg: &Picture, look: &Look) -> Picture {
        let mut p = Picture {
            width: bg.width,
            height: bg.height,
            rgba: bg.rgba.clone(),
        };
        menuscreen::draw_heading(&mut p, look.font, look.pal, TITLE, "Multi Player Setup");
        if !self.have_view {
            look.text(&mut p, (188, 264), "Joining the game...", GOLD);
            return p;
        }
        let modal = self.popup.is_some() || self.notice.is_some() || self.picker.picker_open();
        let host = self.is_host();
        if !self.view.enhanced {
            look.text(
                &mut p,
                CLASSIC_LINE,
                "Ready (Each player must click Ready to start.)",
                GOLD,
            );
        }
        self.draw_rows(&mut p, look, modal);
        self.draw_options(&mut p, look, modal);
        look.label(&mut p, SCENARIO_LABEL, "Scenario:");
        for (i, l) in self.view.scenario.iter().enumerate() {
            let at = (SCENARIO_TEXT.0, SCENARIO_TEXT.1 + i as i32 * 17);
            // spec gap: while the creator's map is on its way the description line says so.
            let line = match (i, self.transfer) {
                (2, Some((_, 0))) => "Asking for the map...".to_string(),
                (2, Some((got, size))) => format!("Receiving the map... {}%", got * 100 / size.max(1)),
                _ => l.clone(),
            };
            look.text(&mut p, at, &fit(look.font, &line, SCENARIO_WIDTH), GOLD);
        }
        for (t, y, label, key) in BUTTONS {
            if !self.button_visible(t) {
                continue;
            }
            let s = ButtonState {
                enabled: self.button_enabled(t),
                hover: !modal && self.ptr.is_hot(t),
                focused: !modal && self.focus == t,
                pressed: !modal && self.ptr.pressed(t),
                default: false,
            };
            menuscreen::draw_button(
                &mut p,
                look.font,
                look.pal,
                Some(look.faces),
                button_rect(y),
                label,
                Some(key),
                s,
            );
        }
        if host && self.picker.picker_open() {
            self.picker.draw_picker(&mut p, look);
        }
        if let Some(text) = &self.notice {
            crate::mpwidgets::draw_panel(look, &mut p, NOTICE);
            for (i, l) in crate::mpmenu::wrap(look.font, text, NOTICE.w - 24).iter().enumerate() {
                look.text(&mut p, (NOTICE_TEXT.0, NOTICE_TEXT.1 + i as i32 * 17), l, GOLD);
            }
            let s = ButtonState {
                enabled: true,
                hover: self.ptr.is_hot(T::NoticeOk),
                focused: self.focus == T::NoticeOk,
                pressed: self.ptr.pressed(T::NoticeOk),
                default: true,
            };
            menuscreen::draw_button(
                &mut p,
                look.font,
                look.pal,
                Some(look.faces),
                NOTICE_OK,
                "OK",
                Some('O'),
                s,
            );
        }
        if let Some((_, pop)) = &self.popup {
            let hot = match self.ptr.hot {
                Some(T::PopRow(i)) => Some(i),
                _ => None,
            };
            draw_popup(look, &mut p, pop, hot);
        }
        p
    }

    fn draw_rows(&self, p: &mut Picture, look: &Look, modal: bool) {
        let host = self.is_host();
        for row in 0..ROWS {
            let r = &self.view.rows[row];
            let text = match r.state {
                RowState::Available => "Available",
                RowState::Computer => "Computer",
                RowState::Closed => "Closed",
                RowState::Human => r.name.as_str(),
            };
            let nr = name_rect(row);
            if host && row > 0 {
                look.drop(p, nr, text, self.drop_state(T::Row(row), modal));
            } else {
                let map = if r.state == RowState::Human { WHITE } else { GOLD };
                look.text(p, (nr.x + 4, nr.y + 1), &fit(look.font, text, nr.w - 8), map);
            }
            if r.state == RowState::Human {
                let own = row == self.me;
                let mut line = r.say.clone();
                if own && !self.draft.is_empty() {
                    line.clone_from(&self.draft.text);
                }
                let b = EditBox {
                    text: line,
                    max: SAY_MAX,
                    set: Charset::Name,
                };
                draw_edit(look, p, chat_rect(row), &b, own && !modal && self.focus == T::Say);
            }
            if !self.view.enhanced && row > 0 && r.state == RowState::Human {
                let rr = ready_rect(row);
                let rgb = |e: u8| look.pal.0[e as usize];
                hud::fill(p, rr, rgb(menuscreen::BLACK));
                let focused = !modal && self.focus == T::Ready(row);
                hud::outline(
                    p,
                    rr,
                    rgb(if focused {
                        menuscreen::FOCUS_RING
                    } else {
                        *menuscreen::OUTER_RING.start()
                    }),
                );
                if r.ready {
                    look.text(p, (rr.x + 4, rr.y + 3), "x", WHITE);
                }
            }
        }
    }

    fn draw_options(&self, p: &mut Picture, look: &Look, modal: bool) {
        let host = self.is_host();
        for (t, title, x, y) in LABELS {
            // A joiner sees the creator's choices; the boxes stay (disabled) and show them. An observer has no race.
            let title = if t == T::Race && self.watching() { "You:" } else { title };
            look.label(p, (x, y), title);
        }
        let mut boxes = vec![T::Race, T::Fog, T::Resources, T::Tileset, T::Location, T::Units];
        if self.extras_box() {
            look.label(p, EXTRAS_LABEL, "Extras:");
            boxes.push(T::Extras);
        }
        for t in boxes {
            let text = if t == T::Extras {
                extras_summary(&self.view.extras, &self.view.client_extras)
            } else if t == T::Race && self.watching() {
                "Observer".to_string()
            } else {
                self.choices(t).get(self.current(t)).cloned().unwrap_or_default()
            };
            let enabled = host || (t == T::Race && !self.watching());
            let st = if enabled {
                self.drop_state(t, modal)
            } else {
                DropState::of(false, false, false)
            };
            look.drop(p, drop_rect(t), &text, st);
        }
        if self.observers_box() {
            let text = observer_labels().swap_remove(usize::from(self.view.observers).min(MAX_OBSERVERS));
            let st = if host {
                self.drop_state(T::Observers, modal)
            } else {
                DropState::of(false, false, false)
            };
            look.drop(p, OBSERVERS_BOX, &text, st);
        }
        if !self.view.watchers.is_empty() {
            let line = format!("Watching: {}", self.view.watchers.join(", "));
            look.text(p, WATCHERS_TEXT, &fit(look.font, &line, WATCHERS_WIDTH), GOLD);
        }
        if self.view.enhanced {
            let name = tidewater_sim::timing::SPEED_NAMES[usize::from(self.view.speed).min(8)];
            look.label(p, SPEED_LABEL, &format!("Game Speed: {name}"));
            self.draw_slider(p, look, modal);
        } else {
            look.label(p, CHEATS_LABEL, "Cheat Codes:");
            let text = CHEAT_CHOICES[usize::from(!self.view.cheats_allowed)];
            let st = if host {
                self.drop_state(T::Cheats, modal)
            } else {
                DropState::of(false, false, false)
            };
            look.drop(p, drop_rect(T::Cheats), text, st);
        }
    }

    fn draw_slider(&self, p: &mut Picture, look: &Look, modal: bool) {
        let host = self.is_host();
        let rgb = |e: u8| look.pal.0[e as usize];
        for (t, r, glyph) in [(T::SpeedLess, SPEED_LESS, "<"), (T::SpeedMore, SPEED_MORE, ">")] {
            hud::fill(p, r, rgb(menuscreen::BLACK));
            let lit = host && !modal && (self.focus == t || self.ptr.is_hot(t));
            hud::outline(
                p,
                r,
                rgb(if lit {
                    menuscreen::FOCUS_RING
                } else {
                    *menuscreen::OUTER_RING.start()
                }),
            );
            look.text(p, (r.x + 7, r.y + 5), glyph, if host { WHITE } else { GREY });
        }
        hud::fill(p, SPEED_TRACK, rgb(menuscreen::BLACK));
        hud::outline(p, SPEED_TRACK, rgb(*menuscreen::OUTER_RING.start()));
        let span = SPEED_TRACK.w as i32 - 12;
        let x = SPEED_TRACK.x + 2 + i32::from(self.view.speed.min(8)) * span / 8;
        hud::fill(p, Rect::new(x, SPEED_TRACK.y + 3, 8, SPEED_TRACK.h - 6), rgb(200));
    }
}

/// How many rows of the Extras list are the simulation's options; the client-only ones (C+L) follow.
const SIM_EXTRAS: usize = 5;

/// The observers list: how many places the creator allows.
fn observer_labels() -> Vec<String> {
    let mut v = vec!["Observers: None".to_string()];
    v.extend((1..=MAX_OBSERVERS).map(|n| format!("Observers: {n}")));
    v
}

/// The Extras list's rows: each option with its value, the simulation's and then the C+L options the creator chooses
/// for everyone (read from the registry).
fn extras_labels(e: &tidewater_sim::Extras, lobby: &[(String, u8)]) -> Vec<String> {
    use tidewater_sim::extras::UNLIMITED;
    let on = |b: bool| if b { "On" } else { "Off" };
    let limit = match e.selection_limit {
        0 => "9".to_string(),
        UNLIMITED => "No limit".to_string(),
        n => n.to_string(),
    };
    let speeds = if e.extra_speeds == 0 {
        "Off".to_string()
    } else {
        format!("+{}", e.extra_speeds)
    };
    vec![
        format!("Rally points: {}", on(e.rally_points)),
        format!("Production queue: {}", on(e.production_queue)),
        format!("Selection: {limit}"),
        format!("Extra speeds: {speeds}"),
        format!("Shift-queued orders: {}", on(e.order_queue)),
    ]
    .into_iter()
    .chain(lobby_extra_labels(lobby))
    .collect()
}

/// The rows of the C+L options: the label and the value in words.
fn lobby_extra_labels(values: &[(String, u8)]) -> Vec<String> {
    let e = crate::extras::ClientExtras::from_lobby_values(values);
    crate::extras::lobby_specs()
        .map(|s| {
            let v = e.get(s.id);
            let shown = match s.kind {
                crate::extras::Kind::Flag => (if v != 0 { "On" } else { "Off" }).to_string(),
                _ => s.text(v),
            };
            format!("{}: {shown}", s.label)
        })
        .collect()
}

/// The C+L values with option number `i` (among the C+L options) moved to its next value.
fn cycle_lobby_extra(values: &[(String, u8)], i: usize) -> Vec<(String, u8)> {
    let mut e = crate::extras::ClientExtras::from_lobby_values(values);
    if let Some(s) = crate::extras::lobby_specs().nth(i) {
        e.set(s.id, crate::extras::next_value(s, e.get(s.id)));
    }
    e.lobby_values()
}

/// The Extras box's text: "None", or the options that are on.
fn extras_summary(e: &tidewater_sim::Extras, lobby: &[(String, u8)]) -> String {
    let mut on = e.describe();
    on.extend(crate::extras::describe_lobby(lobby));
    let text = on.join(", ");
    // The box is 180 pixels wide: a long list is only counted.
    if on.is_empty() {
        "None".to_string()
    } else if text.len() > 20 {
        format!("{} on", on.len())
    } else {
        text
    }
}

/// The options with row `i` of the Extras list changed to its next value.
fn cycle_extra(mut e: tidewater_sim::Extras, i: usize) -> tidewater_sim::Extras {
    use tidewater_sim::extras::{MAX_EXTRA_SPEEDS, UNLIMITED};
    match i {
        0 => e.rally_points = !e.rally_points,
        1 => e.production_queue = !e.production_queue,
        2 => {
            e.selection_limit = match e.selection_limit {
                0 => 12,
                12 => 18,
                18 => UNLIMITED,
                _ => 0,
            }
        }
        3 => e.extra_speeds = (e.extra_speeds + 1) % (MAX_EXTRA_SPEEDS + 1),
        _ => e.order_queue = !e.order_queue,
    }
    e
}

/// What the beacon says about a lobby: nothing once every row is taken. The players are the rows with someone in them,
/// computers too, and the map is its name without the folder and the extension.
fn announce_of(v: &LobbyView, port: u16) -> Option<Announce> {
    let free = v.rows.iter().filter(|r| r.state == RowState::Available).count();
    let taken = v
        .rows
        .iter()
        .filter(|r| matches!(r.state, RowState::Human | RowState::Computer))
        .count();
    let watch = v.observers.saturating_sub(v.watchers.len() as u8);
    if free == 0 && watch == 0 {
        return None;
    }
    let file = v.map_name.rsplit(['\\', '/']).next().unwrap_or("");
    let map = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    Some(
        Announce::ours(
            port,
            taken as u8,
            (taken + free).min(ROWS) as u8,
            &v.rows.first().map(|r| r.name.clone()).unwrap_or_default(),
            map,
        )
        .with_watch(watch),
    )
}

/// The Scenario box's three lines: the type, the file, the description.
fn scenario_lines(m: &MapInfo) -> Vec<String> {
    vec![m.kind.label().to_string(), m.file.clone(), m.description.clone()]
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::input::KeyEvent;
    use crate::net::proto::Identity;
    use crate::net::room::{self, Handshake};

    #[test]
    fn the_extras_rows_cycle_and_summarise() {
        use tidewater_sim::Extras;
        use tidewater_sim::extras::UNLIMITED;
        let mut e = Extras::default();
        assert_eq!(extras_summary(&e, &[]), "None");
        e = cycle_extra(e, 0);
        assert!(e.rally_points);
        for want in [12, 18, UNLIMITED, 0] {
            e = cycle_extra(e, 2);
            assert_eq!(e.selection_limit, want);
        }
        for want in [1, 2, 3, 0] {
            e = cycle_extra(e, 3);
            assert_eq!(e.extra_speeds, want);
        }
        e = cycle_extra(e, 4);
        assert!(e.order_queue);
        assert_eq!(
            extras_summary(&cycle_extra(Extras::default(), 4), &[]),
            "shift-queued orders"
        );
        e = cycle_extra(e, 4);
        e = cycle_extra(e, 0);
        assert!(e.is_default());
        assert_eq!(extras_labels(&e, &[]).len(), 5 + crate::extras::lobby_specs().count());
    }

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

    fn click(l: &mut Lobby, ctx: &mut Ctx, x: i32, y: i32) -> Outcome {
        l.update(
            &Input {
                at: Some((x, y)),
                down: true,
                ..Default::default()
            },
            ctx,
        );
        l.update(
            &Input {
                at: Some((x, y)),
                up: true,
                ..Default::default()
            },
            ctx,
        )
    }

    fn map(path: &str, players: u8) -> MapInfo {
        MapInfo {
            kind: customsetup::MapKind::Custom,
            path: path.into(),
            name: path.into(),
            file: format!("{path}.pud"),
            description: "A test map".into(),
            size: 64,
            era: tidewater_formats::pud::Era::Forest,
            players,
            humans: 2,
            computers: 0,
        }
    }

    /// A small valid map: what every name reads as in these tests.
    fn test_pud() -> Vec<u8> {
        tidewater_formats::pud::build::Map {
            size: 32,
            ..Default::default()
        }
        .bytes()
    }

    fn reader() -> impl FnMut(&str) -> Option<(Vec<u8>, bool)> {
        |_: &str| Some((test_pud(), false))
    }

    fn host_lobby(enhanced: bool, players: u8) -> (Lobby, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let view = room::new_view(enhanced, "Host", race::NEUTRAL);
        let room = HostRoom::new(listener, Identity::ours(1), Texts::default(), view).unwrap();
        let port = room.port();
        let mut read = reader();
        let mut ctx = Ctx::new(&mut read, 5);
        let l = Lobby::new_host(room, vec![map("Maps\\a", players)], None, Texts::default(), &mut ctx);
        (l, port)
    }

    /// A joiner knocks while the creator's lobby runs its frames.
    fn join(l: &mut Lobby, ctx: &mut Ctx, port: u16, name: &str) -> Handshake {
        let addr = format!("127.0.0.1:{port}");
        let name = name.to_string();
        let t = thread::spawn(move || room::handshake(&addr, Identity::ours(1), &name).unwrap());
        while !t.is_finished() {
            l.update(&Input::default(), ctx);
            thread::sleep(Duration::from_millis(5));
        }
        t.join().unwrap()
    }

    /// Runs the lobby's frames until `done` holds.
    fn until(l: &mut Lobby, ctx: &mut Ctx, mut done: impl FnMut(&Lobby) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !done(l) && Instant::now() < end {
            l.update(&Input::default(), ctx);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(done(l), "timed out");
    }

    /// Runs both lobbies' frames until `done` holds.
    fn both(l: &mut Lobby, gl: &mut Lobby, ctx: &mut Ctx, mut done: impl FnMut(&Lobby, &Lobby) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !done(l, gl) && Instant::now() < end {
            l.update(&Input::default(), ctx);
            gl.update(&Input::default(), ctx);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(done(l, gl), "timed out");
    }

    #[test]
    fn the_creators_lobby_opens_with_the_map_the_creator_row_and_start_grey() {
        let (l, _) = host_lobby(true, 4);
        assert_eq!(l.view.map_name, "Maps\\a");
        assert_eq!(l.view.scenario[0], "Custom scenario");
        assert_eq!(l.view.rows[0].state, RowState::Human);
        assert_eq!(l.view.rows[1].state, RowState::Available);
        assert!(!l.button_enabled(T::Start), "the creator alone");
        assert!(l.button_enabled(T::Select));
        assert!(!l.stops().is_empty());
    }

    #[test]
    fn rows_are_set_from_the_drop_down_and_a_joiner_fills_the_lobby() {
        let (mut l, port) = host_lobby(true, 4);
        let mut read = reader();
        let mut ctx = Ctx::new(&mut read, 5);
        // Row 2's drop-down: Computer is the second line.
        click(&mut l, &mut ctx, 100, NAME_Y[2] + 5);
        assert!(l.popup.is_some());
        l.update(&key(Key::Down), &mut ctx);
        l.update(&key(Key::Enter), &mut ctx);
        assert_eq!(l.view.rows[2].state, RowState::Computer);
        assert!(l.button_enabled(T::Start), "the creator and a Computer are enough");
        // A joiner enters row 1.
        let hs = join(&mut l, &mut ctx, port, "Ann");
        assert_eq!(hs.slot, 1);
        let mut g = hs.accept().unwrap();
        until(&mut l, &mut ctx, |l| l.view.rows[1].state == RowState::Human);
        // The joiner's lobby is the creator's.
        let end = Instant::now() + Duration::from_secs(10);
        while g.view.is_none() && Instant::now() < end {
            g.poll();
            thread::sleep(Duration::from_millis(5));
        }
        let mut gl = Lobby::new_guest(g, true);
        both(&mut l, &mut gl, &mut ctx, |_, gl| gl.have_view);
        // It has the map, says so, and then the creator can start.
        both(&mut l, &mut gl, &mut ctx, |l, _| l.view.rows[1].has_map);
        assert!(l.button_enabled(T::Start));
        assert_eq!(gl.view.rows[2].state, RowState::Computer);
        assert!(!gl.button_visible(T::Start), "a joiner has no Start button");
        assert!(!gl.button_enabled(T::Select));
        // The joiner's race and chat reach the creator.
        gl.set_option(T::Race, 1);
        gl.update(&key(Key::Tab), &mut ctx);
        while gl.focus != T::Say {
            gl.update(&key(Key::Tab), &mut ctx);
        }
        gl.update(&type_text("hello"), &mut ctx);
        gl.update(&key(Key::Enter), &mut ctx);
        until(&mut l, &mut ctx, |l| {
            l.view.rows[1].race == race::ORC && l.view.rows[1].say == "hello"
        });
    }

    #[test]
    fn start_with_more_rows_than_the_map_has_slots_says_so_and_a_fitting_game_starts() {
        let (mut l, port) = host_lobby(true, 2);
        let mut read = reader();
        let mut ctx = Ctx::new(&mut read, 5);
        let mut g = join(&mut l, &mut ctx, port, "Ann").accept().unwrap();
        until(&mut l, &mut ctx, |l| l.view.humans() == 2);
        let hash = l.view.map_hash;
        g.set_have_map(hash, true);
        until(&mut l, &mut ctx, |l| l.view.rows[1].has_map);
        // A computer in row 2 makes three rows on a two-slot map.
        if let Some(Backend::Host(h)) = &mut l.back {
            h.set_row(2, RowChoice::Computer);
            l.view = h.view().clone();
        }
        assert!(l.button_enabled(T::Start));
        l.update(&key(Key::Letter('S')), &mut ctx);
        assert_eq!(l.notice.as_deref(), Some("Too many players to load map"));
        l.update(&key(Key::Enter), &mut ctx);
        assert!(l.notice.is_none());
        if let Some(Backend::Host(h)) = &mut l.back {
            h.set_row(2, RowChoice::Available);
            l.view = h.view().clone();
        }
        let o = l.update(&key(Key::Letter('S')), &mut ctx);
        let Outcome::Start(n) = o else {
            panic!("expected a start")
        };
        assert_eq!(n.config.lobby.humans.len(), 2);
        let end = Instant::now() + Duration::from_secs(10);
        let mut got = false;
        while !got && Instant::now() < end {
            got = g.poll().into_iter().any(|e| matches!(e, GuestEvent::Go(_)));
            thread::sleep(Duration::from_millis(5));
        }
        assert!(got);
    }

    #[test]
    fn cancel_game_terminates_the_game_for_the_joiner() {
        let (mut l, port) = host_lobby(true, 4);
        let mut read = reader();
        let mut ctx = Ctx::new(&mut read, 5);
        let g = join(&mut l, &mut ctx, port, "Ann").accept().unwrap();
        let mut gl = Lobby::new_guest(g, true);
        both(&mut l, &mut gl, &mut ctx, |_, gl| gl.have_view);
        let o = l.update(&key(Key::Letter('C')), &mut ctx);
        assert!(matches!(o, Outcome::Cancel(None)));
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            match gl.update(&Input::default(), &mut ctx) {
                Outcome::Cancel(Some(why)) => {
                    assert_eq!(why, Texts::default().terminated);
                    break;
                }
                _ if Instant::now() > end => panic!("no termination"),
                _ => thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    #[test]
    fn a_joiner_without_the_scenario_is_sent_it_and_plays_from_the_copy() {
        let (mut l, port) = host_lobby(true, 4);
        let mut have = reader();
        let mut ctx = Ctx::new(&mut have, 5);
        let g = join(&mut l, &mut ctx, port, "Ann").accept().unwrap();
        let mut gl = Lobby::new_guest(g, true);
        // The joiner has no such map and nowhere it could keep one: it says so, and the creator can't start.
        let mut none = |_: &str| None;
        let mut bare = Ctx::new(&mut none, 5);
        both(&mut l, &mut gl, &mut bare, |l, gl| {
            gl.have_view && l.view.rows[1].state == RowState::Human
        });
        let end = Instant::now() + Duration::from_secs(5);
        while gl.notice.is_none() && Instant::now() < end {
            l.update(&Input::default(), &mut ctx);
            gl.update(&Input::default(), &mut bare);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            gl.notice.as_deref().unwrap().contains("doesn't have"),
            "{:?}",
            gl.notice
        );
        assert!(!l.button_enabled(T::Start), "the joiner lacks the map");
        // Another joiner that can keep a map asks for it and gets it.
        let g2 = join(&mut l, &mut ctx, port, "Bob").accept().unwrap();
        let mut bl = Lobby::new_guest(g2, true);
        let mut none2 = |_: &str| None;
        let mut kept: Vec<(String, u64, usize)> = Vec::new();
        let mut save = |n: &str, h: u64, b: &[u8]| {
            kept.push((n.to_string(), h, b.len()));
            Some("kept\\copy.pud".to_string())
        };
        let mut sending = Ctx::new(&mut none2, 5).with_save(&mut save);
        let end = Instant::now() + Duration::from_secs(10);
        while !l.view.rows[2].has_map && Instant::now() < end {
            l.update(&Input::default(), &mut ctx);
            bl.update(&Input::default(), &mut sending);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(l.view.rows[2].has_map, "the map arrived and Bob said so");
        assert_eq!(bl.local_map.as_deref(), Some("kept\\copy.pud"));
        assert!(bl.transfer.is_none());
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].1, tidewater_replay::start::map_hash(&test_pud()));
        assert_eq!(kept[0].2, test_pud().len());
        // Ann leaves; with only Bob who has the map the creator starts, and Bob is told to play the copy.
        drop(gl);
        let end = Instant::now() + Duration::from_secs(5);
        while l.view.rows[1].state == RowState::Human && Instant::now() < end {
            l.update(&Input::default(), &mut ctx);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(l.button_enabled(T::Start));
        let Outcome::Start(_) = l.update(&key(Key::Letter('S')), &mut ctx) else {
            panic!("expected a start")
        };
        let mut sending = Ctx::new(&mut none2, 5);
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            match bl.update(&Input::default(), &mut sending) {
                Outcome::Start(n) => {
                    assert_eq!(
                        n.config.map_name, "kept\\copy.pud",
                        "the creator's name for the map is not used here"
                    );
                    break;
                }
                _ if Instant::now() > end => panic!("no start"),
                _ => thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    #[test]
    fn classic_mode_shows_ready_boxes_and_cheat_codes_and_enhanced_a_speed_slider() {
        let (mut l, _) = host_lobby(false, 4);
        assert!(!l.room_enhanced());
        assert_eq!(l.choices(T::Resources).len(), 4, "Classic mode has no Random");
        assert_eq!(l.choices(T::Tileset).len(), 5);
        let t = l.targets();
        assert!(t.iter().any(|(c, _)| *c == T::Cheats));
        assert!(!t.iter().any(|(c, _)| *c == T::SpeedMore));
        let (mut e, _) = host_lobby(true, 4);
        assert_eq!(e.choices(T::Resources).len(), 5);
        assert_eq!(e.view.speed, 4, "the slider opens at the middle");
        let mut read = reader();
        let mut ctx = Ctx::new(&mut read, 5);
        for _ in 0..9 {
            e.activate(T::SpeedMore, &mut ctx);
        }
        assert_eq!(e.view.speed, 8);
        e.activate(T::SpeedLess, &mut ctx);
        assert_eq!(e.view.speed, 7);
        l.set_option(T::Cheats, 1);
        assert!(!l.view.cheats_allowed);
    }

    #[test]
    fn options_change_the_game_the_lobby_makes() {
        let (mut l, _) = host_lobby(true, 4);
        l.set_option(T::Fog, 1);
        l.set_option(T::Location, 1);
        l.set_option(T::Units, 1);
        l.set_option(T::Resources, 3);
        assert!(!l.view.custom.fog_of_war);
        assert!(l.view.custom.fixed_starts);
        assert!(l.view.custom.one_worker);
        let (config, _) = l.view.to_config(1);
        assert!(config.lobby.fixed_order);
    }

    #[test]
    fn the_screen_draws_without_panicking_in_every_state() {
        // Drawing needs the installation's font and palette; the frame snapshots cover the pixels. Here: the
        // geometry is on the screen and the controls are reachable.
        for r in (0..ROWS).map(name_rect).chain((0..ROWS).map(chat_rect)) {
            assert!(r.x >= 0 && r.y >= 0 && r.x + r.w as i32 <= 640 && r.y + r.h as i32 <= 480);
        }
        for t in [
            T::Race,
            T::Fog,
            T::Cheats,
            T::Resources,
            T::Tileset,
            T::Location,
            T::Units,
        ] {
            let r = drop_rect(t);
            assert!(r.x + r.w as i32 <= 640 && r.y + r.h as i32 <= 400);
        }
    }

    #[test]
    fn the_extras_list_has_the_c_plus_l_rows_and_the_creator_cycles_them_for_everyone() {
        let (mut l, port) = host_lobby(true, 4);
        let mut read = reader();
        let mut ctx = Ctx::new(&mut read, 5);
        let labels = l.choices(T::Extras);
        let lobby_rows = crate::extras::lobby_specs().count();
        assert!(
            lobby_rows >= 3,
            "health bars, smart attack-move and widescreen at least"
        );
        assert_eq!(labels.len(), SIM_EXTRAS + lobby_rows);
        assert!(labels[SIM_EXTRAS].starts_with("Health bars: "), "{labels:?}");
        assert!(!l.extras_box(), "a lobby with no option on looks as it always has");
        // The creator turns health bars on (the first C+L row): the box shows, and the room's view carries it.
        l.set_option(T::Extras, SIM_EXTRAS);
        assert!(l.extras_box());
        assert_eq!(l.view.client_extras, [("health-bars".to_string(), 1)]);
        assert!(extras_summary(&l.view.extras, &l.view.client_extras).contains("health-bars hovered"));
        // A joiner sees it, read-only, and its own settings are not asked.
        let g = join(&mut l, &mut ctx, port, "Ann").accept().unwrap();
        let mut gl = Lobby::new_guest(g, true);
        both(&mut l, &mut gl, &mut ctx, |_, gl| gl.have_view);
        both(&mut l, &mut gl, &mut ctx, |_, gl| gl.view.client_extras == l_values());
        assert!(
            gl.extras_box(),
            "the joiner sees the box once the creator's lobby has an option on"
        );
        assert!(!gl.stops().contains(&T::Extras) && !gl.stops().contains(&T::Observers));
        gl.set_option(T::Extras, SIM_EXTRAS + 1);
        assert_eq!(gl.view.client_extras, l_values(), "only the creator changes them");
    }

    fn l_values() -> Vec<(String, u8)> {
        vec![("health-bars".to_string(), 1)]
    }

    #[test]
    fn the_creator_allows_observers_and_everyone_sees_who_watches() {
        let (mut l, port) = host_lobby(true, 4);
        let mut read = reader();
        let mut ctx = Ctx::new(&mut read, 5);
        assert!(l.observers_box(), "the creator always has the control");
        assert_eq!(l.choices(T::Observers).len(), MAX_OBSERVERS + 1);
        assert_eq!(l.choices(T::Observers)[0], "Observers: None");
        assert_eq!(l.view.observers, 0, "none by default");
        l.set_option(T::Observers, 2);
        assert_eq!(l.view.observers, 2);
        // A player joins; its screen shows the control (read-only) because the game has places.
        let g = join(&mut l, &mut ctx, port, "Ann").accept().unwrap();
        let mut gl = Lobby::new_guest(g, true);
        both(&mut l, &mut gl, &mut ctx, |_, gl| {
            gl.have_view && gl.view.observers == 2
        });
        assert!(gl.observers_box());
        assert!(!gl.watching());
        // An observer joins from its own menu: no row, no race, no chat, and everyone lists it.
        let addr = format!("127.0.0.1:{port}");
        let t = thread::spawn(move || room::handshake_as(&addr, Identity::ours(1), "Eve", true).unwrap());
        while !t.is_finished() {
            l.update(&Input::default(), &mut ctx);
            thread::sleep(Duration::from_millis(5));
        }
        let mut el = Lobby::new_guest(t.join().unwrap().accept().unwrap(), true);
        assert!(el.watching());
        both(&mut l, &mut el, &mut ctx, |l, el| {
            el.have_view && l.view.watchers == ["Eve"]
        });
        both(&mut l, &mut gl, &mut ctx, |_, gl| gl.view.watchers == ["Eve"]);
        assert!(l.view.rows.iter().all(|r| r.name != "Eve"));
        assert_eq!(l.view.humans(), 2);
        let stops = el.stops();
        assert!(!stops.contains(&T::Say) && !stops.contains(&T::Race) && !stops.contains(&T::Ready(2)));
        assert!(stops.contains(&T::Cancel));
        assert!(el.targets().iter().all(|(t, _)| !matches!(t, T::Say | T::Race)));
        // The beacon says how many places are still free.
        assert_eq!(announce_of(&l.view, 6112).unwrap().watch, 1);
        // Lowering the places sends the watcher away.
        l.set_option(T::Observers, 0);
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            l.update(&Input::default(), &mut ctx);
            if matches!(el.update(&Input::default(), &mut ctx), Outcome::Cancel(Some(_))) {
                break;
            }
            assert!(Instant::now() < end, "the watcher was not sent away");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_beacon_counts_taken_rows_names_the_map_and_stops_when_full() {
        let mut v = crate::net::room::new_view(true, "Ann", race::HUMAN);
        v.map_name = "Maps\\Gold River.pud".into();
        v.rows[1].state = RowState::Computer;
        v.rows[2].state = RowState::Closed;
        let a = announce_of(&v, 6112).unwrap();
        assert_eq!((a.players, a.max, a.tcp_port), (2, 7, 6112));
        assert_eq!((a.owner.as_str(), a.map.as_str()), ("Ann", "Gold River"));
        for r in v.rows.iter_mut() {
            if r.state == RowState::Available {
                r.state = RowState::Closed;
            }
        }
        assert!(announce_of(&v, 6112).is_none(), "no free row: nothing to join");
        v.observers = 2;
        v.watchers = vec!["Eve".into()];
        let a = announce_of(&v, 6112).unwrap();
        assert_eq!(a.watch, 1, "a free place to watch is worth announcing");
        v.watchers.push("Bob".into());
        assert!(announce_of(&v, 6112).is_none());
    }
}
