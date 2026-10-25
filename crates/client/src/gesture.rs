//! Touch gestures turned into the mouse model (`docs/design/touch.md`). The game's rules only know a pointer, buttons,
//! keys and a wheel; this state machine reads the fingers' events (`input::Touch`) and says what the mouse and keyboard
//! would have done, so every rule stays the spec's own. Nothing here depends on an engine or a clock: the caller says
//! how much time passed and hands a [`Config`] (how many window points a millimetre is, where the map view is, the
//! player's touch settings).
//!
//! | Fingers | Gesture | Becomes |
//! |---|---|---|
//! | 1 | Tap (lift before the long press, moved less than [`SLOP_MM`]) | Left press, then release a frame later; [`Frame::tap_press`] marks one on the map view |
//! | 1 | Drag on the map view (past the slop before [`Config::arm_ms`]) | [`Frame::pan`], gliding on after a flick ([`Config::glide`]) |
//! | 1 | Hold for [`Config::arm_ms`], then drag | Left button held from where the finger went down: the selection box ([`Frame::armed`] marks the arming) |
//! | 1 | Drag anywhere else (minimap, panel, menus) | Left button held from where the finger went down |
//! | 1 | Long press ([`Config::long_press_ms`] still, then lift without moving) | Right click where the finger lifts; a pulse shows when it is ready, and nothing is clicked while the finger stays down (a drag then still draws the box) |
//! | 1 | Two taps on one spot within [`DOUBLE_TAP_MS`] | Two clicks at exactly the first tap's point, so the game sees a double click |
//! | 2 | Drag of both together ([`TwoFinger::Pan`]) | [`Frame::pan`] |
//! | 2 | Both down on the map view ([`TwoFinger::Corners`]) | Left button held from the first finger to the second: the selection box |
//! | 2 | Spread or pinch ([`Config::pinch`]) | [`Frame::zoom`] |
//! | 3 | Tap (all up within [`MENU_TAP_MS`], none moved; [`Config::menu_tap`]) | F10, the game menu |
//!
//! **Placing and targeting** ([`Config::hover`]: a building's outline or a spell's target follows the pointer). A
//! finger on the map view moves the pointer instead of clicking: a drag carries the outline along and leaves it where
//! the finger lifts, a tap elsewhere moves it there, and a tap on the outline ([`Config::ghost`]) or near the pointer
//! confirms with a left click at the pointer. A finger held near the view's edge scrolls the map (as the mouse at the
//! screen's edge does); a long press is still a right click, which cancels, sent when the finger lifts. A tap on the spot of a confirm, within
//! [`DOUBLE_TAP_MS`] of it, is dropped: a double tap on the outline confirms once, where the second tap would otherwise
//! reach the game after placement ended and, as a smart tap, send the builder to the site instead of building there.
//!
//! **Frames.** The mouse model needs a press and its release in different frames (a press and release in one frame
//! drop the release), and a pointer move after a press in a later one. The machine therefore emits [`Frame`]s, and
//! the caller applies one per game frame ([`Gesture::pop`]); events that would break these rules start a new frame.
//!
//! **Fingers joining and leaving.** A second finger while one is undecided, panning or held cancels it silently (no
//! click); one that comes while a drag holds the left button releases it where the finger is, then goes on as two
//! fingers. When one of several fingers lifts, the rest are ignored until all are up, so a pinch never turns into a
//! stray click or drag. A cancelled finger releases what it held and nothing more. A new finger stops a glide.

use std::collections::VecDeque;
use std::time::Duration;

use crate::input::{Button, Key, Touch, TouchPhase};

/// How long a finger must stay down, nearly still, to be a long press (the default of [`Config::long_press_ms`]).
pub const LONG_PRESS_MS: f64 = 450.0;
/// How long a finger must stay down, nearly still, before a drag draws the selection box instead of panning (the
/// default of [`Config::arm_ms`]).
pub const ARM_MS: f64 = 300.0;
/// How far a single finger may move and still be a tap or a long press, in millimetres.
pub const SLOP_MM: f32 = 3.0;
/// How far two fingers' centre must move before it pans, in millimetres.
pub const PAN_MM: f32 = 3.0;
/// How much the fingers' distance must change before it zooms, in millimetres.
pub const PINCH_MM: f32 = 3.0;
/// The longest three fingers may stay down for a menu tap.
pub const MENU_TAP_MS: f64 = 450.0;
/// Two taps this close in time (from the first's lift to the second's) are a double tap.
pub const DOUBLE_TAP_MS: f64 = 450.0;
/// How far apart a double tap's two taps may be, in millimetres.
pub const DOUBLE_TAP_MM: f32 = 6.0;
/// While placing, a tap this near the pointer confirms, in millimetres.
pub const CONFIRM_MM: f32 = 8.0;
/// While placing, a finger this near the map view's edge scrolls the map, in millimetres.
pub const EDGE_MM: f32 = 8.0;
/// How fast the edge scroll moves the map, in millimetres a second.
pub const EDGE_MM_S: f32 = 60.0;
/// A pan's glide slows by a factor of e every this many milliseconds.
pub const GLIDE_TAU_MS: f32 = 325.0;
/// A pan lifting faster than this glides on, in millimetres a second.
pub const GLIDE_START_MM_S: f32 = 30.0;
/// A glide stops below this speed, in millimetres a second.
pub const GLIDE_STOP_MM_S: f32 = 8.0;
/// A lift's speed is measured over this last stretch of the pan, in milliseconds.
const VELOCITY_MS: f64 = 100.0;

/// A point in the window's logical points.
pub type Pt = (f32, f32);

/// A rectangle in the window's logical points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Zone {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Zone {
    pub fn contains(&self, p: Pt) -> bool {
        p.0 >= self.x && p.0 < self.x + self.w && p.1 >= self.y && p.1 < self.y + self.h
    }

    /// Which way a point within `margin` of the edge points out of the zone, per axis (-1, 0 or 1).
    fn edge(&self, p: Pt, margin: f32) -> Pt {
        let axis = |v: f32, lo: f32, len: f32| {
            if v < lo + margin {
                -1.0
            } else if v >= lo + len - margin {
                1.0
            } else {
                0.0
            }
        };
        (axis(p.0, self.x, self.w), axis(p.1, self.y, self.h))
    }
}

/// What one finger dragging on the map view does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OneFinger {
    /// It pans; holding first draws the selection box.
    Pan,
    /// It draws the selection box straight away (the mouse's way); two fingers pan.
    Box,
}

/// What two fingers moving together do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TwoFinger {
    Pan,
    /// The two fingers are the selection box's corners.
    Corners,
    Off,
}

/// The settings a frame's gestures are read with.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// How many logical points a millimetre is.
    pub mm: f32,
    /// The map view, in window points; `None` off the game screen, where a drag is always the left button's.
    pub view: Option<Zone>,
    pub one_finger: OneFinger,
    /// A one-finger pan glides on after a flick.
    pub glide: bool,
    pub two_finger: TwoFinger,
    /// Spreading or pinching two fingers zooms.
    pub pinch: bool,
    /// A three-finger tap opens the game menu.
    pub menu_tap: bool,
    pub long_press_ms: f64,
    pub arm_ms: f64,
    /// A building is being placed or a target chosen: fingers on the map view move the pointer (module doc).
    pub hover: bool,
    /// The placed building's outline, in window points.
    pub ghost: Option<Zone>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mm: 6.0,
            view: None,
            one_finger: OneFinger::Pan,
            glide: true,
            two_finger: TwoFinger::Pan,
            pinch: true,
            menu_tap: true,
            long_press_ms: LONG_PRESS_MS,
            arm_ms: ARM_MS,
            hover: false,
            ghost: None,
        }
    }
}

/// What one game frame gets from the gesture machine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    /// The pointer goes here.
    pub pointer: Option<Pt>,
    /// Button changes, in order.
    pub buttons: Vec<(Button, bool)>,
    /// Key changes, in order.
    pub keys: Vec<(Key, bool)>,
    /// How far the content should move with the fingers, in points.
    pub pan: Pt,
    /// By what factor the fingers' distance changed, and the centre it changed around.
    pub zoom: Option<(f32, Pt)>,
    /// This frame's left press starts a tap on the map view (the caller may make it a smart order).
    pub tap_press: bool,
    /// This frame's left release ends a tap (the tap tolerance applies to it).
    pub tap: bool,
    /// A long press fired at this point: draw the pulse.
    pub pulse: Option<Pt>,
    /// A finger was held long enough that a drag now draws the selection box, at this point.
    pub armed: Option<Pt>,
}

impl Frame {
    fn is_empty(&self) -> bool {
        *self == Frame::default()
    }
}

#[derive(Clone, Copy, Debug)]
struct Finger {
    id: u64,
    pos: Pt,
}

#[derive(Clone, Copy, Debug)]
struct Two {
    ids: [u64; 2],
    centre0: Pt,
    dist0: f32,
    last_centre: Pt,
    last_dist: f32,
    pan: bool,
    zoom: bool,
    can_pan: bool,
    can_zoom: bool,
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Idle,
    /// One finger down, undecided: where, when the touch began, whether it began on the map view, whether it was
    /// held long enough to draw the box and whether it was held still for the long press (lifting it then right-clicks).
    Pending {
        id: u64,
        start: Pt,
        t0: f64,
        in_view: bool,
        armed: bool,
        ripe: bool,
    },
    /// One finger dragging with the left button held.
    Drag {
        id: u64,
    },
    /// One finger panning the map view.
    Pan {
        id: u64,
        last: Pt,
    },
    /// One finger moving the pointer while placing or targeting; `ripe` once it was held still for the long press
    /// (lifting it then right-clicks, which cancels).
    Hover {
        id: u64,
        start: Pt,
        t0: f64,
        moved: bool,
        ripe: bool,
    },
    /// A long press fired; the finger is still down.
    Held,
    /// Two fingers.
    Two {
        t0: f64,
        two: Two,
    },
    /// Two fingers as the selection box's corners, the left button held; `b` is the moving corner.
    Corners {
        b: u64,
    },
    /// Three fingers or more: a menu tap if they all lift soon, still.
    Menu {
        t0: f64,
        moved: bool,
    },
    /// Waiting for every finger to lift.
    Ignore,
}

/// The gesture machine.
#[derive(Debug)]
pub struct Gesture {
    mode: Mode,
    fingers: Vec<Finger>,
    /// Where the fingers of a menu tap went down, to see whether they moved.
    starts: Vec<Finger>,
    now: f64,
    cur: Frame,
    queue: VecDeque<Frame>,
    cfg: Config,
    /// A one-finger pan's recent places, for its speed at the lift.
    samples: VecDeque<(f64, Pt)>,
    /// A glide's speed, in points a millisecond.
    glide: Option<Pt>,
    /// The last tap's point and the time of its lift, for a double tap.
    last_tap: Option<(Pt, f64)>,
    /// Where the pointer rests while placing or targeting.
    hover_at: Option<Pt>,
    /// The last confirming tap's point and the time of its lift (or of the last tap dropped after it).
    confirmed: Option<(Pt, f64)>,
}

impl Default for Gesture {
    fn default() -> Self {
        Gesture {
            mode: Mode::Idle,
            fingers: Vec::new(),
            starts: Vec::new(),
            now: 0.0,
            cur: Frame::default(),
            queue: VecDeque::new(),
            cfg: Config::default(),
            samples: VecDeque::new(),
            glide: None,
            last_tap: None,
            hover_at: None,
            confirmed: None,
        }
    }
}

fn dist(a: Pt, b: Pt) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn mid(a: Pt, b: Pt) -> Pt {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

impl Gesture {
    /// Time passes by `dt`, then the fingers' `events` (in order) are read with the settings `cfg`.
    pub fn update(&mut self, dt: Duration, events: &[Touch], cfg: &Config) {
        let ms = dt.as_secs_f64() * 1000.0;
        self.now += ms;
        self.cfg = *cfg;
        self.cfg.mm = cfg.mm.max(0.1);
        if !self.cfg.hover {
            self.hover_at = None;
        }
        for &t in events {
            self.feed(t);
        }
        self.eval_two();
        self.timers(ms as f32);
        self.next_frame();
    }

    /// The fingers that stayed still long enough, the edge scroll and the glide.
    fn timers(&mut self, ms: f32) {
        match self.mode {
            Mode::Pending {
                id,
                start,
                t0,
                in_view,
                armed,
                ripe,
            } => {
                let Some(f) = self.finger(id) else { return };
                // Nothing is clicked while the finger stays down: the pulse says lifting now right-clicks, and a drag
                // still draws the box.
                let ripen = !ripe && self.now - t0 >= self.cfg.long_press_ms;
                let arm = !armed && in_view && self.pans() && self.now - t0 >= self.cfg.arm_ms;
                if ripen {
                    self.pulse(f.pos);
                }
                if arm {
                    self.cur.armed = Some(start);
                }
                if ripen || arm {
                    self.mode = Mode::Pending {
                        id,
                        start,
                        t0,
                        in_view,
                        armed: armed || arm,
                        ripe: ripe || ripen,
                    };
                }
            }
            Mode::Hover {
                id,
                start,
                t0,
                moved,
                ripe,
            } => {
                let Some(f) = self.finger(id) else { return };
                let edge = self.edge_of(f.pos);
                if edge != (0.0, 0.0) {
                    // The view goes toward the edge, so the content moves the other way.
                    let step = EDGE_MM_S * self.cfg.mm * ms / 1000.0;
                    self.cur.pan.0 -= edge.0 * step;
                    self.cur.pan.1 -= edge.1 * step;
                    if !moved {
                        self.mode = Mode::Hover {
                            id,
                            start,
                            t0,
                            moved: true,
                            ripe: false,
                        };
                        self.rest_pointer(f.pos);
                    }
                } else if !moved && !ripe && self.now - t0 >= self.cfg.long_press_ms {
                    // As on the map: the pulse says lifting now cancels; nothing is clicked while the finger is down.
                    self.pulse(f.pos);
                    self.mode = Mode::Hover {
                        id,
                        start,
                        t0,
                        moved,
                        ripe: true,
                    };
                }
            }
            Mode::Idle => {
                if let Some(v) = self.glide {
                    self.cur.pan.0 += v.0 * ms;
                    self.cur.pan.1 += v.1 * ms;
                    let k = (-ms / GLIDE_TAU_MS).exp();
                    let v = (v.0 * k, v.1 * k);
                    let stop = GLIDE_STOP_MM_S * self.cfg.mm / 1000.0;
                    self.glide = (dist(v, (0.0, 0.0)) >= stop).then_some(v);
                }
            }
            _ => {}
        }
    }

    fn long_press(&mut self, at: Pt) {
        self.pulse(at);
        self.button(Button::Right, true, at);
        self.next_frame();
        self.button(Button::Right, false, at);
        self.mode = Mode::Held;
    }

    /// Whether one finger pans the map view.
    fn pans(&self) -> bool {
        self.cfg.one_finger == OneFinger::Pan
    }

    /// The edge-scroll direction of a finger at `p` while placing (zero away from the edge).
    fn edge_of(&self, p: Pt) -> Pt {
        match self.cfg.view {
            Some(v) if v.contains(p) => v.edge(p, EDGE_MM * self.cfg.mm),
            _ => (0.0, 0.0),
        }
    }

    fn in_view(&self, p: Pt) -> bool {
        self.cfg.view.is_some_and(|v| v.contains(p))
    }

    /// The next frame to apply, oldest first.
    pub fn pop(&mut self) -> Option<Frame> {
        self.queue.pop_front()
    }

    /// No finger is down and nothing is waiting to be applied.
    #[cfg(test)]
    fn idle(&self) -> bool {
        matches!(self.mode, Mode::Idle)
            && self.fingers.is_empty()
            && self.queue.is_empty()
            && self.cur.is_empty()
            && self.glide.is_none()
    }

    /// A finger drags with the left button held.
    pub fn dragging(&self) -> bool {
        matches!(self.mode, Mode::Drag { .. } | Mode::Corners { .. })
    }

    /// Where the pointer rests while placing or targeting, once a finger has put it somewhere.
    pub fn hover_pointer(&self) -> Option<Pt> {
        self.hover_at
    }

    /// Whether fingers are down.
    pub fn touching(&self) -> bool {
        !self.fingers.is_empty()
    }

    /// Lets go of everything held (a screen change, losing focus): the left button is released where its finger is.
    pub fn reset(&mut self) {
        let held = match self.mode {
            Mode::Drag { id } | Mode::Corners { b: id } => self.finger(id),
            _ => None,
        };
        if let Some(f) = held {
            self.button(Button::Left, false, f.pos);
        }
        self.glide = None;
        self.mode = if self.fingers.is_empty() {
            Mode::Idle
        } else {
            Mode::Ignore
        };
        self.next_frame();
    }

    fn finger(&self, id: u64) -> Option<Finger> {
        self.fingers.iter().copied().find(|f| f.id == id)
    }

    fn slop(&self) -> f32 {
        SLOP_MM * self.cfg.mm
    }

    // --- building frames -----------------------------------------------------------------------------------------

    /// Ends the frame being built.
    fn next_frame(&mut self) {
        let f = std::mem::take(&mut self.cur);
        if !f.is_empty() {
            self.queue.push_back(f);
        }
    }

    fn pointer(&mut self, p: Pt) {
        // A pointer that moves after a press belongs to a later frame.
        if !self.cur.buttons.is_empty() && self.cur.pointer != Some(p) {
            self.next_frame();
        }
        self.cur.pointer = Some(p);
    }

    /// The pointer moves to `p` and rests there (placing or targeting).
    fn rest_pointer(&mut self, p: Pt) {
        self.pointer(p);
        self.hover_at = Some(p);
    }

    fn button(&mut self, b: Button, down: bool, at: Pt) {
        // The same button twice, or a button after the pointer went elsewhere, is a new frame.
        if self.cur.buttons.iter().any(|&(x, _)| x == b) || self.cur.pointer.is_some_and(|q| q != at) {
            self.next_frame();
        }
        self.cur.pointer = Some(at);
        self.cur.buttons.push((b, down));
    }

    fn key(&mut self, k: Key, down: bool) {
        if self.cur.keys.iter().any(|&(x, _)| x == k) {
            self.next_frame();
        }
        self.cur.keys.push((k, down));
    }

    fn pulse(&mut self, at: Pt) {
        self.cur.pulse = Some(at);
    }

    // --- the fingers -----------------------------------------------------------------------------------------------

    fn feed(&mut self, t: Touch) {
        if t.phase != TouchPhase::Moved {
            self.eval_two();
        }
        match t.phase {
            TouchPhase::Began => self.began(t),
            TouchPhase::Moved => self.moved(t),
            TouchPhase::Ended => self.ended(t, false),
            TouchPhase::Cancelled => self.ended(t, true),
        }
    }

    fn began(&mut self, t: Touch) {
        if self.fingers.iter().any(|f| f.id == t.id) {
            return;
        }
        self.glide = None;
        self.fingers.push(Finger { id: t.id, pos: t.pos });
        match self.mode {
            Mode::Idle => {
                self.starts = vec![Finger { id: t.id, pos: t.pos }];
                let in_view = self.in_view(t.pos);
                self.mode = if self.cfg.hover && in_view {
                    Mode::Hover {
                        id: t.id,
                        start: t.pos,
                        t0: self.now,
                        moved: false,
                        ripe: false,
                    }
                } else {
                    Mode::Pending {
                        id: t.id,
                        start: t.pos,
                        t0: self.now,
                        in_view,
                        armed: false,
                        ripe: false,
                    }
                };
            }
            Mode::Pending { id, t0, .. } => self.two(id, t.id, t0, true),
            Mode::Pan { id, .. } => {
                self.samples.clear();
                self.two(id, t.id, self.now, true);
            }
            Mode::Hover { id, .. } => self.two(id, t.id, self.now, false),
            Mode::Drag { id } => {
                if let Some(f) = self.finger(id) {
                    self.button(Button::Left, false, f.pos);
                }
                self.two(id, t.id, self.now, false);
            }
            Mode::Held => {
                let first = self.fingers.first().map_or(t.id, |f| f.id);
                self.two(first, t.id, self.now, false);
            }
            Mode::Two { t0, .. } => {
                self.starts.push(Finger { id: t.id, pos: t.pos });
                self.mode = if self.cfg.menu_tap {
                    Mode::Menu { t0, moved: false }
                } else {
                    Mode::Ignore
                };
            }
            Mode::Corners { b } => {
                if let Some(f) = self.finger(b) {
                    self.button(Button::Left, false, f.pos);
                }
                self.mode = Mode::Ignore;
            }
            Mode::Menu { .. } => self.starts.push(Finger { id: t.id, pos: t.pos }),
            Mode::Ignore => {}
        }
    }

    /// Two fingers, the older `a` and the new `b`. `fresh`: the first finger had not clicked or held anything, so the
    /// two may be the selection box's corners.
    fn two(&mut self, a: u64, b: u64, t0: f64, fresh: bool) {
        let (Some(fa), Some(fb)) = (self.finger(a), self.finger(b)) else {
            self.mode = Mode::Ignore;
            return;
        };
        if !self.starts.iter().any(|s| s.id == a) {
            self.starts.push(fa);
        }
        self.starts.push(fb);
        if self.cfg.two_finger == TwoFinger::Corners && fresh && self.in_view(fa.pos) && self.in_view(fb.pos) {
            self.button(Button::Left, true, fa.pos);
            self.pointer(fb.pos);
            self.mode = Mode::Corners { b };
            return;
        }
        let (centre, d) = (mid(fa.pos, fb.pos), dist(fa.pos, fb.pos));
        self.mode = Mode::Two {
            t0,
            two: Two {
                ids: [a, b],
                centre0: centre,
                dist0: d,
                last_centre: centre,
                last_dist: d,
                pan: false,
                zoom: false,
                can_pan: self.cfg.two_finger == TwoFinger::Pan,
                can_zoom: self.cfg.pinch,
            },
        };
    }

    fn moved(&mut self, t: Touch) {
        let Some(f) = self.fingers.iter_mut().find(|f| f.id == t.id) else {
            return;
        };
        f.pos = t.pos;
        match self.mode {
            Mode::Pending {
                id,
                start,
                in_view,
                armed,
                ..
            } if id == t.id => {
                if dist(start, t.pos) > self.slop() {
                    if in_view && self.pans() && !armed {
                        // A pan: the content follows the finger from where it went down.
                        self.cur.pan.0 += t.pos.0 - start.0;
                        self.cur.pan.1 += t.pos.1 - start.1;
                        self.samples.clear();
                        self.samples.push_back((self.now, t.pos));
                        self.mode = Mode::Pan { id, last: t.pos };
                    } else {
                        // A drag: the left button goes down where the finger did, and follows it from the next frame.
                        self.button(Button::Left, true, start);
                        self.mode = Mode::Drag { id };
                        self.pointer(t.pos);
                    }
                }
            }
            Mode::Pan { id, last } if id == t.id => {
                self.cur.pan.0 += t.pos.0 - last.0;
                self.cur.pan.1 += t.pos.1 - last.1;
                self.samples.push_back((self.now, t.pos));
                while self.samples.len() > 2 && self.samples.front().is_some_and(|s| self.now - s.0 > VELOCITY_MS) {
                    self.samples.pop_front();
                }
                self.mode = Mode::Pan { id, last: t.pos };
            }
            Mode::Hover {
                id, start, t0, moved, ..
            } if id == t.id => {
                if moved || dist(start, t.pos) > self.slop() {
                    self.mode = Mode::Hover {
                        id,
                        start,
                        t0,
                        moved: true,
                        ripe: false,
                    };
                    self.rest_pointer(t.pos);
                }
            }
            Mode::Drag { id } if id == t.id => self.pointer(t.pos),
            Mode::Corners { b } if b == t.id => self.pointer(t.pos),
            Mode::Menu { t0, .. } => {
                let slop = self.slop();
                let moved = self
                    .starts
                    .iter()
                    .any(|s| self.finger(s.id).is_some_and(|f| dist(f.pos, s.pos) > slop));
                if moved {
                    self.mode = Mode::Menu { t0, moved: true };
                }
            }
            _ => {}
        }
    }

    /// Reads two fingers' places once for all the moves of a frame: SDL reports one finger at a time, and the first
    /// finger's move alone would look like a pinch.
    fn eval_two(&mut self) {
        let Mode::Two { t0, mut two } = self.mode else {
            return;
        };
        if let (Some(a), Some(b)) = (self.finger(two.ids[0]), self.finger(two.ids[1])) {
            let (centre, d) = (mid(a.pos, b.pos), dist(a.pos, b.pos));
            if two.can_pan && !two.pan && dist(centre, two.centre0) > PAN_MM * self.cfg.mm {
                two.pan = true;
            }
            if two.can_zoom && !two.zoom && (d - two.dist0).abs() > PINCH_MM * self.cfg.mm {
                two.zoom = true;
            }
            if two.pan {
                self.cur.pan.0 += centre.0 - two.last_centre.0;
                self.cur.pan.1 += centre.1 - two.last_centre.1;
                two.last_centre = centre;
            }
            if two.zoom && two.last_dist > 1.0 && d > 1.0 {
                let (old, _) = self.cur.zoom.unwrap_or((1.0, centre));
                self.cur.zoom = Some((old * d / two.last_dist, centre));
                two.last_dist = d;
            }
        }
        self.mode = Mode::Two { t0, two };
    }

    fn ended(&mut self, t: Touch, cancelled: bool) {
        let Some(at) = self.fingers.iter().position(|f| f.id == t.id) else {
            return;
        };
        let last = self.fingers[at];
        self.fingers.remove(at);
        let pos = if cancelled { last.pos } else { t.pos };
        match self.mode {
            Mode::Pending {
                id,
                start,
                in_view,
                ripe,
                ..
            } if id == t.id => {
                if !cancelled {
                    if ripe {
                        self.long_press(pos);
                    } else if !self.repeats_confirm(start) {
                        self.tap(start, in_view);
                    }
                }
                self.mode = Mode::Idle;
            }
            Mode::Pan { id, .. } if id == t.id => {
                if !cancelled && self.cfg.glide {
                    self.glide = self.lift_speed();
                }
                self.samples.clear();
                self.mode = Mode::Idle;
            }
            Mode::Hover {
                id, start, moved, ripe, ..
            } if id == t.id => {
                if !cancelled {
                    if moved {
                        self.rest_pointer(pos);
                    } else if ripe {
                        self.long_press(pos);
                    } else if !self.repeats_confirm(start) {
                        self.hover_tap(start);
                    }
                }
                self.mode = Mode::Idle;
            }
            Mode::Drag { id } if id == t.id => {
                if !cancelled {
                    self.pointer(pos);
                }
                self.button(Button::Left, false, pos);
                self.mode = Mode::Idle;
            }
            Mode::Corners { b } => {
                let end = if b == t.id {
                    pos
                } else {
                    self.finger(b).map_or(pos, |f| f.pos)
                };
                self.button(Button::Left, false, end);
                self.mode = Mode::Ignore;
            }
            Mode::Held => self.mode = Mode::Idle,
            Mode::Menu { t0, moved } => {
                if self.fingers.is_empty() {
                    if !cancelled && !moved && self.now - t0 <= MENU_TAP_MS {
                        self.key(Key::F(10), true);
                        self.next_frame();
                        self.key(Key::F(10), false);
                    }
                    self.mode = Mode::Idle;
                } else if cancelled {
                    self.mode = Mode::Ignore;
                }
            }
            Mode::Two { .. } => self.mode = Mode::Ignore,
            Mode::Idle
            | Mode::Pending { .. }
            | Mode::Pan { .. }
            | Mode::Hover { .. }
            | Mode::Drag { .. }
            | Mode::Ignore => {}
        }
        if self.fingers.is_empty() {
            self.starts.clear();
            if matches!(self.mode, Mode::Ignore) {
                self.mode = Mode::Idle;
            }
        }
    }

    /// A pan's speed at its lift, in points a millisecond, if fast enough to glide.
    fn lift_speed(&self) -> Option<Pt> {
        let (&(t0, p0), &(t1, p1)) = (self.samples.front()?, self.samples.back()?);
        // A finger that stopped before lifting does not glide.
        if self.now - t1 > VELOCITY_MS / 2.0 {
            return None;
        }
        let span = (self.now - t0).max(1.0) as f32;
        let v = ((p1.0 - p0.0) / span, (p1.1 - p0.1) / span);
        (dist(v, (0.0, 0.0)) >= GLIDE_START_MM_S * self.cfg.mm / 1000.0).then_some(v)
    }

    /// A tap at `at`: the button goes down, and up in the next frame. A tap soon after another, near it, clicks at
    /// the first one's very point, so the game sees a double click.
    fn tap(&mut self, at: Pt, in_view: bool) {
        let at = match self.last_tap {
            Some((p, t)) if self.now - t <= DOUBLE_TAP_MS && dist(p, at) <= DOUBLE_TAP_MM * self.cfg.mm => p,
            _ => at,
        };
        self.last_tap = Some((at, self.now));
        self.button(Button::Left, true, at);
        self.cur.tap_press = in_view;
        self.next_frame();
        self.button(Button::Left, false, at);
        self.cur.tap = true;
    }

    /// A tap while placing or targeting: on the outline or near the pointer it confirms there, elsewhere the pointer
    /// moves to it.
    fn hover_tap(&mut self, at: Pt) {
        let on_ghost = self.cfg.ghost.is_some_and(|g| g.contains(at));
        let near = self
            .hover_at
            .filter(|&p| on_ghost || dist(p, at) <= CONFIRM_MM * self.cfg.mm);
        match near {
            Some(p) => {
                self.button(Button::Left, true, p);
                self.next_frame();
                self.button(Button::Left, false, p);
                self.confirmed = Some((at, self.now));
            }
            None => self.rest_pointer(at),
        }
    }

    /// Whether a tap at `at` repeats the last confirm (the module doc): soon after it and on the same spot, as for a
    /// double tap. Such a tap is dropped, and the next one is measured from it.
    fn repeats_confirm(&mut self, at: Pt) -> bool {
        let repeats = self
            .confirmed
            .is_some_and(|(p, t)| self.now - t <= DOUBLE_TAP_MS && dist(p, at) <= DOUBLE_TAP_MM * self.cfg.mm);
        self.confirmed = repeats.then_some((at, self.now));
        repeats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 points to a millimetre: the slop is 15 points, a pan or pinch starts at 15.
    const MM: f32 = 5.0;
    const FRAME: Duration = Duration::from_millis(16);

    fn t(id: u64, phase: TouchPhase, x: f32, y: f32) -> Touch {
        Touch { id, phase, pos: (x, y) }
    }
    fn down(id: u64, x: f32, y: f32) -> Touch {
        t(id, TouchPhase::Began, x, y)
    }
    fn mv(id: u64, x: f32, y: f32) -> Touch {
        t(id, TouchPhase::Moved, x, y)
    }
    fn up(id: u64, x: f32, y: f32) -> Touch {
        t(id, TouchPhase::Ended, x, y)
    }
    fn cancel(id: u64, x: f32, y: f32) -> Touch {
        t(id, TouchPhase::Cancelled, x, y)
    }

    /// Runs frames of `FRAME` with `events` in the first and returns every frame the machine produced.
    fn run(g: &mut Gesture, events: &[Touch]) -> Vec<Frame> {
        step(g, events, 1)
    }

    fn step(g: &mut Gesture, events: &[Touch], frames: u32) -> Vec<Frame> {
        let mut out = Vec::new();
        for i in 0..frames {
            let c = g.cfg;
            g.update(FRAME, if i == 0 { events } else { &[] }, &c);
        }
        while let Some(f) = g.pop() {
            out.push(f);
        }
        out
    }

    fn waits(g: &mut Gesture, ms: u64) -> Vec<Frame> {
        let mut out = Vec::new();
        let mut left = ms as i64;
        while left > 0 {
            let c = g.cfg;
            g.update(FRAME, &[], &c);
            left -= FRAME.as_millis() as i64;
            while let Some(f) = g.pop() {
                out.push(f);
            }
        }
        out
    }

    fn buttons(frames: &[Frame]) -> Vec<(usize, Button, bool)> {
        frames
            .iter()
            .enumerate()
            .flat_map(|(i, f)| f.buttons.iter().map(move |&(b, d)| (i, b, d)))
            .collect()
    }

    #[test]
    fn a_tap_is_a_left_press_and_a_release_in_the_next_frame() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 80.0)]);
        assert!(frames.is_empty(), "nothing until the finger decides");
        frames.extend(waits(&mut g, 100));
        frames.extend(run(&mut g, &[up(1, 102.0, 81.0)]));
        // The press is where the finger went down.
        assert_eq!(
            buttons(&frames),
            [(0, Button::Left, true), (1, Button::Left, false)],
            "{frames:?}"
        );
        assert_eq!(frames[0].pointer, Some((100.0, 80.0)));
        assert_eq!(frames[1].pointer, Some((100.0, 80.0)));
        assert!(!frames[0].tap && frames[1].tap, "the release carries the tap");
        assert!(g.idle());
    }

    #[test]
    fn a_tap_that_starts_and_ends_in_one_frame_still_takes_two() {
        let mut g = plain();
        let frames = run(&mut g, &[down(1, 10.0, 10.0), up(1, 10.0, 10.0)]);
        assert_eq!(buttons(&frames), [(0, Button::Left, true), (1, Button::Left, false)]);
    }

    /// The map view in the tests' window: 600 × 400 points from the corner.
    const VIEW: Zone = Zone {
        x: 0.0,
        y: 0.0,
        w: 600.0,
        h: 400.0,
    };

    fn with(cfg: Config) -> Gesture {
        Gesture {
            cfg: Config { mm: MM, ..cfg },
            ..Gesture::default()
        }
    }

    /// No map view: a menu, or the old tests' world where a drag is the left button's.
    fn plain() -> Gesture {
        with(Config::default())
    }

    /// The game screen, with the default settings.
    fn on_map() -> Gesture {
        with(Config {
            view: Some(VIEW),
            ..Config::default()
        })
    }

    fn total_pan(frames: &[Frame]) -> Pt {
        frames.iter().fold((0.0, 0.0), |a, f| (a.0 + f.pan.0, a.1 + f.pan.1))
    }

    #[test]
    fn a_tap_on_the_map_is_marked_for_the_smart_order() {
        let mut g = on_map();
        let frames = run(&mut g, &[down(1, 100.0, 100.0), up(1, 100.0, 100.0)]);
        assert_eq!(buttons(&frames), [(0, Button::Left, true), (1, Button::Left, false)]);
        assert!(frames[0].tap_press && !frames[1].tap_press);
        // Off the map view (the panel, the minimap) it is a plain click.
        let frames = run(&mut g, &[down(1, 620.0, 100.0), up(1, 620.0, 100.0)]);
        assert!(frames.iter().all(|f| !f.tap_press), "{frames:?}");
    }

    #[test]
    fn one_finger_dragging_on_the_map_pans_without_a_click() {
        let mut g = on_map();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        // 10 points: within the slop.
        frames.extend(run(&mut g, &[mv(1, 110.0, 100.0)]));
        assert!(frames.is_empty(), "{frames:?}");
        frames.extend(run(&mut g, &[mv(1, 130.0, 110.0)]));
        assert_eq!(total_pan(&frames), (30.0, 10.0), "the travel from the start counts");
        frames.extend(run(&mut g, &[mv(1, 140.0, 110.0)]));
        assert_eq!(total_pan(&frames), (40.0, 10.0));
        // A slow lift (the finger rested first) does not glide.
        frames.extend(waits(&mut g, 200));
        frames.extend(run(&mut g, &[up(1, 140.0, 110.0)]));
        frames.extend(waits(&mut g, 200));
        assert_eq!(total_pan(&frames), (40.0, 10.0));
        assert!(buttons(&frames).is_empty(), "{frames:?}");
        assert!(frames.iter().all(|f| f.pointer.is_none()), "a pan moves no pointer");
        assert!(g.idle());
    }

    #[test]
    fn a_flick_glides_on_and_slows_to_a_stop() {
        let mut g = on_map();
        run(&mut g, &[down(1, 100.0, 100.0)]);
        // 20 points a frame of 16 ms: 250 mm a second at 5 points a millimetre.
        let mut x = 100.0;
        for _ in 0..5 {
            x += 20.0;
            run(&mut g, &[mv(1, x, 100.0)]);
        }
        run(&mut g, &[up(1, x, 100.0)]);
        let first = run(&mut g, &[]);
        assert!(first[0].pan.0 > 10.0, "it carries on: {first:?}");
        let later = waits(&mut g, 300);
        let last = later.last().unwrap().pan.0;
        assert!(last > 0.0 && last < first[0].pan.0 / 2.0, "it slows: {last}");
        waits(&mut g, 3000);
        assert!(g.idle(), "and stops");
        // A finger stops a glide at once.
        let mut g = on_map();
        run(&mut g, &[down(1, 100.0, 100.0)]);
        for i in 1..=5 {
            run(&mut g, &[mv(1, 100.0 + 20.0 * i as f32, 100.0)]);
        }
        run(&mut g, &[up(1, 200.0, 100.0)]);
        let frames = run(&mut g, &[down(1, 300.0, 300.0)]);
        assert!(frames.is_empty(), "{frames:?}");
    }

    #[test]
    fn a_glide_can_be_turned_off() {
        let mut g = with(Config {
            view: Some(VIEW),
            glide: false,
            ..Config::default()
        });
        run(&mut g, &[down(1, 100.0, 100.0)]);
        for i in 1..=5 {
            run(&mut g, &[mv(1, 100.0 + 20.0 * i as f32, 100.0)]);
        }
        run(&mut g, &[up(1, 200.0, 100.0)]);
        assert!(run(&mut g, &[]).is_empty());
    }

    #[test]
    fn holding_then_dragging_on_the_map_draws_the_box() {
        let mut g = on_map();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        frames.extend(waits(&mut g, 280));
        assert!(frames.is_empty(), "not armed yet: {frames:?}");
        frames.extend(waits(&mut g, 40));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].armed, Some((100.0, 100.0)), "the arming shows");
        frames.extend(run(&mut g, &[mv(1, 200.0, 180.0)]));
        frames.extend(run(&mut g, &[up(1, 220.0, 190.0)]));
        let b = buttons(&frames);
        assert_eq!(b.len(), 2, "{frames:?}");
        assert!(b[0].2 && !b[1].2 && b[0].1 == Button::Left);
        let press = frames.iter().find(|f| !f.buttons.is_empty()).unwrap();
        assert_eq!(
            press.pointer,
            Some((100.0, 100.0)),
            "the box starts where the finger landed"
        );
        assert_eq!(frames.last().unwrap().pointer, Some((220.0, 190.0)));
        assert_eq!(total_pan(&frames), (0.0, 0.0));
        assert!(g.idle());
    }

    #[test]
    fn an_armed_finger_still_long_presses_if_it_stays_and_lifts() {
        let mut g = on_map();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        frames.extend(waits(&mut g, 500));
        assert!(
            buttons(&frames).is_empty(),
            "nothing while the finger is down: {frames:?}"
        );
        assert!(frames.iter().any(|f| f.armed.is_some()) && frames.iter().any(|f| f.pulse.is_some()));
        frames.extend(run(&mut g, &[up(1, 100.0, 100.0)]));
        let b = buttons(&frames);
        assert_eq!(
            b.iter().map(|x| x.1).collect::<Vec<_>>(),
            [Button::Right, Button::Right]
        );
    }

    #[test]
    fn a_drag_after_a_long_still_hold_is_the_box_not_a_right_click() {
        let mut g = on_map();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        frames.extend(waits(&mut g, 3000));
        frames.extend(run(&mut g, &[mv(1, 220.0, 190.0)]));
        frames.extend(run(&mut g, &[up(1, 220.0, 190.0)]));
        assert_eq!(
            buttons(&frames).iter().map(|x| (x.1, x.2)).collect::<Vec<_>>(),
            [(Button::Left, true), (Button::Left, false)],
            "{frames:?}"
        );
        assert_eq!(total_pan(&frames), (0.0, 0.0));
        assert!(g.idle());
    }

    #[test]
    fn the_hold_times_follow_the_settings() {
        let mut g = with(Config {
            view: Some(VIEW),
            long_press_ms: 750.0,
            arm_ms: 500.0,
            ..Config::default()
        });
        run(&mut g, &[down(1, 100.0, 100.0)]);
        let frames = waits(&mut g, 480);
        assert!(frames.is_empty());
        let frames = waits(&mut g, 40);
        assert!(frames[0].armed.is_some());
        let frames = waits(&mut g, 200);
        assert!(frames.iter().all(|f| f.pulse.is_none()), "no long press before 750 ms");
        let frames = waits(&mut g, 60);
        assert!(frames[0].pulse.is_some());
        assert_eq!(buttons(&run(&mut g, &[up(1, 100.0, 100.0)])).len(), 2);
    }

    #[test]
    fn the_box_setting_makes_a_drag_the_box_at_once() {
        let mut g = with(Config {
            view: Some(VIEW),
            one_finger: OneFinger::Box,
            ..Config::default()
        });
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        frames.extend(run(&mut g, &[mv(1, 150.0, 100.0)]));
        frames.extend(run(&mut g, &[up(1, 150.0, 100.0)]));
        assert_eq!(buttons(&frames).len(), 2, "{frames:?}");
        assert_eq!(total_pan(&frames), (0.0, 0.0));
        // Nothing arms either: a still finger is only ever a long press.
        let mut g = with(Config {
            view: Some(VIEW),
            one_finger: OneFinger::Box,
            ..Config::default()
        });
        run(&mut g, &[down(1, 100.0, 100.0)]);
        assert!(waits(&mut g, 400).iter().all(|f| f.armed.is_none()));
    }

    #[test]
    fn a_double_tap_clicks_twice_at_the_first_point() {
        let mut g = on_map();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0), up(1, 100.0, 100.0)]);
        frames.extend(waits(&mut g, 100));
        // 20 points (4 mm) off: the same spot for a finger.
        frames.extend(run(&mut g, &[down(1, 112.0, 116.0), up(1, 112.0, 116.0)]));
        let presses: Vec<_> = frames
            .iter()
            .filter(|f| f.buttons.contains(&(Button::Left, true)))
            .map(|f| f.pointer)
            .collect();
        assert_eq!(presses, [Some((100.0, 100.0)), Some((100.0, 100.0))]);
        // Too late, or too far: where the finger is.
        waits(&mut g, 600);
        let frames = run(&mut g, &[down(1, 112.0, 116.0), up(1, 112.0, 116.0)]);
        assert_eq!(frames[0].pointer, Some((112.0, 116.0)));
        let frames = run(&mut g, &[down(1, 200.0, 116.0), up(1, 200.0, 116.0)]);
        assert_eq!(frames[0].pointer, Some((200.0, 116.0)));
    }

    #[test]
    fn two_fingers_can_be_the_box_corners() {
        let mut g = with(Config {
            view: Some(VIEW),
            two_finger: TwoFinger::Corners,
            ..Config::default()
        });
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        frames.extend(run(&mut g, &[down(2, 200.0, 150.0)]));
        frames.extend(run(&mut g, &[mv(2, 250.0, 180.0)]));
        frames.extend(run(&mut g, &[up(2, 250.0, 180.0)]));
        frames.extend(run(&mut g, &[up(1, 100.0, 100.0)]));
        let b = buttons(&frames);
        assert_eq!(b.len(), 2, "{frames:?}");
        assert_eq!(frames[0].pointer, Some((100.0, 100.0)));
        assert_eq!(frames[0].buttons, [(Button::Left, true)]);
        let release = frames.iter().find(|f| f.buttons == [(Button::Left, false)]).unwrap();
        assert_eq!(release.pointer, Some((250.0, 180.0)));
        assert_eq!(total_pan(&frames), (0.0, 0.0));
        assert!(g.idle());
    }

    #[test]
    fn two_finger_pan_and_pinch_can_be_turned_off() {
        let mut g = with(Config {
            view: Some(VIEW),
            two_finger: TwoFinger::Off,
            pinch: false,
            menu_tap: false,
            ..Config::default()
        });
        run(&mut g, &[down(1, 100.0, 100.0), down(2, 200.0, 100.0)]);
        let mut frames = run(&mut g, &[mv(1, 50.0, 150.0), mv(2, 250.0, 150.0)]);
        frames.extend(run(&mut g, &[down(3, 300.0, 300.0)]));
        frames.extend(run(
            &mut g,
            &[up(1, 50.0, 150.0), up(2, 250.0, 150.0), up(3, 300.0, 300.0)],
        ));
        assert!(frames.is_empty(), "{frames:?}");
        // The pinch alone stays when the pan is off.
        let mut g = with(Config {
            view: Some(VIEW),
            two_finger: TwoFinger::Off,
            ..Config::default()
        });
        run(&mut g, &[down(1, 100.0, 100.0), down(2, 200.0, 100.0)]);
        let frames = run(&mut g, &[mv(1, 70.0, 130.0), mv(2, 230.0, 130.0)]);
        assert!(frames[0].zoom.is_some() && frames[0].pan == (0.0, 0.0), "{frames:?}");
    }

    fn placing(ghost: Option<Zone>) -> Gesture {
        with(Config {
            view: Some(VIEW),
            hover: true,
            ghost,
            ..Config::default()
        })
    }

    #[test]
    fn placing_a_tap_moves_the_outline_and_a_second_tap_on_it_confirms() {
        let mut g = placing(None);
        let frames = run(&mut g, &[down(1, 300.0, 200.0), up(1, 300.0, 200.0)]);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].pointer, Some((300.0, 200.0)));
        assert!(frames[0].buttons.is_empty(), "the first tap only moves it");
        assert_eq!(g.hover_pointer(), Some((300.0, 200.0)));
        // Somewhere else: it moves again.
        let frames = run(&mut g, &[down(1, 400.0, 250.0), up(1, 400.0, 250.0)]);
        assert!(buttons(&frames).is_empty());
        // On the outline (the zone the caller says it covers), a click at the pointer, not the finger.
        g.cfg.ghost = Some(Zone {
            x: 390.0,
            y: 240.0,
            w: 80.0,
            h: 80.0,
        });
        let frames = run(&mut g, &[down(1, 450.0, 300.0), up(1, 450.0, 300.0)]);
        assert_eq!(buttons(&frames), [(0, Button::Left, true), (1, Button::Left, false)]);
        assert_eq!(frames[0].pointer, Some((400.0, 250.0)));
        assert!(frames.iter().all(|f| !f.tap && !f.tap_press));
        // Near the pointer counts too (no outline: a spell's target).
        let mut g = placing(None);
        run(&mut g, &[down(1, 300.0, 200.0), up(1, 300.0, 200.0)]);
        let frames = run(&mut g, &[down(1, 320.0, 210.0), up(1, 320.0, 210.0)]);
        assert_eq!(frames[0].pointer, Some((300.0, 200.0)));
        assert_eq!(buttons(&frames).len(), 2);
    }

    /// The iPad report: a double tap on the outline built nothing. The first tap confirmed and ended placement; the
    /// second came as an ordinary tap, which the smart tap turned into a right click, so the builder walked to the site
    /// and stopped. That second tap is dropped now, whether placement has ended by then or not.
    #[test]
    fn placing_a_double_tap_on_the_outline_confirms_once() {
        let ghost = Zone {
            x: 280.0,
            y: 180.0,
            w: 64.0,
            h: 64.0,
        };
        let mut g = placing(Some(ghost));
        run(&mut g, &[down(1, 300.0, 200.0), up(1, 300.0, 200.0)]);
        let mut frames = run(&mut g, &[down(1, 310.0, 210.0), up(1, 310.0, 210.0)]);
        assert_eq!(buttons(&frames).len(), 2, "the confirm: {frames:?}");
        // The game ends placement mode.
        g.cfg.hover = false;
        g.cfg.ghost = None;
        frames.extend(waits(&mut g, 100));
        // 20 points (4 mm) off: the same spot for a finger.
        frames.extend(run(&mut g, &[down(1, 322.0, 226.0), up(1, 322.0, 226.0)]));
        assert_eq!(buttons(&frames).len(), 2, "no second click: {frames:?}");
        assert!(frames.iter().all(|f| !f.tap_press && !f.tap));
        // A third tap straight after is dropped too.
        frames.extend(waits(&mut g, 100));
        frames.extend(run(&mut g, &[down(1, 318.0, 220.0), up(1, 318.0, 220.0)]));
        assert_eq!(buttons(&frames).len(), 2, "{frames:?}");
        // Later, or elsewhere, a tap is a tap again.
        waits(&mut g, 600);
        let frames = run(&mut g, &[down(1, 318.0, 220.0), up(1, 318.0, 220.0)]);
        assert_eq!(buttons(&frames).len(), 2);
        assert!(frames[0].tap_press);
        let mut g = placing(Some(ghost));
        run(&mut g, &[down(1, 300.0, 200.0), up(1, 300.0, 200.0)]);
        run(&mut g, &[down(1, 310.0, 210.0), up(1, 310.0, 210.0)]);
        g.cfg.hover = false;
        waits(&mut g, 100);
        let frames = run(&mut g, &[down(1, 450.0, 210.0), up(1, 450.0, 210.0)]);
        assert_eq!(buttons(&frames).len(), 2, "30 mm away: {frames:?}");
        // While placement is still on (a refused site keeps it), the repeat doesn't confirm a second time either.
        let mut g = placing(Some(ghost));
        run(&mut g, &[down(1, 300.0, 200.0), up(1, 300.0, 200.0)]);
        run(&mut g, &[down(1, 310.0, 210.0), up(1, 310.0, 210.0)]);
        waits(&mut g, 100);
        let frames = run(&mut g, &[down(1, 312.0, 212.0), up(1, 312.0, 212.0)]);
        assert!(buttons(&frames).is_empty(), "{frames:?}");
    }

    #[test]
    fn placing_a_drag_carries_the_outline_and_leaves_it() {
        let mut g = placing(None);
        let mut frames = run(&mut g, &[down(1, 300.0, 200.0)]);
        frames.extend(run(&mut g, &[mv(1, 340.0, 200.0)]));
        frames.extend(run(&mut g, &[mv(1, 360.0, 220.0)]));
        frames.extend(run(&mut g, &[up(1, 362.0, 221.0)]));
        assert!(buttons(&frames).is_empty(), "{frames:?}");
        assert_eq!(frames.last().unwrap().pointer, Some((362.0, 221.0)));
        assert_eq!(g.hover_pointer(), Some((362.0, 221.0)));
        assert_eq!(total_pan(&frames), (0.0, 0.0));
    }

    #[test]
    fn placing_a_finger_at_the_edge_scrolls_and_does_not_long_press() {
        let mut g = placing(None);
        // 8 mm is 40 points: x 590 is by the right edge.
        let mut frames = run(&mut g, &[down(1, 590.0, 200.0)]);
        frames.extend(waits(&mut g, 600));
        let pan = total_pan(&frames);
        assert!(pan.0 < -100.0 && pan.1 == 0.0, "the view goes right: {pan:?}");
        assert!(buttons(&frames).is_empty());
        // Away from the edge, a still finger is a right click (cancel) when it lifts, never while it is down.
        let mut g = placing(None);
        run(&mut g, &[down(1, 300.0, 200.0)]);
        let frames = waits(&mut g, 2000);
        assert!(buttons(&frames).is_empty(), "{frames:?}");
        assert!(frames.iter().any(|f| f.pulse.is_some()), "the pulse says it is ready");
        let frames = run(&mut g, &[up(1, 300.0, 200.0)]);
        assert_eq!(buttons(&frames)[0].1, Button::Right);
        // A drag after the hold carries the outline instead, and cancels nothing.
        let mut g = placing(None);
        let mut frames = run(&mut g, &[down(1, 300.0, 200.0)]);
        frames.extend(waits(&mut g, 2000));
        frames.extend(run(&mut g, &[mv(1, 360.0, 230.0)]));
        frames.extend(run(&mut g, &[up(1, 360.0, 230.0)]));
        assert!(buttons(&frames).is_empty(), "{frames:?}");
        assert_eq!(g.hover_pointer(), Some((360.0, 230.0)));
    }

    #[test]
    fn placing_ends_and_the_resting_pointer_goes() {
        let mut g = placing(None);
        run(&mut g, &[down(1, 300.0, 200.0), up(1, 300.0, 200.0)]);
        assert!(g.hover_pointer().is_some());
        g.cfg.hover = false;
        run(&mut g, &[]);
        assert_eq!(g.hover_pointer(), None);
    }

    #[test]
    fn a_finger_that_wanders_a_little_is_still_a_tap() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        // 10 points is 2 mm: within the slop of 3.
        frames.extend(run(&mut g, &[mv(1, 106.0, 108.0)]));
        frames.extend(run(&mut g, &[up(1, 106.0, 108.0)]));
        let b = buttons(&frames);
        assert_eq!(b.len(), 2, "{frames:?}");
        assert_eq!(
            frames[0].pointer,
            Some((100.0, 100.0)),
            "it clicks where the finger landed"
        );
    }

    #[test]
    fn a_drag_holds_the_left_button_from_where_the_finger_landed() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        frames.extend(run(&mut g, &[mv(1, 130.0, 100.0)]));
        frames.extend(run(&mut g, &[mv(1, 200.0, 150.0)]));
        frames.extend(run(&mut g, &[up(1, 220.0, 160.0)]));
        let b = buttons(&frames);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].1, Button::Left);
        assert!(b[0].2 && !b[1].2);
        // The press is at the start, the pointer then follows.
        assert_eq!(frames[0].pointer, Some((100.0, 100.0)));
        assert_eq!(frames[0].buttons, [(Button::Left, true)]);
        assert_eq!(
            frames[1].pointer,
            Some((130.0, 100.0)),
            "later frame: the move after the press"
        );
        let last = frames.last().unwrap();
        assert_eq!(last.pointer, Some((220.0, 160.0)));
        assert_eq!(last.buttons, [(Button::Left, false)]);
        assert!(!last.tap, "a drag is not a tap");
        assert!(g.idle());
    }

    #[test]
    fn a_still_finger_long_presses_into_a_right_click_with_a_pulse() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 200.0, 120.0)]);
        frames.extend(waits(&mut g, 400));
        assert!(frames.is_empty(), "not yet at 400 ms: {frames:?}");
        frames.extend(waits(&mut g, 100));
        assert!(
            buttons(&frames).is_empty(),
            "nothing while the finger is down: {frames:?}"
        );
        assert_eq!(frames[0].pulse, Some((200.0, 120.0)), "the pulse says it is ready");
        // The right click comes with the lift, where the finger lifts.
        let frames = run(&mut g, &[up(1, 201.0, 120.0)]);
        assert_eq!(
            buttons(&frames),
            [(0, Button::Right, true), (1, Button::Right, false)],
            "{frames:?}"
        );
        assert_eq!(frames[0].pointer, Some((201.0, 120.0)));
        assert!(g.idle());
    }

    #[test]
    fn a_long_press_allows_small_movement_but_not_a_drag() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 200.0, 120.0)]);
        frames.extend(run(&mut g, &[mv(1, 208.0, 126.0)]));
        frames.extend(waits(&mut g, 500));
        frames.extend(run(&mut g, &[up(1, 208.0, 126.0)]));
        assert_eq!(buttons(&frames).len(), 2, "still a long press: {frames:?}");
        assert_eq!(buttons(&frames)[0].1, Button::Right);
        // Past the slop it is a drag instead, however long the wait.
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 200.0, 120.0)]);
        frames.extend(waits(&mut g, 300));
        frames.extend(run(&mut g, &[mv(1, 260.0, 120.0)]));
        frames.extend(waits(&mut g, 600));
        assert_eq!(buttons(&frames), [(0, Button::Left, true)], "{frames:?}");
        assert!(frames.iter().all(|f| f.pulse.is_none()));
    }

    #[test]
    fn a_tap_just_before_the_long_press_time_is_still_a_tap() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 20.0, 20.0)]);
        frames.extend(waits(&mut g, 420));
        frames.extend(run(&mut g, &[up(1, 20.0, 20.0)]));
        let b = buttons(&frames);
        assert_eq!(b.iter().map(|x| x.1).collect::<Vec<_>>(), [Button::Left, Button::Left]);
    }

    #[test]
    fn two_fingers_moving_together_pan() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0), down(2, 200.0, 100.0)]);
        // 6 points: under the pan threshold of 15; nothing yet.
        frames.extend(run(&mut g, &[mv(1, 106.0, 100.0), mv(2, 206.0, 100.0)]));
        assert!(frames.is_empty());
        // Past it: the pan includes the travel so far (centre 150 -> 140 is -10 on top of the 6... measured from the start).
        frames.extend(run(&mut g, &[mv(1, 80.0, 110.0), mv(2, 180.0, 110.0)]));
        assert_eq!(frames.len(), 1);
        let p = frames[0].pan;
        assert!((p.0 - -20.0).abs() < 1e-3 && (p.1 - 10.0).abs() < 1e-3, "{p:?}");
        assert!(frames[0].zoom.is_none(), "the distance did not change");
        assert!(frames[0].buttons.is_empty(), "no click came out of a pan");
        // And on: each move adds its own travel.
        let more = run(&mut g, &[mv(1, 70.0, 110.0), mv(2, 170.0, 110.0)]);
        assert!((more[0].pan.0 - -10.0).abs() < 1e-3);
    }

    #[test]
    fn two_fingers_spreading_zoom_around_their_centre() {
        let mut g = plain();
        run(&mut g, &[down(1, 100.0, 100.0), down(2, 200.0, 100.0)]);
        // Spread from 100 apart to 160: factor 1.6 around the centre (150, 100); the pan stays quiet.
        let frames = run(&mut g, &[mv(1, 70.0, 100.0), mv(2, 230.0, 100.0)]);
        assert_eq!(frames.len(), 1);
        let (f, c) = frames[0].zoom.expect("a zoom");
        assert!((f - 1.6).abs() < 1e-3, "{f}");
        assert_eq!(c, (150.0, 100.0));
        assert_eq!(frames[0].pan, (0.0, 0.0));
        // Pinching in again is a factor under one.
        let frames = run(&mut g, &[mv(1, 100.0, 100.0), mv(2, 200.0, 100.0)]);
        let (f, _) = frames[0].zoom.unwrap();
        assert!((f - 100.0 / 160.0).abs() < 1e-3, "{f}");
    }

    #[test]
    fn several_moves_in_one_frame_multiply_their_zoom() {
        let mut g = plain();
        run(&mut g, &[down(1, 100.0, 100.0), down(2, 200.0, 100.0)]);
        let frames = run(
            &mut g,
            &[
                mv(1, 80.0, 100.0),
                mv(2, 200.0, 100.0),
                mv(1, 60.0, 100.0),
                mv(2, 200.0, 100.0),
            ],
        );
        let (f, _) = frames[0].zoom.unwrap();
        assert!((f - 1.4).abs() < 1e-3, "{f}");
    }

    #[test]
    fn a_second_finger_cancels_an_undecided_first_without_a_click() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        frames.extend(run(&mut g, &[down(2, 180.0, 100.0)]));
        frames.extend(run(&mut g, &[up(2, 180.0, 100.0), up(1, 100.0, 100.0)]));
        assert!(frames.is_empty(), "{frames:?}");
        assert!(g.idle());
    }

    #[test]
    fn a_second_finger_during_a_drag_lets_go_of_the_button() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0), mv(1, 150.0, 100.0)]);
        frames.extend(run(&mut g, &[down(2, 300.0, 300.0)]));
        let b = buttons(&frames);
        assert_eq!(b.len(), 2, "{frames:?}");
        assert!(b[0].2 && !b[1].2, "pressed, then released where the finger is");
        assert_eq!(frames.last().unwrap().pointer, Some((150.0, 100.0)));
    }

    #[test]
    fn after_a_pinch_the_remaining_finger_is_ignored() {
        let mut g = plain();
        run(&mut g, &[down(1, 100.0, 100.0), down(2, 200.0, 100.0)]);
        let mut frames = run(&mut g, &[up(2, 200.0, 100.0)]);
        // The first finger stays and drags far: no click, no box.
        frames.extend(run(&mut g, &[mv(1, 300.0, 300.0)]));
        frames.extend(waits(&mut g, 600));
        frames.extend(run(&mut g, &[up(1, 300.0, 300.0)]));
        assert!(frames.is_empty(), "{frames:?}");
        // A new touch afterwards works.
        let frames = run(&mut g, &[down(1, 10.0, 10.0), up(1, 10.0, 10.0)]);
        assert_eq!(buttons(&frames).len(), 2);
    }

    #[test]
    fn three_fingers_tapping_open_the_menu() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0), down(2, 150.0, 100.0)]);
        frames.extend(run(&mut g, &[down(3, 200.0, 100.0)]));
        frames.extend(waits(&mut g, 100));
        assert!(frames.is_empty(), "nothing before they lift");
        frames.extend(run(
            &mut g,
            &[up(1, 100.0, 100.0), up(2, 150.0, 100.0), up(3, 200.0, 100.0)],
        ));
        let keys: Vec<(usize, Key, bool)> = frames
            .iter()
            .enumerate()
            .flat_map(|(i, f)| f.keys.iter().map(move |&(k, d)| (i, k, d)))
            .collect();
        assert_eq!(keys, [(0, Key::F(10), true), (1, Key::F(10), false)], "{frames:?}");
        assert!(buttons(&frames).is_empty());
        assert!(g.idle());
    }

    #[test]
    fn three_fingers_held_too_long_or_moved_do_not_open_the_menu() {
        let mut g = plain();
        run(
            &mut g,
            &[down(1, 100.0, 100.0), down(2, 150.0, 100.0), down(3, 200.0, 100.0)],
        );
        let mut frames = waits(&mut g, 600);
        frames.extend(run(
            &mut g,
            &[up(1, 100.0, 100.0), up(2, 150.0, 100.0), up(3, 200.0, 100.0)],
        ));
        assert!(frames.is_empty(), "held too long: {frames:?}");
        let mut g = plain();
        run(
            &mut g,
            &[down(1, 100.0, 100.0), down(2, 150.0, 100.0), down(3, 200.0, 100.0)],
        );
        let mut frames = run(&mut g, &[mv(3, 300.0, 100.0)]);
        frames.extend(run(
            &mut g,
            &[up(1, 100.0, 100.0), up(2, 150.0, 100.0), up(3, 300.0, 100.0)],
        ));
        assert!(frames.is_empty(), "moved: {frames:?}");
        assert!(g.idle());
    }

    #[test]
    fn three_fingers_may_land_a_few_frames_apart() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        frames.extend(step(&mut g, &[down(2, 150.0, 100.0)], 2));
        frames.extend(step(&mut g, &[down(3, 200.0, 100.0)], 2));
        frames.extend(step(&mut g, &[up(2, 150.0, 100.0)], 1));
        frames.extend(step(&mut g, &[up(1, 100.0, 100.0), up(3, 200.0, 100.0)], 1));
        assert_eq!(
            frames.iter().filter(|f| f.keys.contains(&(Key::F(10), true))).count(),
            1
        );
    }

    #[test]
    fn a_cancelled_drag_releases_the_button_and_nothing_else() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0), mv(1, 150.0, 100.0)]);
        frames.extend(run(&mut g, &[cancel(1, 150.0, 100.0)]));
        let b = buttons(&frames);
        assert_eq!(b.len(), 2);
        assert!(!b[1].2, "the button is up again");
        assert!(g.idle());
    }

    #[test]
    fn cancelled_taps_and_menu_taps_do_nothing() {
        let mut g = plain();
        let frames = run(&mut g, &[down(1, 100.0, 100.0), cancel(1, 100.0, 100.0)]);
        assert!(frames.is_empty());
        assert!(g.idle());
        // A cancel in a three-finger tap spoils it.
        let mut frames = run(&mut g, &[down(1, 1.0, 1.0), down(2, 50.0, 1.0), down(3, 100.0, 1.0)]);
        frames.extend(run(&mut g, &[cancel(2, 50.0, 1.0)]));
        frames.extend(run(&mut g, &[up(1, 1.0, 1.0), up(3, 100.0, 1.0)]));
        assert!(frames.is_empty(), "{frames:?}");
        assert!(g.idle());
    }

    #[test]
    fn a_finger_moved_after_a_long_press_drags_instead() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 100.0, 100.0)]);
        frames.extend(waits(&mut g, 500));
        frames.extend(run(&mut g, &[mv(1, 300.0, 300.0)]));
        frames.extend(run(&mut g, &[up(1, 300.0, 300.0)]));
        assert_eq!(
            buttons(&frames).iter().map(|x| (x.1, x.2)).collect::<Vec<_>>(),
            [(Button::Left, true), (Button::Left, false)],
            "no right click: {frames:?}"
        );
    }

    #[test]
    fn reset_lets_go_of_a_drag() {
        let mut g = plain();
        run(&mut g, &[down(1, 100.0, 100.0), mv(1, 150.0, 100.0)]);
        g.reset();
        let frames: Vec<_> = std::iter::from_fn(|| g.pop()).collect();
        assert_eq!(buttons(&frames).last().map(|b| b.2), Some(false), "{frames:?}");
        // The finger is still down but ignored; its lift is silent.
        let frames = run(&mut g, &[up(1, 150.0, 100.0)]);
        assert!(frames.is_empty());
        assert!(g.idle());
    }

    #[test]
    fn two_taps_in_a_row_make_two_clicks() {
        let mut g = plain();
        let mut frames = run(&mut g, &[down(1, 10.0, 10.0), up(1, 10.0, 10.0)]);
        frames.extend(run(&mut g, &[down(1, 12.0, 11.0), up(1, 12.0, 11.0)]));
        let b = buttons(&frames);
        assert_eq!(b.len(), 4, "{frames:?}");
        assert!(b.iter().filter(|x| x.2).count() == 2);
    }
}
