//! The lobby's network side: a room that joiners enter, change their race, ready box and chat line in, and that the
//! host fills with Computer rows, closes, kicks from and starts (`docs/design/network.md`, "The lobby";
//! `docs/specs/ui/game-setup-lobby.md`).
//!
//! Unlike the blocking handshake of `tcp::host_game`, a room never blocks: the screens poll it each frame. The host's
//! [`HostRoom`] owns the listening socket and the joiners' connections and holds the one true [`LobbyView`]; every
//! change goes out to every joiner as a whole new view (it is a few hundred bytes). A joiner's [`GuestRoom`] holds the
//! last view it was sent and can change only its own row's race, ready box and chat line.
//!
//! **Handshake.** A connection is accepted and a short-lived thread reads its [`Msg::Hello`]. The host checks the
//! [`Identity`] (a refusal says what differs), the name (no duplicates) and that a row is free, reserves the row and
//! answers [`Msg::Welcome`]; the joiner answers [`Msg::Ready`]; the host then fills the row and sends the joiner the
//! lobby. A joiner that vanishes in between frees its row again.
//!
//! **The map.** A joiner says whether it has the lobby's map ([`Msg::HaveMap`], by hash) each time the lobby's map
//! changes, and asks for it ([`Msg::MapRequest`]) when it lacks it. The host sends the map it holds in chunks
//! ([`Msg::MapOffer`], [`Msg::MapChunk`]); the joiner takes it only if it is no bigger than [`MAX_MAP_BYTES`], arrives
//! in order, hashes to what was asked for and parses as a map. The host starts no game while a joiner lacks the map.
//!
//! **Starting.** [`HostRoom::start`] packs the filled rows into slots (Available and Closed rows are skipped, so a
//! slot never lands on a map slot nobody fills), settles Random races from the game's seed, and sends each joiner
//! [`Msg::Go`] with its slot and the game. The connections then become the turn engine's wires (`tcp::HostWire`,
//! `tcp::ClientWire`), so no message already queued is lost.

use std::io;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::{Result, bail};
use tidewater_sim::setup::{Custom, Game, Lobby, Setup, race};

use super::lobby::NetStart;
use super::proto::{
    GameConfig, HelloRead, Identity, LobbyView, MAP_CHUNK, MAX_MAP_BYTES, MAX_OBSERVERS, Msg, OBSERVER_BASE, RowState,
    RowView, read_frame, read_hello, write_frame,
};
use super::tcp::{self, ClientWire, Event, HANDSHAKE_TIMEOUT, HostWire, Link};

/// What a joiner that asks to observe is told by a lobby that has no place for one (or none free).
const NO_OBSERVERS: &str = "This game has no place for an observer.";

/// The lobby's rows.
pub const ROWS: usize = 8;
/// The longest line in a row's chat box.
pub const SAY_MAX: usize = 70;

/// The words of the refusals and notices the host sends (network table entries 6, 7, 8, 21 and 89).
#[derive(Clone, Debug)]
pub struct Texts {
    pub full: String,
    pub terminated: String,
    pub kicked: String,
    pub too_many: String,
    pub duplicate_name: String,
}

impl Texts {
    /// The words from the network table (`entry`), the spec's where the table gives none.
    pub fn from(entry: &dyn Fn(u16) -> String) -> Texts {
        let d = Texts::default();
        let get = |n: u16, fallback: String| {
            let s = entry(n);
            if s.is_empty() { fallback } else { s }
        };
        Texts {
            full: get(6, d.full),
            terminated: get(7, d.terminated),
            kicked: get(8, d.kicked),
            too_many: get(21, d.too_many),
            duplicate_name: get(89, d.duplicate_name),
        }
    }
}

impl Default for Texts {
    fn default() -> Self {
        Texts {
            full: "The requested game is full.".into(),
            terminated: "Game was terminated by the game creator.".into(),
            kicked: "You were kicked out by the game creator.".into(),
            too_many: "Too many players to load map".into(),
            duplicate_name: "Duplicate name - pick another".into(),
        }
    }
}

/// An empty lobby: the host in row 0, every other row Available.
pub fn new_view(enhanced: bool, host_name: &str, host_race: u8) -> LobbyView {
    let mut rows: Vec<RowView> = (0..ROWS).map(|_| RowView::new(RowState::Available)).collect();
    rows[0] = RowView {
        state: RowState::Human,
        name: host_name.to_string(),
        race: host_race,
        ready: true,
        say: String::new(),
        has_map: true,
    };
    LobbyView {
        enhanced,
        map_name: String::new(),
        map_hash: 0,
        scenario: Vec::new(),
        map_players: 0,
        custom: Custom::default(),
        // The slider's middle (`ui/game-setup-lobby.md`, "Options").
        speed: 4,
        cheats_allowed: true,
        row: super::lockstep::DIRECT_IP_ROW,
        extras: tidewater_sim::Extras::default(),
        client_extras: Vec::new(),
        observers: 0,
        watchers: Vec::new(),
        rows,
    }
}

impl LobbyView {
    /// The rows that play, in order: Human and Computer rows. A game's slot `n` is the `n`th of them.
    pub fn participants(&self) -> Vec<usize> {
        (0..self.rows.len())
            .filter(|&i| matches!(self.rows[i].state, RowState::Human | RowState::Computer))
            .collect()
    }

    pub fn humans(&self) -> usize {
        self.rows.iter().filter(|r| r.state == RowState::Human).count()
    }

    /// The game this lobby makes, with the Random races settled from `seed`, and each slot's row. `local` is the
    /// receiver's slot, set by the caller.
    pub fn to_config(&self, seed: u32) -> (GameConfig, Vec<usize>) {
        let rows = self.participants();
        let mut humans = Vec::new();
        let mut computers = Vec::new();
        let mut names = Vec::new();
        let mut randoms = 0u32;
        for (slot, &row) in rows.iter().enumerate() {
            let r = &self.rows[row];
            match r.state {
                RowState::Human => {
                    let race = if r.race == race::NEUTRAL {
                        randoms += 1;
                        random_race(seed, randoms)
                    } else {
                        r.race
                    };
                    humans.push((slot as u8, race));
                    names.push(r.name.clone());
                }
                _ => {
                    computers.push(slot as u8);
                    names.push(String::new());
                }
            }
        }
        let custom = Custom {
            race: None,
            opponents: None,
            ..self.custom
        };
        let config = GameConfig {
            map_name: self.map_name.clone(),
            map_hash: self.map_hash,
            seed,
            setup: Setup {
                game: Game::Custom(custom),
                enhanced: self.enhanced,
            },
            lobby: Lobby {
                humans,
                local: 0,
                computers,
                fixed_order: self.custom.fixed_starts,
            },
            speed: self.speed,
            row: self.row,
            extras: self.extras,
            client_extras: self.client_extras.clone(),
            names,
        };
        (config, rows)
    }
}

/// A Random race, decided once from the game's seed so every machine plays the same races (`ui/game-setup-lobby.md`,
/// "Races and colours": the original flips a coin per machine and shares the result; ours is the host's one draw).
fn random_race(seed: u32, nth: u32) -> u8 {
    let h = seed.wrapping_mul(0x9E37_79B1).rotate_left(5 * nth) ^ nth.wrapping_mul(0x85EB_CA6B);
    if (h >> 7) & 1 == 0 { race::HUMAN } else { race::ORC }
}

/// What the host's drop-down on a row offers (Kick Out is Available on a row that holds a human).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowChoice {
    Available,
    Computer,
    Closed,
    KickOut,
}

/// Why the host cannot start yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    /// No scenario is chosen.
    NoMap,
    /// The creator is the only participant (no joiner, no Computer row). Gap-fill for `ui/game-setup-lobby.md` trace
    /// request LB1: a game needs at least two participants; Available rows are ignored.
    Alone,
    /// More rows are filled than the map has slots (network entry 21).
    TooMany,
    /// Classic mode: a joiner has not ticked its Ready box.
    NotReady,
    /// A joiner does not have the map yet (it may be on its way).
    MissingMap,
}

// ---------------------------------------------------------------------------------------------------------------
// The host

enum Shake {
    Hello {
        id: usize,
        identity: Box<Identity>,
        name: String,
        observer: bool,
        stream: TcpStream,
    },
    Ready {
        id: usize,
        stream: TcpStream,
    },
    Gone {
        id: usize,
    },
}

/// Where a connected joiner sits: a row of the lobby, or a place to watch from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seat {
    Row(usize),
    Watch,
}

struct Member {
    id: usize,
    seat: Seat,
    /// The joiner's name (an observer's is only kept here and in [`LobbyView::watchers`]).
    name: String,
    /// An observer has the lobby's map (a row says so in the view).
    has_map: bool,
    link: Link,
    /// Map requests answered for the current map: a joiner can't make the host send it over and over.
    requests: u8,
}

/// How many times one joiner may ask for one map.
const MAP_REQUESTS: u8 = 3;

struct Reserved {
    id: usize,
    /// The row held for the joiner; none for an observer, who holds a place to watch from.
    row: Option<usize>,
    name: String,
}

/// The game being set up on the host's machine.
pub struct HostRoom {
    listener: TcpListener,
    identity: Identity,
    texts: Texts,
    view: LobbyView,
    shakes: Receiver<Shake>,
    shake_tx: Sender<Shake>,
    events_tx: Sender<Event>,
    events: Receiver<Event>,
    members: Vec<Member>,
    reserved: Vec<Reserved>,
    next_id: usize,
    /// The lobby's map file, to send to a joiner that lacks it.
    map_data: Option<Arc<Vec<u8>>>,
}

impl HostRoom {
    /// Opens a room on `listener`, which this takes over. `identity` is what joiners must match.
    pub fn new(listener: TcpListener, identity: Identity, texts: Texts, view: LobbyView) -> io::Result<HostRoom> {
        listener.set_nonblocking(true)?;
        let (shake_tx, shakes) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        Ok(HostRoom {
            listener,
            identity,
            texts,
            view,
            shakes,
            shake_tx,
            events_tx,
            events,
            members: Vec::new(),
            reserved: Vec::new(),
            next_id: 0,
            map_data: None,
        })
    }

    /// The bytes of the lobby's map, for a joiner that lacks it. Maps over [`MAX_MAP_BYTES`] are never sent.
    pub fn set_map_data(&mut self, bytes: Vec<u8>) {
        self.map_data = Some(Arc::new(bytes));
    }

    pub fn view(&self) -> &LobbyView {
        &self.view
    }

    /// The port the room listens on.
    pub fn port(&self) -> u16 {
        self.listener.local_addr().map_or(0, |a| a.port())
    }

    fn broadcast(&self) {
        let msg = Msg::Lobby(Box::new(self.view.clone()));
        for m in &self.members {
            m.link.send(&msg);
        }
    }

    /// Sends the lobby to every joiner after the host changed it, and clears the Classic Ready boxes when `clears`
    /// (a changed map or option).
    fn changed(&mut self, clears: bool) {
        if clears && !self.view.enhanced {
            for r in self.view.rows.iter_mut().skip(1).filter(|r| r.state == RowState::Human) {
                r.ready = false;
            }
        }
        self.broadcast();
    }

    /// Picks the scenario. The Classic Ready boxes clear.
    pub fn set_map(&mut self, name: &str, hash: u64, scenario: Vec<String>, players: u8) {
        let v = &mut self.view;
        if v.map_name == name && v.map_hash == hash && v.scenario == scenario && v.map_players == players {
            return;
        }
        let new_map = v.map_hash != hash;
        v.map_name = name.to_string();
        v.map_hash = hash;
        v.scenario = scenario;
        v.map_players = players;
        if new_map {
            // Every joiner is asked again whether it has this map.
            for r in v.rows.iter_mut().skip(1) {
                r.has_map = false;
            }
            for m in &mut self.members {
                m.requests = 0;
                m.has_map = false;
            }
            self.map_data = None;
        }
        self.changed(true);
    }

    /// Sets the options. The Classic Ready boxes clear when something changed.
    pub fn set_options(&mut self, custom: Custom, speed: u8, cheats_allowed: bool) {
        let v = &mut self.view;
        if v.custom == custom && v.speed == speed && v.cheats_allowed == cheats_allowed {
            return;
        }
        v.custom = custom;
        v.speed = speed.min(8);
        v.cheats_allowed = cheats_allowed;
        self.changed(true);
    }

    /// The optional improvements that change the simulation (`docs/design/extras.md`), the host's choice for everyone.
    pub fn set_extras(&mut self, extras: tidewater_sim::Extras) {
        if self.view.extras != extras {
            self.view.extras = extras;
            self.changed(true);
        }
    }

    /// The C+L options (`crate::extras`) every player plays with, the host's choice: the values that are not the default,
    /// by name (`ClientExtras::lobby_values`).
    pub fn set_client_extras(&mut self, values: Vec<(String, u8)>) {
        if self.view.client_extras != values {
            self.view.client_extras = values;
            self.changed(true);
        }
    }

    /// How many observers the game takes (0 to [`MAX_OBSERVERS`]; none by default). Observers beyond the new number
    /// that are already in are sent away, the last to come first.
    pub fn set_observers(&mut self, places: u8) {
        let places = places.min(MAX_OBSERVERS as u8);
        if self.view.observers == places {
            return;
        }
        self.view.observers = places;
        while self.members.iter().filter(|m| m.seat == Seat::Watch).count() > usize::from(places) {
            let Some(i) = self.members.iter().rposition(|m| m.seat == Seat::Watch) else {
                break;
            };
            let m = self.members.remove(i);
            m.link.send(&Msg::Refuse {
                reason: self.texts.kicked.clone(),
            });
        }
        self.refresh_watchers();
        self.broadcast();
    }

    /// The names of the observers in the view, in the order they came.
    fn refresh_watchers(&mut self) {
        self.view.watchers = self
            .members
            .iter()
            .filter(|m| m.seat == Seat::Watch)
            .map(|m| m.name.clone())
            .collect();
    }

    /// The host's own race.
    pub fn set_host_race(&mut self, race: u8) {
        if race <= tidewater_sim::setup::race::NEUTRAL && self.view.rows[0].race != race {
            self.view.rows[0].race = race;
            self.changed(false);
        }
    }

    /// The line in the host's chat box.
    pub fn say(&mut self, text: &str) {
        self.view.rows[0].say = clean_say(text);
        self.changed(false);
    }

    /// The host's drop-down on a row 1 to 7. Anything but Available on a row that holds a human kicks that human.
    pub fn set_row(&mut self, row: usize, choice: RowChoice) {
        if row == 0 || row >= ROWS {
            return;
        }
        if self.view.rows[row].state == RowState::Human {
            self.kick(row);
        }
        let state = match choice {
            RowChoice::Available | RowChoice::KickOut => RowState::Available,
            RowChoice::Computer => RowState::Computer,
            RowChoice::Closed => RowState::Closed,
        };
        // A joiner being welcomed into the row loses it.
        self.reserved.retain(|r| r.row != Some(row));
        self.view.rows[row] = RowView::new(state);
        self.changed(false);
    }

    fn kick(&mut self, row: usize) {
        if let Some(i) = self.members.iter().position(|m| m.seat == Seat::Row(row)) {
            let m = self.members.remove(i);
            m.link.send(&Msg::Refuse {
                reason: self.texts.kicked.clone(),
            });
        }
    }

    /// Whether the game can start, and if not, why.
    pub fn can_start(&self) -> Result<(), StartError> {
        let v = &self.view;
        if v.map_name.is_empty() {
            return Err(StartError::NoMap);
        }
        if v.participants().len() < 2 {
            return Err(StartError::Alone);
        }
        if v.participants().len() > usize::from(v.map_players) {
            return Err(StartError::TooMany);
        }
        if v.rows.iter().skip(1).any(|r| r.state == RowState::Human && !r.has_map)
            || self.members.iter().any(|m| m.seat == Seat::Watch && !m.has_map)
        {
            return Err(StartError::MissingMap);
        }
        if !v.enhanced && v.rows.iter().skip(1).any(|r| r.state == RowState::Human && !r.ready) {
            return Err(StartError::NotReady);
        }
        Ok(())
    }

    /// Everything that happened since the last call: connections accepted and answered, joiners' changes, joiners
    /// gone. Returns whether the lobby changed (the screen draws it again).
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        // New connections.
        while let Ok((stream, _)) = self.listener.accept() {
            let id = self.next_id;
            self.next_id += 1;
            let tx = self.shake_tx.clone();
            let ours = self.identity.protocol;
            let _ = thread::Builder::new().name(format!("net-hello-{id}")).spawn(move || {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
                let _ = stream.set_nodelay(true);
                let mut stream = stream;
                match read_hello(&mut stream) {
                    Ok(HelloRead::Hello {
                        identity,
                        name,
                        observer,
                    }) => {
                        let _ = tx.send(Shake::Hello {
                            id,
                            identity: Box::new(identity),
                            name,
                            observer,
                            stream,
                        });
                    }
                    Ok(HelloRead::OtherProtocol(p)) => {
                        // Its layout is unknown: it is told why, by the version alone.
                        let _ = write_frame(
                            &mut stream,
                            &Msg::Refuse {
                                reason: Identity::protocol_reason(ours, p),
                            },
                        );
                        let _ = tx.send(Shake::Gone { id });
                    }
                    _ => {
                        let _ = tx.send(Shake::Gone { id });
                    }
                }
            });
        }
        while let Ok(s) = self.shakes.try_recv() {
            changed |= self.shake(s);
        }
        while let Ok(ev) = self.events.try_recv() {
            changed |= self.event(ev);
        }
        changed
    }

    fn name_taken(&self, name: &str) -> bool {
        let same = |n: &str| n.eq_ignore_ascii_case(name);
        self.view
            .rows
            .iter()
            .any(|r| r.state == RowState::Human && same(&r.name))
            || self.members.iter().any(|m| m.seat == Seat::Watch && same(&m.name))
            || self.reserved.iter().any(|r| same(&r.name))
    }

    fn shake(&mut self, s: Shake) -> bool {
        match s {
            Shake::Hello {
                id,
                identity,
                name,
                observer,
                mut stream,
            } => {
                let refuse = |stream: &mut TcpStream, reason: String| {
                    let _ = write_frame(stream, &Msg::Refuse { reason });
                };
                if let Some(why) = self.identity.mismatch(&identity) {
                    refuse(&mut stream, why);
                    return false;
                }
                // An observer takes no row: it needs one of the places the host allowed (`docs/design/network.md`,
                // "Observers"), and starts from the host's view.
                let (row, slot) = if observer {
                    let taken = self.members.iter().filter(|m| m.seat == Seat::Watch).count()
                        + self.reserved.iter().filter(|r| r.row.is_none()).count();
                    if taken >= usize::from(self.view.observers) {
                        refuse(&mut stream, NO_OBSERVERS.to_string());
                        return false;
                    }
                    (None, OBSERVER_BASE + taken as u8)
                } else {
                    let free = (1..ROWS).find(|&i| {
                        self.view.rows[i].state == RowState::Available
                            && !self.reserved.iter().any(|r| r.row == Some(i))
                    });
                    let Some(row) = free else {
                        refuse(&mut stream, self.texts.full.clone());
                        return false;
                    };
                    (Some(row), row as u8)
                };
                let fallback = match row {
                    Some(row) => format!("Player {}", row + 1),
                    None => format!("Observer {}", slot - OBSERVER_BASE + 1),
                };
                let name = tcp::clean_name(&name).unwrap_or(fallback);
                if self.name_taken(&name) {
                    refuse(&mut stream, self.texts.duplicate_name.clone());
                    return false;
                }
                let (mut config, _) = self.view.to_config(0);
                config.lobby.local = slot;
                let welcome = Msg::Welcome {
                    slot,
                    config: Box::new(config),
                };
                if write_frame(&mut stream, &welcome).is_err() {
                    return false;
                }
                self.reserved.push(Reserved { id, row, name });
                let tx = self.shake_tx.clone();
                let _ = thread::Builder::new().name(format!("net-ready-{id}")).spawn(move || {
                    let mut stream = stream;
                    match read_frame(&mut stream) {
                        Ok(Msg::Ready) => {
                            let _ = tx.send(Shake::Ready { id, stream });
                        }
                        _ => {
                            let _ = tx.send(Shake::Gone { id });
                        }
                    }
                });
                false
            }
            Shake::Ready { id, stream } => {
                let Some(i) = self.reserved.iter().position(|r| r.id == id) else {
                    return false;
                };
                let r = self.reserved.remove(i);
                let Ok(link) = Link::spawn(stream, id, self.events_tx.clone()) else {
                    return false;
                };
                let Some(row) = r.row else {
                    self.members.push(Member {
                        id,
                        seat: Seat::Watch,
                        name: r.name,
                        has_map: false,
                        link,
                        requests: 0,
                    });
                    self.refresh_watchers();
                    self.broadcast();
                    return true;
                };
                self.view.rows[row] = RowView {
                    state: RowState::Human,
                    name: r.name.clone(),
                    race: race::NEUTRAL,
                    // Enhanced rules have no Ready box: a joiner is ready the moment it joins.
                    ready: self.view.enhanced,
                    say: String::new(),
                    // Until the joiner says it has the map.
                    has_map: false,
                };
                self.members.push(Member {
                    id,
                    seat: Seat::Row(row),
                    name: r.name,
                    has_map: false,
                    link,
                    requests: 0,
                });
                self.broadcast();
                true
            }
            Shake::Gone { id } => {
                self.reserved.retain(|r| r.id != id);
                false
            }
        }
    }

    fn event(&mut self, ev: Event) -> bool {
        match ev {
            Event::Closed(id) => {
                let Some(i) = self.members.iter().position(|m| m.id == id) else {
                    return false;
                };
                let m = self.members.remove(i);
                match m.seat {
                    Seat::Row(row) => self.view.rows[row] = RowView::new(RowState::Available),
                    Seat::Watch => self.refresh_watchers(),
                }
                self.broadcast();
                true
            }
            Event::Msg(id, msg) => {
                let Some(seat) = self.members.iter().find(|m| m.id == id).map(|m| m.seat) else {
                    return false;
                };
                match msg {
                    Msg::HaveMap { hash, have } => {
                        let has = have && hash == self.view.map_hash && !self.view.map_name.is_empty();
                        match seat {
                            Seat::Row(row) => {
                                if self.view.rows[row].has_map == has {
                                    return false;
                                }
                                self.view.rows[row].has_map = has;
                            }
                            Seat::Watch => {
                                let Some(m) = self.members.iter_mut().find(|m| m.id == id) else {
                                    return false;
                                };
                                if m.has_map == has {
                                    return false;
                                }
                                m.has_map = has;
                            }
                        }
                        self.broadcast();
                        return true;
                    }
                    Msg::MapRequest { hash } => {
                        self.send_map(id, hash);
                        return false;
                    }
                    _ => {}
                }
                // An observer has no row to change.
                let Seat::Row(row) = seat else { return false };
                let r = &mut self.view.rows[row];
                match msg {
                    Msg::SetRace { race } if race <= tidewater_sim::setup::race::NEUTRAL => r.race = race,
                    Msg::SetReady { ready } if !self.view.enhanced => r.ready = ready,
                    Msg::Say { text } => r.say = clean_say(&text),
                    _ => return false,
                }
                self.broadcast();
                true
            }
        }
    }

    /// Answers a joiner's request for the map `hash`: the map in chunks, or why not.
    fn send_map(&mut self, id: usize, hash: u64) {
        let Some(m) = self.members.iter_mut().find(|m| m.id == id) else {
            return;
        };
        let denied = |reason: &str| Msg::MapDenied {
            hash,
            reason: reason.to_string(),
        };
        let data = match &self.map_data {
            Some(d) if hash == self.view.map_hash && !d.is_empty() && d.len() <= MAX_MAP_BYTES => d.clone(),
            Some(d) if d.len() > MAX_MAP_BYTES => {
                m.link.send(&denied("The map is too big to send."));
                return;
            }
            _ => {
                m.link.send(&denied("The game creator can't send this map."));
                return;
            }
        };
        if m.requests >= MAP_REQUESTS {
            m.link.send(&denied("The map was already sent to you."));
            return;
        }
        m.requests += 1;
        m.link.send(&Msg::MapOffer {
            hash,
            size: data.len() as u32,
        });
        for (i, chunk) in data.chunks(MAP_CHUNK).enumerate() {
            m.link.send(&Msg::MapChunk {
                hash,
                offset: (i * MAP_CHUNK) as u32,
                bytes: chunk.to_vec(),
            });
        }
    }

    /// Closes the room: every joiner is told the game was terminated.
    pub fn cancel(self) {
        for m in &self.members {
            m.link.send(&Msg::Refuse {
                reason: self.texts.terminated.clone(),
            });
        }
    }

    /// Starts the game: every joiner is sent its slot and the game, and the connections become the turn engine's
    /// wire. `seed` is the game's seed (the host's clock).
    pub fn start(mut self, seed: u32) -> Result<NetStart> {
        if let Err(e) = self.can_start() {
            bail!("the game can't start yet: {e:?}");
        }
        // A joiner that left in this very frame is noticed before the game is built.
        self.poll();
        self.can_start()
            .map_err(|e| anyhow::anyhow!("the game can't start yet: {e:?}"))?;
        let (config, rows) = self.view.to_config(seed);
        let mut wires = Vec::new();
        let mut watchers = 0u8;
        for m in self.members {
            let (slot, view_of) = match m.seat {
                Seat::Row(row) => {
                    let Some(slot) = rows.iter().position(|&r| r == row) else {
                        continue;
                    };
                    (slot as u8, slot as u8)
                }
                // An observer's slot only names its connection; it looks at the game from the host's place.
                Seat::Watch => {
                    watchers += 1;
                    (
                        OBSERVER_BASE + watchers - 1,
                        config.lobby.humans.first().map_or(0, |h| h.0),
                    )
                }
            };
            let mut theirs = config.clone();
            theirs.lobby.local = view_of;
            m.link.send(&Msg::Go {
                slot,
                config: Box::new(theirs),
            });
            wires.push((slot, m.id, m.link));
        }
        let names = config.names.clone();
        let wire = HostWire::from_parts(wires, self.events, names.clone());
        Ok(NetStart {
            config,
            me: 0,
            wire: Box::new(wire),
            names,
        })
    }
}

/// A chat line as it is kept: printable text, [`SAY_MAX`] characters.
fn clean_say(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(SAY_MAX).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// The joiner

/// A joiner that has been welcomed: it knows the game and its row, and must say whether it can play it.
#[derive(Debug)]
pub struct Handshake {
    stream: TcpStream,
    /// The joiner's row, or an observer's number from [`OBSERVER_BASE`] up.
    pub slot: u8,
    /// The joiner watches rather than plays.
    pub observer: bool,
    /// The game as the host showed it: the map to check for.
    pub config: GameConfig,
}

/// Connects to the game at `addr` and says hello. Blocks until the host welcomes or refuses; run it on a thread.
pub fn handshake(addr: &str, identity: Identity, name: &str) -> Result<Handshake> {
    handshake_as(addr, identity, name, false)
}

/// [`handshake`] as an observer when `observer` is set: the host must have a free place to watch from.
pub fn handshake_as(addr: &str, identity: Identity, name: &str, observer: bool) -> Result<Handshake> {
    let mut stream = tcp::connect(addr)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_nodelay(true)?;
    write_frame(
        &mut stream,
        &Msg::Hello {
            identity,
            name: name.to_string(),
            observer,
        },
    )?;
    match read_frame(&mut stream) {
        Ok(Msg::Welcome { slot, config }) => Ok(Handshake {
            stream,
            slot,
            observer,
            config: *config,
        }),
        Ok(Msg::Refuse { reason }) => bail!("{reason}"),
        Ok(_) => bail!("the host sent something unexpected: is that a Tidewater game?"),
        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
            bail!("the host's reply didn't make sense: it may be a different version of the program")
        }
        Err(e) => bail!("the host went away before showing the game: {e}"),
    }
}

impl Handshake {
    /// This machine can play the game: say so and enter the room.
    pub fn accept(mut self) -> io::Result<GuestRoom> {
        write_frame(&mut self.stream, &Msg::Ready)?;
        self.stream.set_read_timeout(None)?;
        let (tx, rx) = mpsc::channel();
        let link = Link::spawn(self.stream, 0, tx)?;
        Ok(GuestRoom {
            parts: Some((link, rx)),
            incoming: None,
            slot: self.slot,
            observer: self.observer,
            view: None,
        })
    }

    /// This machine can't play the game: tell the host why and leave.
    pub fn decline(mut self, reason: &str) {
        let _ = write_frame(
            &mut self.stream,
            &Msg::Refuse {
                reason: reason.to_string(),
            },
        );
    }
}

/// What a joiner's room tells its screen.
pub enum GuestEvent {
    /// The lobby changed (the first one is the lobby as the joiner enters).
    View,
    /// The host started the game.
    Go(Box<NetStart>),
    /// Part of the requested map has arrived: `got` of `size` bytes.
    MapProgress { got: usize, size: usize },
    /// The requested map has arrived whole, hashes to what was asked for and parses.
    MapReceived { hash: u64, bytes: Vec<u8> },
    /// The map will not come (refused, too big, damaged), and why.
    MapFailed(String),
    /// The room is over: kicked out, terminated, or the connection ended. The words are for the player.
    Ended(String),
}

/// The joiner's end of a room.
pub struct GuestRoom {
    parts: Option<(Link, Receiver<Event>)>,
    /// The map asked for, and what has arrived of it.
    incoming: Option<Incoming>,
    /// The joiner's row, or an observer's number from [`OBSERVER_BASE`] up.
    pub slot: u8,
    /// The joiner watches rather than plays: it has no row.
    pub observer: bool,
    /// The lobby, once the host has sent it.
    pub view: Option<LobbyView>,
}

/// A map being received.
struct Incoming {
    hash: u64,
    /// The size the host offered, once it has.
    size: Option<usize>,
    data: Vec<u8>,
}

impl GuestRoom {
    fn send(&self, msg: Msg) {
        if let Some((link, _)) = &self.parts {
            link.send(&msg);
        }
    }

    pub fn set_race(&mut self, race: u8) {
        self.send(Msg::SetRace { race });
    }

    pub fn set_ready(&mut self, ready: bool) {
        self.send(Msg::SetReady { ready });
    }

    /// Tells the host whether this machine has the lobby's map `hash`.
    pub fn set_have_map(&mut self, hash: u64, have: bool) {
        self.send(Msg::HaveMap { hash, have });
    }

    /// Asks the host for the map `hash`; what arrives is reported by [`GuestEvent`]s.
    pub fn request_map(&mut self, hash: u64) {
        self.incoming = Some(Incoming {
            hash,
            size: None,
            data: Vec::new(),
        });
        self.send(Msg::MapRequest { hash });
    }

    fn map_message(&mut self, msg: Msg, out: &mut Vec<GuestEvent>) {
        let Some(inc) = &mut self.incoming else { return };
        let fail = |why: &str| GuestEvent::MapFailed(why.to_string());
        match msg {
            Msg::MapOffer { hash, size } if hash == inc.hash && inc.size.is_none() => {
                let size = size as usize;
                if size == 0 || size > MAX_MAP_BYTES {
                    self.incoming = None;
                    out.push(fail("The map is too big to receive."));
                } else {
                    inc.size = Some(size);
                    inc.data.reserve(size);
                    out.push(GuestEvent::MapProgress { got: 0, size });
                }
            }
            Msg::MapChunk { hash, offset, bytes } if hash == inc.hash => {
                let Some(size) = inc.size else { return };
                // In order, and no more than was offered.
                if offset as usize != inc.data.len() || inc.data.len() + bytes.len() > size {
                    self.incoming = None;
                    out.push(fail("The map arrived damaged."));
                    return;
                }
                inc.data.extend_from_slice(&bytes);
                if inc.data.len() < size {
                    out.push(GuestEvent::MapProgress {
                        got: inc.data.len(),
                        size,
                    });
                    return;
                }
                let (hash, data) = (inc.hash, std::mem::take(&mut inc.data));
                self.incoming = None;
                if tidewater_replay::start::map_hash(&data) != hash
                    || tidewater_formats::pud::Pud::parse(&data).is_err()
                {
                    out.push(fail("The map arrived damaged."));
                } else {
                    out.push(GuestEvent::MapReceived { hash, bytes: data });
                }
            }
            Msg::MapDenied { hash, reason } if hash == inc.hash => {
                self.incoming = None;
                out.push(GuestEvent::MapFailed(reason));
            }
            _ => {}
        }
    }

    pub fn say(&mut self, text: &str) {
        self.send(Msg::Say { text: clean_say(text) });
    }

    /// What arrived since the last call. After [`GuestEvent::Go`] or [`GuestEvent::Ended`] the room does nothing more.
    pub fn poll(&mut self) -> Vec<GuestEvent> {
        let mut out = Vec::new();
        while let Some((_, events)) = &self.parts {
            let Ok(ev) = events.try_recv() else { break };
            match ev {
                Event::Closed(_) => {
                    self.parts = None;
                    out.push(GuestEvent::Ended("Connection terminated.".into()));
                }
                Event::Msg(_, Msg::Lobby(view)) => {
                    self.view = Some(*view);
                    out.push(GuestEvent::View);
                }
                Event::Msg(_, Msg::Refuse { reason }) => {
                    self.parts = None;
                    out.push(GuestEvent::Ended(reason));
                }
                Event::Msg(_, Msg::Go { slot, config }) => {
                    // The messages after the Go are the game's: they stay in the channel for the wire.
                    let Some((link, events)) = self.parts.take() else { break };
                    let config = *config;
                    let host = config.lobby.humans.first().map_or(0, |h| h.0);
                    let names = config.names.clone();
                    out.push(GuestEvent::Go(Box::new(NetStart {
                        config,
                        me: slot,
                        wire: Box::new(ClientWire::from_parts(link, events, host, slot)),
                        names,
                    })));
                }
                Event::Msg(_, msg @ (Msg::MapOffer { .. } | Msg::MapChunk { .. } | Msg::MapDenied { .. })) => {
                    self.map_message(msg, &mut out);
                }
                Event::Msg(..) => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn ident() -> Identity {
        Identity::ours(1)
    }

    fn host(enhanced: bool) -> HostRoom {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut view = new_view(enhanced, "Host", race::HUMAN);
        view.map_name = "Maps\\x.pud".into();
        view.map_hash = 9;
        view.map_players = 4;
        HostRoom::new(listener, ident(), Texts::default(), view).unwrap()
    }

    fn join(room: &mut HostRoom, name: &str) -> GuestRoom {
        let addr = format!("127.0.0.1:{}", room.port());
        let name = name.to_string();
        let t = thread::spawn(move || handshake(&addr, ident(), &name));
        wait(room, |_| t.is_finished());
        let hs = t.join().unwrap().unwrap();
        let mut g = hs.accept().unwrap();
        wait_guest(room, &mut g, |g| g.view.is_some());
        let (slot, hash) = (g.slot as usize, room.view().map_hash);
        g.set_have_map(hash, true);
        wait_guest(room, &mut g, |g| g.view.as_ref().is_some_and(|v| v.rows[slot].has_map));
        g
    }

    fn wait(room: &mut HostRoom, mut done: impl FnMut(&mut HostRoom) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !done(room) && Instant::now() < end {
            room.poll();
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_guest(room: &mut HostRoom, g: &mut GuestRoom, mut done: impl FnMut(&GuestRoom) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !done(g) && Instant::now() < end {
            room.poll();
            g.poll();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(done(g), "timed out");
    }

    #[test]
    fn a_joiner_takes_the_first_free_row_and_sees_the_lobby() {
        let mut room = host(true);
        let g = join(&mut room, "Ann");
        assert_eq!(g.slot, 1);
        let v = g.view.as_ref().unwrap();
        assert_eq!(v.rows[1].name, "Ann");
        assert!(v.rows[1].ready, "Enhanced rules: ready on joining");
        assert_eq!(v.rows[2].state, RowState::Available);
        assert_eq!(room.view().humans(), 2);
    }

    #[test]
    fn a_joiner_with_another_identity_is_refused_and_says_why() {
        let mut room = host(true);
        let addr = format!("127.0.0.1:{}", room.port());
        let t = thread::spawn(move || handshake(&addr, Identity::ours(2), "Ann"));
        wait(&mut room, |_| t.is_finished());
        let err = t.join().unwrap().err().unwrap();
        assert!(format!("{err:#}").contains("game data"), "{err:#}");
        assert_eq!(room.view().humans(), 1);
    }

    #[test]
    fn an_observer_is_refused_by_the_menu_lobby() {
        let mut room = host(true);
        let addr = format!("127.0.0.1:{}", room.port());
        let t = thread::spawn(move || tcp::join_as(&addr, ident(), "Eve", true, |_| Ok(()), |_| {}));
        wait(&mut room, |_| t.is_finished());
        let err = t.join().unwrap().err().unwrap();
        assert!(format!("{err:#}").contains("no place for an observer"), "{err:#}");
        assert_eq!(room.view().humans(), 1);
    }

    #[test]
    fn a_full_room_and_a_duplicate_name_are_refused() {
        let mut room = host(true);
        for i in 1..ROWS {
            room.set_row(i, RowChoice::Closed);
        }
        let addr = format!("127.0.0.1:{}", room.port());
        let a = addr.clone();
        let t = thread::spawn(move || handshake(&a, ident(), "Ann"));
        wait(&mut room, |_| t.is_finished());
        assert!(format!("{:#}", t.join().unwrap().err().unwrap()).contains("full"));
        room.set_row(1, RowChoice::Available);
        let t = thread::spawn(move || handshake(&addr, ident(), "host"));
        wait(&mut room, |_| t.is_finished());
        assert!(format!("{:#}", t.join().unwrap().err().unwrap()).contains("Duplicate"));
    }

    #[test]
    fn rows_race_chat_and_kick_out_reach_the_other_side() {
        let mut room = host(true);
        let mut g = join(&mut room, "Ann");
        g.set_race(race::ORC);
        g.say("hello");
        wait_guest(&mut room, &mut g, |g| {
            g.view
                .as_ref()
                .is_some_and(|v| v.rows[1].race == race::ORC && v.rows[1].say == "hello")
        });
        room.set_row(2, RowChoice::Computer);
        room.say("hi back");
        wait_guest(&mut room, &mut g, |g| {
            g.view
                .as_ref()
                .is_some_and(|v| v.rows[2].state == RowState::Computer && v.rows[0].say == "hi back")
        });
        room.set_row(1, RowChoice::KickOut);
        let end = Instant::now() + Duration::from_secs(10);
        let mut ended = None;
        while ended.is_none() && Instant::now() < end {
            room.poll();
            for e in g.poll() {
                if let GuestEvent::Ended(r) = e {
                    ended = Some(r);
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(ended.unwrap(), Texts::default().kicked);
        assert_eq!(room.view().rows[1].state, RowState::Available);
    }

    #[test]
    fn a_joiner_who_leaves_frees_its_row_and_cancel_terminates_the_game() {
        let mut room = host(true);
        let g = join(&mut room, "Ann");
        drop(g);
        wait(&mut room, |r| r.view().humans() == 1);
        assert_eq!(room.view().rows[1].state, RowState::Available);
        let mut g = join(&mut room, "Bob");
        room.cancel();
        let end = Instant::now() + Duration::from_secs(10);
        let mut ended = None;
        while ended.is_none() && Instant::now() < end {
            for e in g.poll() {
                if let GuestEvent::Ended(r) = e {
                    ended = Some(r);
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(ended.unwrap(), Texts::default().terminated);
    }

    #[test]
    fn the_creator_and_a_computer_can_start_and_available_rows_are_ignored() {
        let mut room = host(true);
        assert_eq!(room.can_start(), Err(StartError::Alone));
        room.set_row(2, RowChoice::Computer);
        assert_eq!(room.view().humans(), 1);
        assert_eq!(
            room.can_start(),
            Ok(()),
            "the creator and one Computer; the other rows stay Available"
        );
        let n = room.start(7).unwrap();
        assert_eq!(n.me, 0);
        assert_eq!(n.config.lobby.humans.len(), 1, "one human, the other slot a Computer");
    }

    #[test]
    fn start_needs_a_map_a_joiner_room_and_in_classic_mode_ready_boxes() {
        let mut room = host(false);
        assert_eq!(room.can_start(), Err(StartError::Alone), "the creator alone");
        let mut g = join(&mut room, "Ann");
        assert_eq!(
            room.can_start(),
            Err(StartError::NotReady),
            "Classic: the joiner must tick Ready"
        );
        g.set_ready(true);
        wait_guest(&mut room, &mut g, |g| g.view.as_ref().is_some_and(|v| v.rows[1].ready));
        assert_eq!(room.can_start(), Ok(()));
        // A changed option clears the Ready boxes again.
        let custom = Custom {
            one_worker: true,
            ..room.view().custom
        };
        room.set_options(custom, 4, true);
        assert_eq!(room.can_start(), Err(StartError::NotReady));
        // More rows than the map has slots.
        let mut room = host(true);
        let _g = join(&mut room, "Ann");
        room.set_map("Maps\\x.pud", 9, vec![], 2);
        room.set_row(2, RowChoice::Computer);
        assert_eq!(room.can_start(), Err(StartError::TooMany));
        room.set_map("", 0, vec![], 0);
        assert_eq!(room.can_start(), Err(StartError::NoMap));
    }

    #[test]
    fn start_packs_the_rows_into_slots_and_both_sides_get_the_same_game() {
        let mut room = host(true);
        room.set_host_race(race::ORC);
        room.set_row(1, RowChoice::Closed);
        room.set_row(2, RowChoice::Computer);
        room.set_row(1, RowChoice::Available);
        let mut g = join(&mut room, "Ann");
        assert_eq!(g.slot, 1);
        room.set_row(3, RowChoice::Computer);
        room.poll();
        let mut got = None;
        let start = room.start(1234).unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while got.is_none() && Instant::now() < end {
            for e in g.poll() {
                if let GuestEvent::Go(n) = e {
                    got = Some(n);
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        let theirs = got.expect("the joiner is sent the game");
        assert_eq!(start.me, 0);
        assert_eq!(theirs.me, 1);
        let mine = start.config.clone();
        assert_eq!(mine.seed, 1234);
        assert_eq!(mine.lobby.humans.len(), 2);
        assert_eq!(mine.lobby.humans[0], (0, race::ORC));
        assert!(
            matches!(mine.lobby.humans[1].1, race::HUMAN | race::ORC),
            "Random is settled"
        );
        assert_eq!(
            mine.lobby.computers,
            [2, 3],
            "rows 2 and 3 pack into slots 2 and 3 after humans in 0 and 1"
        );
        assert_eq!(theirs.config.lobby.humans, mine.lobby.humans);
        assert_eq!(theirs.config.lobby.local, 1);
        assert_eq!(theirs.names, ["Host", "Ann", "", ""]);
        // The wires carry a turn both ways after the start.
        let (mut a, mut b) = (start.wire, theirs.wire);
        a.send(Msg::Turn {
            slot: 0,
            seq: 1,
            cmd: None,
        });
        let end = Instant::now() + Duration::from_secs(10);
        let mut heard = false;
        while !heard && Instant::now() < end {
            heard = b
                .poll()
                .iter()
                .any(|i| matches!(i, crate::net::tcp::Inbound::Msg { from: 0, .. }));
            thread::sleep(Duration::from_millis(5));
        }
        assert!(heard);
    }

    #[test]
    fn the_hosts_optional_improvements_reach_the_game_config() {
        let mut room = host(true);
        assert!(room.view().to_config(1).0.extras.is_default());
        let on = tidewater_sim::Extras {
            rally_points: true,
            selection_limit: 12,
            ..Default::default()
        };
        room.set_extras(on);
        assert_eq!(room.view().extras, on);
        assert_eq!(room.view().to_config(1).0.extras, on);
    }

    #[test]
    fn the_hosts_c_plus_l_options_reach_the_joiners_view_and_both_games_configs() {
        let mut room = host(true);
        let mut g = join(&mut room, "Ann");
        assert!(g.view.as_ref().unwrap().client_extras.is_empty(), "all off by default");
        let chosen = vec![("health-bars".to_string(), 2), ("smart-attack-move".to_string(), 1)];
        room.set_client_extras(chosen.clone());
        wait_guest(&mut room, &mut g, |g| {
            g.view.as_ref().is_some_and(|v| v.client_extras == chosen)
        });
        assert_eq!(room.view().to_config(1).0.client_extras, chosen);
        // The start hands the same choice to the joiner, whatever the joiner's own settings are.
        let start = room.start(5).unwrap();
        let got = events_until_go(&mut g);
        assert_eq!(start.config.client_extras, chosen);
        assert_eq!(got.config.client_extras, chosen);
        let theirs = crate::extras::ClientExtras::from_lobby_values(&got.config.client_extras);
        assert_eq!(theirs.health_bars(), crate::extras::HealthBars::Selected);
        assert!(theirs.smart_attack_move());
    }

    fn events_until_go(g: &mut GuestRoom) -> Box<NetStart> {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            for e in g.poll() {
                if let GuestEvent::Go(n) = e {
                    return n;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("the joiner is never sent the game");
    }

    /// An observer's handshake through the room, with its map already there.
    fn watch(room: &mut HostRoom, name: &str) -> GuestRoom {
        let addr = format!("127.0.0.1:{}", room.port());
        let name = name.to_string();
        let t = thread::spawn(move || handshake_as(&addr, ident(), &name, true));
        wait(room, |_| t.is_finished());
        let hs = t.join().unwrap().unwrap();
        assert!(
            hs.observer && hs.slot >= OBSERVER_BASE,
            "an observer's number, not a row"
        );
        let mut g = hs.accept().unwrap();
        wait_guest(room, &mut g, |g| g.view.is_some());
        let hash = room.view().map_hash;
        g.set_have_map(hash, true);
        let end = Instant::now() + Duration::from_secs(10);
        while room.can_start() == Err(StartError::MissingMap) && Instant::now() < end {
            room.poll();
            thread::sleep(Duration::from_millis(5));
        }
        g
    }

    #[test]
    fn observers_come_in_only_where_the_host_allows_them_and_take_no_row() {
        let mut room = host(true);
        let join_err = |room: &mut HostRoom| {
            let addr = format!("127.0.0.1:{}", room.port());
            let t = thread::spawn(move || handshake_as(&addr, ident(), "Eve", true));
            wait(room, |_| t.is_finished());
            format!("{:#}", t.join().unwrap().err().unwrap())
        };
        assert_eq!(room.view().observers, 0, "none by default");
        assert!(join_err(&mut room).contains("no place for an observer"));
        room.set_observers(1);
        let mut player = join(&mut room, "Ann");
        let mut eve = watch(&mut room, "Eve");
        // It is in the list of watchers, not in a row, and everyone sees it.
        assert_eq!(room.view().watchers, ["Eve"]);
        assert_eq!(room.view().humans(), 2, "the host and Ann: the observer takes no slot");
        assert!(room.view().rows.iter().all(|r| r.name != "Eve"));
        wait_guest(&mut room, &mut player, |g| {
            g.view
                .as_ref()
                .is_some_and(|v| v.watchers == ["Eve"] && v.observers == 1)
        });
        assert_eq!(eve.view.as_ref().unwrap().observers, 1);
        // The one place is taken, and a name is still unique.
        assert!(join_err(&mut room).contains("no place for an observer"));
        // The observer cannot change a row, and the game can start with it in.
        eve.set_race(race::ORC);
        eve.say("hi");
        for _ in 0..20 {
            room.poll();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(room.view().rows.iter().all(|r| r.say != "hi" && (r.name != "Eve")));
        assert_eq!(room.can_start(), Ok(()));
        let start = room.start(7).unwrap();
        let theirs = events_until_go(&mut player);
        let seen = events_until_go(&mut eve);
        assert_eq!(theirs.me, 1);
        assert_eq!(seen.me, OBSERVER_BASE);
        assert!(
            !seen.config.lobby.humans.iter().any(|h| h.0 == seen.me),
            "not one of the game's players"
        );
        assert_eq!(seen.config.lobby.local, 0, "it looks from the host's place");
        assert_eq!(start.config.lobby.humans.len(), 2);
        // The host's turns reach the observer's wire.
        let (mut a, mut b) = (start.wire, seen.wire);
        a.send(Msg::Turn {
            slot: 0,
            seq: 1,
            cmd: None,
        });
        let end = Instant::now() + Duration::from_secs(10);
        let mut heard = false;
        while !heard && Instant::now() < end {
            heard = b
                .poll()
                .iter()
                .any(|i| matches!(i, crate::net::tcp::Inbound::Msg { from: 0, .. }));
            thread::sleep(Duration::from_millis(5));
        }
        assert!(heard);
    }

    #[test]
    fn an_observer_who_lacks_the_map_blocks_the_start_until_it_has_it_and_fewer_places_send_observers_away() {
        let (bytes, hash) = pud();
        let mut room = host(true);
        room.set_map("Maps\\x.pud", hash, vec![], 4);
        room.set_map_data(bytes);
        room.set_observers(2);
        let _player = join(&mut room, "Ann");
        let addr = format!("127.0.0.1:{}", room.port());
        let t = thread::spawn(move || handshake_as(&addr, ident(), "Eve", true));
        wait(&mut room, |_| t.is_finished());
        let mut eve = t.join().unwrap().unwrap().accept().unwrap();
        wait_guest(&mut room, &mut eve, |g| g.view.is_some());
        assert_eq!(
            room.can_start(),
            Err(StartError::MissingMap),
            "the observer needs the map too"
        );
        eve.set_have_map(hash, true);
        let end = Instant::now() + Duration::from_secs(10);
        while room.can_start().is_err() && Instant::now() < end {
            room.poll();
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(room.can_start(), Ok(()));
        // Fewer places than watchers: the watcher is sent away.
        room.set_observers(0);
        let end = Instant::now() + Duration::from_secs(10);
        let mut ended = false;
        while !ended && Instant::now() < end {
            room.poll();
            ended = eve.poll().iter().any(|e| matches!(e, GuestEvent::Ended(_)));
            thread::sleep(Duration::from_millis(5));
        }
        assert!(ended);
        assert!(room.view().watchers.is_empty());
    }

    #[test]
    fn a_joiner_of_the_previous_protocol_is_refused_with_both_versions() {
        use std::io::Write;
        let mut room = host(true);
        let addr = format!("127.0.0.1:{}", room.port());
        let t = thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            // Protocol 7's Hello: variant 0, the version, then what that version put after it.
            let body = [0u8, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            s.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
            s.write_all(&body).unwrap();
            read_frame(&mut s)
        });
        wait(&mut room, |_| t.is_finished());
        match t.join().unwrap().unwrap() {
            Msg::Refuse { reason } => {
                assert_eq!(reason, Identity::protocol_reason(super::super::proto::PROTOCOL, 7));
                assert!(
                    reason.contains(&format!("host {}", super::super::proto::PROTOCOL)),
                    "{reason}"
                );
                assert!(reason.contains("joiner 7"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// A small valid map and its hash.
    fn pud() -> (Vec<u8>, u64) {
        let bytes = tidewater_formats::pud::build::Map {
            size: 32,
            ..Default::default()
        }
        .bytes();
        let hash = tidewater_replay::start::map_hash(&bytes);
        (bytes, hash)
    }

    /// A room whose map is the synthetic one, held by the host, and a joiner that has asked for it.
    fn room_with_map() -> (HostRoom, GuestRoom, u64) {
        let (bytes, hash) = pud();
        let mut room = host(true);
        room.set_map("Maps\\x.pud", hash, vec![], 4);
        room.set_map_data(bytes);
        let g = join_no_map(&mut room, "Ann");
        (room, g, hash)
    }

    /// Joins without saying the map is there.
    fn join_no_map(room: &mut HostRoom, name: &str) -> GuestRoom {
        let addr = format!("127.0.0.1:{}", room.port());
        let name = name.to_string();
        let t = thread::spawn(move || handshake(&addr, ident(), &name));
        wait(room, |_| t.is_finished());
        let mut g = t.join().unwrap().unwrap().accept().unwrap();
        wait_guest(room, &mut g, |g| g.view.is_some());
        g
    }

    /// Both ends run until the joiner has an event that `done` takes.
    fn events_until(
        room: &mut HostRoom,
        g: &mut GuestRoom,
        mut done: impl FnMut(&GuestEvent) -> bool,
    ) -> Vec<GuestEvent> {
        let end = Instant::now() + Duration::from_secs(10);
        let mut all = Vec::new();
        while Instant::now() < end {
            room.poll();
            for e in g.poll() {
                let stop = done(&e);
                all.push(e);
                if stop {
                    return all;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out");
    }

    #[test]
    fn a_map_the_joiner_lacks_is_sent_in_chunks_checked_and_then_the_host_can_start() {
        let (mut room, mut g, hash) = room_with_map();
        assert_eq!(room.can_start(), Err(StartError::MissingMap));
        g.set_have_map(hash, false);
        g.request_map(hash);
        let events = events_until(&mut room, &mut g, |e| {
            matches!(e, GuestEvent::MapReceived { .. } | GuestEvent::MapFailed(_))
        });
        let Some(GuestEvent::MapReceived { hash: h, bytes }) = events.last() else {
            panic!("no map");
        };
        assert_eq!((*h, bytes.as_slice()), (hash, pud().0.as_slice()));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, GuestEvent::MapProgress { got: 0, .. }))
        );
        assert_eq!(
            room.can_start(),
            Err(StartError::MissingMap),
            "the joiner has not said it has it"
        );
        g.set_have_map(hash, true);
        wait_guest(&mut room, &mut g, |g| {
            g.view.as_ref().is_some_and(|v| v.rows[1].has_map)
        });
        assert_eq!(room.can_start(), Ok(()));
        // A new map asks every joiner again.
        room.set_map("Maps\\y.pud", hash ^ 1, vec![], 4);
        wait_guest(&mut room, &mut g, |g| {
            g.view.as_ref().is_some_and(|v| !v.rows[1].has_map)
        });
        assert_eq!(room.can_start(), Err(StartError::MissingMap));
    }

    #[test]
    fn a_map_is_denied_when_the_host_has_none_to_send_and_after_too_many_requests() {
        let (mut room, mut g, hash) = room_with_map();
        g.request_map(hash ^ 1);
        let ev = events_until(&mut room, &mut g, |e| matches!(e, GuestEvent::MapFailed(_)));
        assert!(matches!(ev.last(), Some(GuestEvent::MapFailed(_))));
        for _ in 0..MAP_REQUESTS {
            g.request_map(hash);
            events_until(&mut room, &mut g, |e| matches!(e, GuestEvent::MapReceived { .. }));
        }
        g.request_map(hash);
        let ev = events_until(&mut room, &mut g, |e| matches!(e, GuestEvent::MapFailed(_)));
        let Some(GuestEvent::MapFailed(why)) = ev.last() else {
            panic!()
        };
        assert!(why.contains("already"), "{why}");
    }

    #[test]
    fn the_joiner_refuses_a_map_that_is_too_big_out_of_order_or_not_the_one_asked_for() {
        let (_, hash) = pud();
        let (mut room, mut g, _) = room_with_map();
        let mut out = Vec::new();
        let ask = |g: &mut GuestRoom| {
            g.incoming = Some(Incoming {
                hash,
                size: None,
                data: Vec::new(),
            });
        };
        // Too big.
        ask(&mut g);
        g.map_message(
            Msg::MapOffer {
                hash,
                size: MAX_MAP_BYTES as u32 + 1,
            },
            &mut out,
        );
        assert!(matches!(out.pop(), Some(GuestEvent::MapFailed(_))));
        assert!(g.incoming.is_none());
        // A chunk out of order.
        ask(&mut g);
        g.map_message(Msg::MapOffer { hash, size: 100 }, &mut out);
        assert!(matches!(out.pop(), Some(GuestEvent::MapProgress { .. })));
        g.map_message(
            Msg::MapChunk {
                hash,
                offset: 50,
                bytes: vec![0; 10],
            },
            &mut out,
        );
        assert!(matches!(out.pop(), Some(GuestEvent::MapFailed(_))));
        // More than offered.
        ask(&mut g);
        g.map_message(Msg::MapOffer { hash, size: 10 }, &mut out);
        out.clear();
        g.map_message(
            Msg::MapChunk {
                hash,
                offset: 0,
                bytes: vec![0; 11],
            },
            &mut out,
        );
        assert!(matches!(out.pop(), Some(GuestEvent::MapFailed(_))));
        // The right size but not the hash asked for, and not a map.
        ask(&mut g);
        g.map_message(Msg::MapOffer { hash, size: 10 }, &mut out);
        out.clear();
        g.map_message(
            Msg::MapChunk {
                hash,
                offset: 0,
                bytes: vec![7; 10],
            },
            &mut out,
        );
        assert!(matches!(out.pop(), Some(GuestEvent::MapFailed(_))));
        // The right bytes under another hash than the one asked for.
        let (bytes, _) = pud();
        g.incoming = Some(Incoming {
            hash: hash ^ 5,
            size: None,
            data: Vec::new(),
        });
        g.map_message(
            Msg::MapOffer {
                hash: hash ^ 5,
                size: bytes.len() as u32,
            },
            &mut out,
        );
        out.clear();
        g.map_message(
            Msg::MapChunk {
                hash: hash ^ 5,
                offset: 0,
                bytes,
            },
            &mut out,
        );
        assert!(
            matches!(out.pop(), Some(GuestEvent::MapFailed(_))),
            "the content must hash to what was asked for"
        );
        // Nothing asked for: nothing taken.
        g.map_message(Msg::MapOffer { hash, size: 10 }, &mut out);
        assert!(out.is_empty());
        room.poll();
    }

    #[test]
    fn a_joiner_of_another_protocol_is_refused_with_the_reason() {
        use std::io::Write;
        let mut room = host(true);
        let addr = format!("127.0.0.1:{}", room.port());
        let t = thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            // A protocol-1 Hello: variant 0, version 1, then what that version put after it.
            let body = [0u8, 1, 5, b'a', b'b', b'c', b'd', b'e', 0, 0];
            s.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
            s.write_all(&body).unwrap();
            read_frame(&mut s)
        });
        wait(&mut room, |_| t.is_finished());
        match t.join().unwrap().unwrap() {
            Msg::Refuse { reason } => assert!(reason.contains("protocol"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn random_races_are_settled_from_the_seed_and_stay_the_same_for_a_seed() {
        let mut v = new_view(true, "Host", race::NEUTRAL);
        v.rows[1] = RowView {
            state: RowState::Human,
            name: "A".into(),
            race: race::NEUTRAL,
            ready: true,
            say: String::new(),
            has_map: true,
        };
        let (a, _) = v.to_config(7);
        let (b, _) = v.to_config(7);
        assert_eq!(a.lobby.humans, b.lobby.humans);
        let mut seen = std::collections::HashSet::new();
        for seed in 0..64 {
            let (c, _) = v.to_config(seed);
            for h in &c.lobby.humans {
                assert!(matches!(h.1, race::HUMAN | race::ORC));
                seen.insert(h.1);
            }
        }
        assert_eq!(seen.len(), 2, "both races come up over many seeds");
    }
}
