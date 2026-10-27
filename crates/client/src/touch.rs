//! The core's touch step (`docs/design/touch.md`): the glue between the fingers' events ([`Input::touches`]), the chrome
//! ([`crate::chrome`]), the gesture machine ([`crate::gesture`]) and the mouse-and-keyboard input the rest of the core
//! reads. It runs in the frame's input step, after a scripted run's input is in and before the frame's events are
//! applied, and does nothing at all until a finger appears (or `--touch-emulation` turns the mouse into one), so
//! without touch every frame is exactly what it was.
//!
//! **What it hands the game.**
//! - Buttons and keys, through [`Input::button`] and [`Input::key`], one gesture frame a game frame.
//! - The pointer ([`TouchState::place_pointer`]): where the gesture last put it while a finger drags or a click is in
//!   progress, nowhere otherwise (a lifted finger leaves no cursor to scroll the edge of the map).
//! - A pan and a zoom for the map view (`game::touch_view`), or, outside the game, the pan as mouse-wheel lines for
//!   lists.
//! - The tap tolerance ([`Input::tap_radius`]) for the frame a tap's left button comes up.
//! - The smart tap (option `touch-smart-tap`, `mouse::smart_tap`): with units of yours selected, a tap on the map
//!   view that is not on one of your own units is a right click, press and release; the chrome's plain-click latch
//!   makes the next tap a left click.
//! - The chrome's keys: the group buttons (a tap is the digit, a hold Ctrl and the digit), the menu (F10) and the
//!   idle-worker button (F6, `crate::idle::KEY`), each let go a frame later.
//! - In the Controller and touch mode with a controller connected (options `controls` and `pad-hide-touch`), no
//!   chrome: it neither shows nor takes fingers. In the Controller and Keyboard and mouse modes ([`crate::controls`])
//!   fingers are dropped before the chrome and the gesture machine ([`park`]).
//! - A pulse where a long press fired, drawn onto the picture after it is composed ([`draw_pulse`]).
//!
//! [`Input::touches`]: crate::input::Input::touches
//! [`Input::button`]: crate::input::Input::button
//! [`Input::key`]: crate::input::Input::key
//! [`Input::tap_radius`]: crate::input::Input::tap_radius

use std::time::Duration;

use crate::chrome::{self, Chrome, Control, Fed, Latch, Overlay};
use crate::core::{Core, game_running};
use crate::extras::{TouchSettings, TouchTwoFinger};
use crate::gesture::{self, Gesture, OneFinger, Pt, TwoFinger, Zone};
use crate::input::{Button, Key, KeyEvent, Touch, TouchPhase, Wheel, WindowGeom};
use crate::states::AppState;
use crate::touchemu::Emulator;

/// How far a tap may miss a unit and still select it, in millimetres.
pub const TAP_MM: f32 = 6.0;
/// How far fingers travel to scroll a list by a line, in millimetres.
pub const LINE_MM: f32 = 7.0;
/// How long the long-press pulse shows, in milliseconds.
pub const PULSE_MS: f32 = 400.0;
/// A screen whose short side is under this many millimetres is a phone's, where the pinch zoom is on by default.
pub const PHONE_MM: f32 = 90.0;
/// The status bar's prompt while placing a building with a finger: a tap away from the outline only moves it there.
pub const PLACE_PROMPT: &str = "Tap the outline to build; drag to move it";
/// The status bar's prompt while choosing a target with a finger: a tap away from the pointer only moves it there.
pub const TARGET_PROMPT: &str = "Tap a target, then tap it again to confirm";

/// The core's touch state.
#[derive(Default)]
pub struct TouchState {
    /// The mouse is a finger (`--touch-emulation`).
    pub emulate: bool,
    /// Touch is in use: the chrome shows and the pointer is the fingers'.
    pub active: bool,
    /// The platform has no mouse to speak of (a phone or tablet): touch is always the main input.
    pub touch_only: bool,
    /// A real finger has touched in this run: touch is there for the `controls` option (`crate::controls`).
    pub seen: bool,
    /// A real mouse has moved in this run on a phone or tablet, which has none to start with.
    pub mouse_seen: bool,
    /// Touch was in use when the `controls` option turned fingers off: it comes back when they are on again.
    parked: bool,
    /// The platform's pointer last frame, to see a mouse on a phone or tablet.
    mouse_last: Option<Option<Pt>>,
    gesture: Gesture,
    pub chrome: Chrome,
    emu: Emulator,
    pointer: Option<Pt>,
    /// The pulse: where, and how old in milliseconds.
    pulse: Option<(Pt, f32)>,
    /// The map view's pan and zoom not yet taken by the game, in window points.
    pan: Pt,
    zoom: Option<(f32, Pt)>,
    /// Map pixels panned short of a tile (`game::touch_view`).
    pub view_acc: Pt,
    wheel_acc: f32,
    /// The one-shot latches come up at the next step (a locked one stays).
    release_mods: bool,
    /// Milliseconds of touch steps so far, and the latch last turned on and when, for the double tap that locks it.
    clock: f32,
    latched_at: Option<(Control, f32)>,
    /// Keys the chrome pressed, let go at the next step.
    queued: Vec<(Key, bool)>,
    /// The tap whose press went out as a right click (the smart tap): its release does too.
    tap_right: bool,
    pause: bool,
    /// The platform's pointer the last frame, to see a mouse come back.
    last_platform: Option<Option<Pt>>,
    /// The chrome is hidden ([`chrome_hidden`]): for a controller, or by a widescreen game screen.
    hidden: bool,
    /// The chrome as the shell last drew it, and a count of its changes.
    drawn: Chrome,
    drawn_hidden: bool,
    pub revision: u64,
    /// A text box wanted typing last step ([`text_wanted`]), and the keyboard is open because it did.
    text_was: bool,
    auto_keyboard: bool,
    /// Fingers spent closing the keyboard ([`TouchState::text_tap`]): the rest of their touch goes nowhere.
    spent: Vec<u64>,
}

impl TouchState {
    /// The state for a run: `emulate` makes the mouse a finger from the start.
    pub fn new(emulate: bool) -> Self {
        TouchState {
            emulate,
            active: emulate,
            ..Default::default()
        }
    }

    /// Notes a real mouse on a phone or tablet (a pointer that moved with no finger down), for the `controls`
    /// option's list of what is there. Called with the platform's pointer, before the fingers' pointer replaces it.
    pub fn note_mouse(&mut self, pointer: Option<Pt>, scripted: bool) {
        if !self.touch_only || scripted || self.mouse_seen {
            return;
        }
        let last = self.mouse_last.replace(pointer);
        if last.is_some_and(|l| l != pointer) && pointer.is_some() && !self.gesture.touching() {
            self.mouse_seen = true;
        }
    }

    /// The chrome's picture for a window, `None` while touch is not in use. Draw it over the picture; it changes only
    /// when [`TouchState::revision`] does.
    pub fn overlay(&self, size: (u32, u32), factor: f32) -> Option<Overlay> {
        (self.active && !self.hidden).then(|| self.chrome.overlay(size, factor))
    }

    /// The map view's pan (window points) and zoom (a factor and its centre in window points) since the last call.
    pub fn take_view(&mut self) -> (Pt, Option<(f32, Pt)>) {
        (std::mem::take(&mut self.pan), self.zoom.take())
    }

    /// Whether the pause control was pressed since the last call.
    pub fn take_pause(&mut self) -> bool {
        std::mem::take(&mut self.pause)
    }

    /// The system hid the soft keyboard by itself (its hide key, Back, a tap elsewhere): the keyboard button goes
    /// dark, so the next tap on it opens the keyboard again rather than "closing" one that is already gone.
    pub fn keyboard_hidden(&mut self) {
        self.chrome.keyboard = false;
        self.auto_keyboard = false;
    }

    /// The pointer after a platform's or a scene's (called once the frame's pointer is set): while touch is in use it
    /// is the fingers'. A real mouse that moves with no finger down takes the pointer back.
    pub fn place_pointer(&mut self, pointer: &mut Option<Pt>, scripted: bool) {
        if !self.active {
            return;
        }
        if !self.emulate && !scripted {
            let last = self.last_platform.replace(*pointer);
            if last.is_some_and(|l| l != *pointer) && !self.gesture.touching() && self.pointer.is_none() {
                self.active = false;
                self.last_platform = None;
                return;
            }
        }
        *pointer = self.pointer;
    }

    /// A key pressed now and let go at the next step.
    fn stroke(&mut self, k: Key, keys: &mut Vec<(Key, bool)>) {
        keys.push((k, true));
        self.queued.push((k, false));
    }

    fn press(&mut self, c: Control, keys: &mut Vec<(Key, bool)>) {
        match c {
            Control::Shift | Control::Ctrl | Control::Alt => {
                let (latch, key) = match c {
                    Control::Shift => (&mut self.chrome.shift, Key::ShiftLeft),
                    Control::Ctrl => (&mut self.chrome.ctrl, Key::ControlLeft),
                    _ => (&mut self.chrome.alt, Key::AltLeft),
                };
                // A second tap soon after the one that turned it on locks it (`chrome::Latch`).
                let quick = self
                    .latched_at
                    .is_some_and(|(at, when)| at == c && self.clock - when < chrome::LOCK_MS);
                let was = *latch;
                *latch = latch.tapped(quick);
                if *latch != was && was.held() != latch.held() {
                    keys.push((key, latch.held()));
                }
                self.latched_at = (*latch == Latch::Once).then_some((c, self.clock));
            }
            Control::Plain => self.chrome.plain = !self.chrome.plain,
            Control::Pause => self.pause = true,
            Control::Keyboard => {
                self.chrome.keyboard = !self.chrome.keyboard;
                self.auto_keyboard = false;
            }
            Control::Menu => self.stroke(Key::F(10), keys),
            Control::IdleWorker => self.stroke(Key::F(crate::idle::KEY), keys),
            Control::Group(n) => self.stroke(Key::Digit(n), keys),
        }
    }

    /// After a click: the one-shot latches let go; a locked one stays held.
    /// Lets go every latch, locked or not.
    fn drop_latches(&mut self, keys: &mut Vec<(Key, bool)>) {
        for (latch, k) in [
            (&mut self.chrome.shift, Key::ShiftLeft),
            (&mut self.chrome.ctrl, Key::ControlLeft),
            (&mut self.chrome.alt, Key::AltLeft),
        ] {
            if latch.held() {
                keys.push((k, false));
            }
            *latch = Latch::Off;
        }
        self.latched_at = None;
    }

    /// A finger while a text box wants typing (`area`, window points; "Text boxes"): landing outside the box with the
    /// keyboard open closes the keyboard and is spent on that (true), so no keyboard needs a hide key of its own;
    /// landing on the box with it closed opens it again and goes on to the box.
    fn text_tap(&mut self, f: Touch, area: Option<(f32, f32, f32, f32)>) -> bool {
        if let Some(i) = self.spent.iter().position(|&id| id == f.id) {
            if matches!(f.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.spent.swap_remove(i);
            }
            return true;
        }
        let (Some((x, y, w, h)), TouchPhase::Began) = (area, f.phase) else {
            return false;
        };
        let inside = (x..=x + w).contains(&f.pos.0) && (y..=y + h).contains(&f.pos.1);
        if self.chrome.keyboard && !inside {
            self.chrome.keyboard = false;
            self.auto_keyboard = false;
            self.spent.push(f.id);
            return true;
        }
        if !self.chrome.keyboard && inside {
            self.chrome.keyboard = true;
            self.auto_keyboard = true;
        }
        false
    }

    fn release_latches(&mut self, keys: &mut Vec<(Key, bool)>) {
        for (latch, k) in [
            (&mut self.chrome.shift, Key::ShiftLeft),
            (&mut self.chrome.ctrl, Key::ControlLeft),
            (&mut self.chrome.alt, Key::AltLeft),
        ] {
            if *latch == Latch::Once {
                *latch = Latch::Off;
                keys.push((k, false));
            }
        }
        self.latched_at = None;
    }

    /// A control held long enough: a group button sets its group (Ctrl and the digit).
    fn hold(&mut self, c: Control, keys: &mut Vec<(Key, bool)>) {
        if let Control::Group(n) = c {
            let ctrl = !self.chrome.ctrl.held();
            if ctrl {
                keys.push((Key::ControlLeft, true));
            }
            self.stroke(Key::Digit(n), keys);
            if ctrl {
                self.queued.push((Key::ControlLeft, false));
            }
        }
    }
}

/// Whether a window is a phone's: its short side under [`PHONE_MM`].
pub fn is_phone(w: WindowGeom) -> bool {
    let short = w.size.0.min(w.size.1) as f32 / w.factor.max(0.01);
    short / w.points_per_mm().max(0.01) < PHONE_MM
}

/// The gesture machine's settings from the touch options for a window (`docs/design/touch.md`, "Options").
fn gesture_config(s: &TouchSettings, w: WindowGeom) -> gesture::Config {
    gesture::Config {
        mm: w.points_per_mm(),
        one_finger: if s.drag_pans { OneFinger::Pan } else { OneFinger::Box },
        glide: s.glide,
        two_finger: match s.two_finger {
            TouchTwoFinger::Pan => TwoFinger::Pan,
            TouchTwoFinger::Corners => TwoFinger::Corners,
            TouchTwoFinger::Off => TwoFinger::Off,
        },
        pinch: s.pinch.unwrap_or_else(|| is_phone(w)),
        menu_tap: s.menu_tap,
        long_press_ms: s.long_press_ms,
        arm_ms: s.arm_ms,
        ..Default::default()
    }
}

/// Whether the side bars are hidden: by the mode (`controls::Mode::chrome_shown`: a controller, or no fingers) or by a
/// game screen wider than 640 (`screen_w`, option `widescreen`), whose map fills the pillarbox they sit in. The
/// widescreen option is the touch player's choice between the bars and the wider map.
pub fn chrome_hidden(mode: crate::controls::Mode, hide_touch: bool, screen_w: u32) -> bool {
    !mode.chrome_shown(hide_touch) || screen_w > crate::display::SCREEN.0
}

/// The text box that wants typing this frame (`docs/design/touch.md`, "Text boxes"): the in-game chat line (in game
/// screen pixels), the save dialog's name, or a multiplayer menu's or the lobby's focused box (in 640 x 480 menu
/// pixels, `true`).
pub fn text_box(c: &Core) -> Option<(crate::hud::Rect, bool)> {
    match c.states.get() {
        AppState::Game => {
            if let Some(m) = &c.menu {
                return m.name_box().map(|r| (r, true));
            }
            let chat = c.screen_ui.as_ref().is_some_and(|s| s.status.chat.is_some());
            chat.then(|| c.hud.as_ref().and_then(|h| h.layout))
                .flatten()
                .map(|l| (l.bottom, false))
        }
        AppState::Multiplayer => crate::frontend::mp_text_box(c).map(|r| (r, true)),
        _ => None,
    }
}

/// Whether a text box wants typing this frame ([`text_box`]).
pub fn text_wanted(c: &Core) -> bool {
    text_box(c).is_some()
}

/// The text box in window points (left, top, width, height), for the shell to keep clear of the soft keyboard
/// (`SDL_SetTextInputArea`) and for a finger to be told from a tap elsewhere.
pub fn text_area(c: &Core, w: WindowGeom) -> Option<(f32, f32, f32, f32)> {
    let (r, menu) = text_box(c)?;
    let lb = w.letterbox();
    let o = if menu { lb.flow_origin() } else { (0, 0) };
    let at = |x: i32, y: i32| lb.window_of(((x + o.0) as f32, (y + o.1) as f32), w.factor);
    let (x0, y0) = at(r.x, r.y);
    let (x1, y1) = at(r.x + r.w as i32, r.y + r.h as i32);
    Some((x0, y0, x1 - x0, y1 - y0))
}

/// Opens the soft keyboard as a text box starts wanting typing, and closes it as the box is done if it was opened so.
fn follow_text_boxes(t: &mut TouchState, wanted: bool) {
    if wanted == t.text_was {
        return;
    }
    t.text_was = wanted;
    if wanted && !t.chrome.keyboard {
        t.chrome.keyboard = true;
        t.auto_keyboard = true;
    } else if !wanted && std::mem::take(&mut t.auto_keyboard) {
        t.chrome.keyboard = false;
    }
}

/// One frame's touch step; see the module doc.
pub fn step(c: &mut Core, elapsed: Duration, window: Option<WindowGeom>) {
    c.input.tap_radius = None;
    let mode = crate::controls::mode(c);
    if !mode.fingers() {
        park(c);
        return;
    }
    if std::mem::take(&mut c.touch.parked) {
        c.touch.active = true;
    }
    let fresh = !c.input.touches.is_empty() || !c.input.mouse_touches.is_empty();
    if !(c.touch.active || fresh) {
        return;
    }
    let in_game = game_running(c);
    let wanted = text_wanted(c);
    let raw = std::mem::take(&mut c.input.touches);
    let mice = std::mem::take(&mut c.input.mouse_touches);
    let Some(w) = window else { return };
    let area = text_area(c, w);
    let lb = w.letterbox();
    let ppmm = w.points_per_mm();
    let extras = crate::game::extras(c);
    let settings = extras.touch();
    let (hover, ghost) = if in_game {
        crate::game::touch_aim(c)
    } else {
        (false, None)
    };
    let to_zone = |r: (f32, f32, f32, f32)| {
        let (x0, y0) = lb.window_of((r.0, r.1), w.factor);
        let (x1, y1) = lb.window_of((r.2, r.3), w.factor);
        Zone {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    };
    let cfg = gesture::Config {
        view: if in_game { view_zone(c, w) } else { None },
        hover,
        ghost: ghost.map(to_zone),
        ..gesture_config(&settings, w)
    };
    let options = chrome::Options {
        button_mm: settings.button_mm,
        swap: settings.swap_bars,
        idle_worker: extras.idle_worker(),
    };
    let tap_px = TAP_MM * ppmm * w.factor / lb.scale;
    let hide_chrome = chrome_hidden(mode, extras.pad().hide_touch, w.screen.0);
    // The Pause button shows play while the game is paused.
    c.touch.chrome.paused = in_game && c.sim.as_ref().is_some_and(|s| s.world.paused);
    let Core {
        touch: t,
        input,
        script,
        ..
    } = c;
    t.active = true;
    follow_text_boxes(t, wanted);
    let ms = elapsed.as_secs_f32() * 1000.0;

    t.chrome
        .layout(chrome::frame_of(lb.screen(), w.safe, w.factor, ppmm, in_game, options));
    // The keys the chrome pressed last step come up first.
    let mut keys: Vec<(Key, bool)> = std::mem::take(&mut t.queued);
    // The one-shot latches come up a frame after the click they were for; a locked one stays held.
    if std::mem::take(&mut t.release_mods) {
        t.release_latches(&mut keys);
    }

    // The fingers: the screen's, then the mouse's if emulated; the chrome takes its own.
    let mut fingers: Vec<Touch> = raw;
    if !input.focused && script.is_none() {
        // The window lost the keyboard: whatever is held lets go.
        fingers.extend(t.emu.cancel());
        t.gesture.reset();
        // The game lets go of every key then (`Input::focus_lost`), so the latches, locked or not, show it.
        (t.chrome.shift, t.chrome.ctrl, t.chrome.alt) = (Latch::Off, Latch::Off, Latch::Off);
        t.latched_at = None;
    }
    for m in mice {
        fingers.extend(t.emu.feed(m, lb.centre(w.factor), 5.0 * ppmm));
    }
    // With the chrome hidden (`chrome_hidden`) every finger goes to the gesture machine, and with a controller the
    // shoulder buttons are the modifiers (`pad`). Latches held as it hides let go, as nothing could tap them off.
    if hide_chrome && !t.hidden {
        t.drop_latches(&mut keys);
    }
    t.hidden = hide_chrome;
    let mut passed = Vec::new();
    for f in fingers {
        match if hide_chrome { Fed::Pass } else { t.chrome.feed(f) } {
            Fed::Pass if t.text_tap(f, area) => {}
            Fed::Pass => passed.push(f),
            Fed::Consumed => {}
            Fed::Pressed(ctl) => t.press(ctl, &mut keys),
        }
    }
    if !hide_chrome {
        for ctl in t.chrome.advance(ms) {
            t.hold(ctl, &mut keys);
        }
    }
    t.clock += ms;

    // The gesture machine, one of its frames a game frame.
    t.gesture.update(elapsed, &passed, &cfg);
    if let Some((_, age)) = &mut t.pulse {
        *age += ms;
        if *age >= PULSE_MS {
            t.pulse = None;
        }
    }
    let mut frame = t.gesture.pop();
    // The smart tap: decided at a tap's press, from the game's selection, unless the plain-click latch is on (it is
    // spent on that tap either way).
    if let Some(f) = &frame
        && f.tap_press
    {
        let plain = std::mem::take(&mut t.chrome.plain);
        let at = f.pointer.map(|p| lb.screen_of(p, w.factor));
        t.tap_right = false;
        if settings.smart_tap
            && !plain
            && let Some(at) = at
        {
            let right = crate::game::touch_smart_tap(c, at, tap_px);
            c.touch.tap_right = right;
        }
    }
    let Core { touch: t, input, .. } = c;
    if t.tap_right
        && let Some(f) = &mut frame
        && (f.tap_press || f.tap)
    {
        for (b, _) in &mut f.buttons {
            if *b == Button::Left {
                *b = Button::Right;
            }
        }
        if f.tap {
            f.tap = false;
            t.tap_right = false;
        }
    }
    let mut released = false;
    match frame {
        Some(f) => {
            keys.extend(f.keys.iter().copied());
            for &(b, down) in &f.buttons {
                input.button(b, down);
                released |= !down;
            }
            if f.pointer.is_some() {
                t.pointer = f.pointer;
            } else if !t.gesture.dragging() {
                t.pointer = t.gesture.hover_pointer();
            }
            if f.tap {
                input.tap_radius = Some(tap_px);
            }
            if let Some(p) = f.pulse.or(f.armed) {
                t.pulse = Some((p, 0.0));
            }
            t.pan.0 += f.pan.0;
            t.pan.1 += f.pan.1;
            if let Some((k, at)) = f.zoom {
                t.zoom = Some((t.zoom.map_or(1.0, |z| z.0) * k, at));
            }
        }
        None => {
            if !t.gesture.dragging() {
                // While placing or targeting the pointer rests where a finger left it, so the outline stays.
                t.pointer = t.gesture.hover_pointer();
            }
        }
    }
    t.release_mods |= released;
    for (k, down) in keys {
        input.key(KeyEvent {
            key: k,
            pressed: down,
            text: None,
        });
    }

    // Outside a game the pan scrolls lists, as the wheel does; a pinch means nothing there.
    if !in_game {
        t.wheel_acc += t.pan.1 / (LINE_MM * ppmm);
        let n = t.wheel_acc.trunc();
        if n != 0.0 {
            t.wheel_acc -= n;
            input.wheel(Wheel { y: n, lines: true });
        }
        t.pan = (0.0, 0.0);
        t.zoom = None;
    }
    if t.chrome != t.drawn || t.hidden != t.drawn_hidden {
        t.drawn = t.chrome.clone();
        t.drawn_hidden = t.hidden;
        t.revision += 1;
    }
}

/// The touch step in a mode without fingers (`controls::Mode::fingers`): the frame's fingers are dropped before the
/// chrome and the gesture machine see them, and whatever touch held lets go (the left button, the latched keys, the
/// chrome's queued keys), so a mode change in the middle of a press leaves nothing stuck. Touch that was in use comes
/// back with the fingers ([`TouchState::parked`]). Does nothing when touch was not in use, which is all that happens
/// in Automatic, where a mode without fingers means there are none.
fn park(c: &mut Core) {
    c.input.touches.clear();
    c.input.mouse_touches.clear();
    let Core { touch: t, input, .. } = c;
    if !t.active {
        return;
    }
    let mut keys: Vec<(Key, bool)> = std::mem::take(&mut t.queued);
    t.drop_latches(&mut keys);
    t.emu.cancel();
    t.gesture.reset();
    while let Some(f) = t.gesture.pop() {
        keys.extend(f.keys.iter().copied());
        for &(b, down) in &f.buttons {
            input.button(b, down);
        }
    }
    // A fresh machine: the fingers that were down are not coming back to be lifted.
    t.gesture = Gesture::default();
    t.latched_at = None;
    t.pointer = None;
    t.pulse = None;
    t.pan = (0.0, 0.0);
    t.zoom = None;
    t.tap_right = false;
    t.release_mods = false;
    t.parked = true;
    t.active = false;
    for (k, down) in keys {
        input.key(KeyEvent {
            key: k,
            pressed: down,
            text: None,
        });
    }
    t.revision += 1;
}

/// The map view's place in the window, in points, as the last frame drew it.
fn view_zone(c: &Core, w: WindowGeom) -> Option<Zone> {
    let (r, _, _) = c.view.as_ref()?.drawn()?;
    let lb = w.letterbox();
    let (x0, y0) = lb.window_of((r.x as f32, r.y as f32), w.factor);
    let (x1, y1) = lb.window_of(((r.x + r.w as i32) as f32, (r.y + r.h as i32) as f32), w.factor);
    Some(Zone {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Draws the long-press pulse (a ring that grows and fades) onto the composed picture; the same ring marks a finger
/// held long enough to draw the selection box.
pub fn draw_pulse(c: &mut Core) {
    let Some((at, age)) = c.touch.pulse else { return };
    let Some(w) = c.input.window else { return };
    let (cx, cy) = w.letterbox().screen_of(at, w.factor);
    let t = (age / PULSE_MS).clamp(0.0, 1.0);
    let radius = 4.0 + 26.0 * t;
    let strength = (1.0 - t) * 0.85;
    let (width, height) = (c.frame.width as i32, c.frame.height as i32);
    let reach = (radius + 2.0).ceil() as i32;
    for y in (cy as i32 - reach).max(0)..=(cy as i32 + reach).min(height - 1) {
        for x in (cx as i32 - reach).max(0)..=(cx as i32 + reach).min(width - 1) {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let edge = 1.0 - (d - radius).abs() / 1.5;
            if edge > 0.0 {
                let a = (edge * strength).clamp(0.0, 1.0);
                let i = (y as usize * width as usize + x as usize) * 4;
                for (ch, ink) in c.frame.rgba[i..i + 3].iter_mut().zip([255.0, 240.0, 160.0]) {
                    *ch = (*ch as f32 + (ink - *ch as f32) * a).round() as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The placing and targeting prompts fit the status bar's text in the install's own font (`TIDEWATER_INSTALL`;
    /// skipped without one).
    #[test]
    fn the_prompts_fit_the_status_bar_in_the_real_font() {
        let Some(root) = std::env::var_os("TIDEWATER_INSTALL") else {
            eprintln!("skipped: set TIDEWATER_INSTALL to run");
            return;
        };
        let install = tidewater_formats::install::Install::open(&root).expect("the install opens");
        let front = crate::frontend::Front::load(install).expect("the fonts load");
        let room = 341 - crate::messages::STATUS_TEXT_X;
        for p in [PLACE_PROMPT, TARGET_PROMPT] {
            let w = front.small.width(p.as_bytes()) as i32;
            assert!(w <= room, "{p:?} is {w} wide, the bar has {room}");
        }
    }

    /// The `widescreen` option applies on touch too (`crate::widescreen`): on a phone the width is the option's, and the
    /// side bars give way to a wider screen (`chrome_hidden`).
    #[test]
    fn widescreen_applies_on_touch_and_takes_the_bars_place() {
        use crate::controls::Mode;
        use crate::extras::Widescreen;
        let phone = (1336, 752);
        for (mode, want) in [
            (Widescreen::Off, 640),
            (Widescreen::Ratio16x10, 768),
            (Widescreen::Ratio16x9, 864),
            (Widescreen::Auto, 864),
        ] {
            let w = crate::widescreen::width(mode, Some(phone));
            assert_eq!(w, want, "{mode:?}");
            assert_eq!(chrome_hidden(Mode::Touch, true, w), w > 640, "{mode:?}");
        }
        // A 4:3 tablet with `auto`: no wider, so the bars stay.
        let w = crate::widescreen::width(Widescreen::Auto, Some((2048, 1536)));
        assert_eq!(w, 640);
        assert!(!chrome_hidden(Mode::Touch, true, w));
    }

    #[test]
    fn without_touch_the_pointer_is_left_alone() {
        let mut t = TouchState::new(false);
        let mut p = Some((10.0, 20.0));
        t.place_pointer(&mut p, false);
        assert_eq!(p, Some((10.0, 20.0)));
        assert!(!t.active);
    }

    #[test]
    fn with_touch_the_pointer_is_the_fingers_and_nowhere_when_none_is_down() {
        let mut t = TouchState::new(true);
        let mut p = Some((10.0, 20.0));
        t.place_pointer(&mut p, false);
        assert_eq!(p, None, "an emulated finger up leaves no cursor behind");
        t.pointer = Some((5.0, 6.0));
        t.place_pointer(&mut p, false);
        assert_eq!(p, Some((5.0, 6.0)));
    }

    #[test]
    fn a_mouse_that_moves_with_no_finger_down_takes_the_pointer_back() {
        let mut t = TouchState::new(false);
        t.active = true;
        let mut p = Some((1.0, 1.0));
        t.place_pointer(&mut p, false);
        assert!(t.active, "the first look only learns where the mouse is");
        let mut p = Some((9.0, 9.0));
        t.place_pointer(&mut p, false);
        assert!(!t.active);
        assert_eq!(p, Some((9.0, 9.0)));
        // A scripted run's pointer never counts as a mouse.
        t.active = true;
        let mut p = Some((3.0, 3.0));
        t.place_pointer(&mut p, true);
        assert!(t.active);
    }

    #[test]
    fn the_latches_toggle_their_keys_and_the_toggles_flip() {
        let mut t = TouchState::new(true);
        let mut keys = Vec::new();
        t.press(Control::Shift, &mut keys);
        t.press(Control::Alt, &mut keys);
        t.press(Control::Shift, &mut keys);
        assert_eq!(
            keys,
            [(Key::ShiftLeft, true), (Key::AltLeft, true), (Key::ShiftLeft, false)]
        );
        assert!(t.chrome.alt.held() && !t.chrome.shift.held());
        t.press(Control::Plain, &mut keys);
        t.press(Control::Keyboard, &mut keys);
        t.press(Control::Pause, &mut keys);
        assert!(t.chrome.plain && t.chrome.keyboard);
        assert!(t.take_pause() && !t.take_pause());
    }

    #[test]
    fn a_double_tapped_latch_stays_held_across_clicks_until_tapped_again() {
        let mut t = TouchState::new(true);
        let mut keys = Vec::new();
        // Two quick taps: Shift goes down once and locks.
        t.press(Control::Shift, &mut keys);
        t.clock += 200.0;
        t.press(Control::Shift, &mut keys);
        assert_eq!(t.chrome.shift, Latch::Locked);
        assert_eq!(keys, [(Key::ShiftLeft, true)]);
        // Unit after unit: each click's release leaves it held, while a one-shot Ctrl beside it lets go.
        t.clock += 1000.0;
        t.press(Control::Ctrl, &mut keys);
        keys.clear();
        for _ in 0..3 {
            t.release_latches(&mut keys);
            t.clock += 500.0;
        }
        assert_eq!(keys, [(Key::ControlLeft, false)]);
        assert_eq!((t.chrome.shift, t.chrome.ctrl), (Latch::Locked, Latch::Off));
        // One tap unlocks it and lets the key go.
        keys.clear();
        t.press(Control::Shift, &mut keys);
        assert_eq!(t.chrome.shift, Latch::Off);
        assert_eq!(keys, [(Key::ShiftLeft, false)]);
        // Two slow taps are on, then off: no lock.
        keys.clear();
        t.press(Control::Alt, &mut keys);
        t.clock += chrome::LOCK_MS + 50.0;
        t.press(Control::Alt, &mut keys);
        assert_eq!(t.chrome.alt, Latch::Off);
        assert_eq!(keys, [(Key::AltLeft, true), (Key::AltLeft, false)]);
        // A click between the taps spends the latch: the next tap is a fresh one-shot, not a lock.
        keys.clear();
        t.press(Control::Shift, &mut keys);
        t.release_latches(&mut keys);
        t.clock += 100.0;
        t.press(Control::Shift, &mut keys);
        assert_eq!(t.chrome.shift, Latch::Once);
    }

    #[test]
    fn a_text_box_opens_the_keyboard_and_closes_only_what_it_opened() {
        let mut t = TouchState::new(false);
        let mut keys = Vec::new();
        // The box opens it and, once done, closes it.
        follow_text_boxes(&mut t, true);
        assert!(t.chrome.keyboard);
        follow_text_boxes(&mut t, true);
        assert!(t.chrome.keyboard, "only the change acts");
        follow_text_boxes(&mut t, false);
        assert!(!t.chrome.keyboard);
        // Opened by its button first: the box leaves it open after.
        t.press(Control::Keyboard, &mut keys);
        follow_text_boxes(&mut t, true);
        follow_text_boxes(&mut t, false);
        assert!(t.chrome.keyboard, "the player opened it");
        t.press(Control::Keyboard, &mut keys);
        // Hidden by the system while the box wants typing: it stays hidden until the next box.
        follow_text_boxes(&mut t, true);
        t.keyboard_hidden();
        follow_text_boxes(&mut t, true);
        assert!(!t.chrome.keyboard);
        follow_text_boxes(&mut t, false);
        follow_text_boxes(&mut t, true);
        assert!(t.chrome.keyboard, "the next box opens it again");
    }

    #[test]
    fn a_tap_outside_the_box_closes_the_keyboard_and_one_on_it_opens_it() {
        let mut t = TouchState::new(false);
        let area = Some((100.0, 200.0, 50.0, 10.0));
        let touch = |id, phase, pos| Touch { id, phase, pos };
        follow_text_boxes(&mut t, true);
        assert!(t.chrome.keyboard);
        // On the box: it goes on to the box.
        assert!(!t.text_tap(touch(1, TouchPhase::Began, (120.0, 205.0)), area));
        assert!(t.chrome.keyboard);
        // Elsewhere: the keyboard closes and the whole touch is spent.
        assert!(t.text_tap(touch(2, TouchPhase::Began, (10.0, 10.0)), area));
        assert!(!t.chrome.keyboard);
        assert!(t.text_tap(touch(2, TouchPhase::Moved, (12.0, 10.0)), area));
        assert!(t.text_tap(touch(2, TouchPhase::Ended, (12.0, 10.0)), area));
        assert!(t.spent.is_empty());
        // With it closed, a tap elsewhere acts as ever, and one on the box opens it.
        assert!(!t.text_tap(touch(3, TouchPhase::Began, (10.0, 10.0)), area));
        assert!(!t.chrome.keyboard);
        assert!(!t.text_tap(touch(4, TouchPhase::Began, (149.0, 209.0)), area));
        assert!(t.chrome.keyboard);
        // No box: fingers are left alone.
        assert!(!t.text_tap(touch(5, TouchPhase::Began, (10.0, 10.0)), None));
        assert!(t.chrome.keyboard);
    }

    #[test]
    fn hiding_the_bars_lets_go_a_locked_latch() {
        let mut t = TouchState::new(false);
        let mut keys = Vec::new();
        t.press(Control::Ctrl, &mut keys);
        t.press(Control::Ctrl, &mut keys);
        assert_eq!(t.chrome.ctrl, Latch::Locked);
        keys.clear();
        t.drop_latches(&mut keys);
        assert_eq!(t.chrome.ctrl, Latch::Off);
        assert_eq!(keys, [(Key::ControlLeft, false)]);
    }

    #[test]
    fn the_keyboard_button_follows_the_system_hiding_the_keyboard() {
        let mut t = TouchState::new(true);
        let mut keys = Vec::new();
        t.press(Control::Keyboard, &mut keys);
        assert!(t.chrome.keyboard && t.chrome.is_on(Control::Keyboard));
        t.keyboard_hidden();
        assert!(!t.chrome.keyboard && !t.chrome.is_on(Control::Keyboard));
        // The next tap opens it again, and no key went to the game either way.
        t.press(Control::Keyboard, &mut keys);
        assert!(t.chrome.keyboard);
        assert!(keys.is_empty());
        // Hidden while already closed: nothing changes.
        let mut closed = TouchState::new(true);
        closed.keyboard_hidden();
        assert_eq!(closed.chrome, TouchState::new(true).chrome);
    }

    #[test]
    fn the_key_buttons_press_now_and_let_go_at_the_next_step() {
        let mut t = TouchState::new(true);
        let mut keys = Vec::new();
        t.press(Control::Group(3), &mut keys);
        t.press(Control::Menu, &mut keys);
        t.press(Control::IdleWorker, &mut keys);
        assert_eq!(keys, [(Key::Digit(3), true), (Key::F(10), true), (Key::F(6), true)]);
        assert_eq!(
            t.queued,
            [(Key::Digit(3), false), (Key::F(10), false), (Key::F(6), false)]
        );
        // A held group button sets the group: Ctrl and the digit, both up a step later.
        let (mut keys, mut t) = (Vec::new(), TouchState::new(true));
        t.hold(Control::Group(2), &mut keys);
        assert_eq!(keys, [(Key::ControlLeft, true), (Key::Digit(2), true)]);
        assert_eq!(t.queued, [(Key::Digit(2), false), (Key::ControlLeft, false)]);
        // With Ctrl latched already, the latch keeps it.
        let (mut keys, mut t) = (Vec::new(), TouchState::new(true));
        t.chrome.ctrl = Latch::Once;
        t.hold(Control::Group(2), &mut keys);
        assert_eq!(keys, [(Key::Digit(2), true)]);
        assert_eq!(t.queued, [(Key::Digit(2), false)]);
    }

    #[test]
    fn a_phone_is_a_screen_under_nine_centimetres_across() {
        let at = |size: (u32, u32), factor: f32| WindowGeom {
            ppi: 160.0 * factor,
            ..WindowGeom::new(size, factor)
        };
        // A phone in portrait and landscape: 393 points is 62 mm at 160 points to the inch.
        assert!(is_phone(at((1179, 2556), 3.0)));
        assert!(is_phone(at((2556, 1179), 3.0)));
        // A tablet: 820 points is 130 mm.
        assert!(!is_phone(at((1640, 2360), 2.0)));
    }

    #[test]
    fn the_touch_options_set_the_gesture_machine() {
        let phone = WindowGeom {
            ppi: 480.0,
            ..WindowGeom::new((1179, 2556), 3.0)
        };
        let tablet = WindowGeom {
            ppi: 320.0,
            ..WindowGeom::new((1640, 2360), 2.0)
        };
        let s = crate::extras::ClientExtras::default().touch();
        let (p, t) = (gesture_config(&s, phone), gesture_config(&s, tablet));
        assert!(p.pinch && !t.pinch, "the pinch is on for phones only by default");
        assert_eq!(p.one_finger, OneFinger::Pan);
        assert!(p.glide && p.menu_tap);
        assert_eq!(p.two_finger, TwoFinger::Pan);
        assert_eq!((p.arm_ms, p.long_press_ms), (300.0, 450.0));
        let s = TouchSettings {
            drag_pans: false,
            two_finger: TouchTwoFinger::Corners,
            pinch: Some(true),
            ..s
        };
        let t = gesture_config(&s, tablet);
        assert!(t.pinch);
        assert_eq!(t.one_finger, OneFinger::Box);
        assert_eq!(t.two_finger, TwoFinger::Corners);
    }
}
