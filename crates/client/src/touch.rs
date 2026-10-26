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
//! - With a controller connected (option `pad-hide-touch`), no chrome: it neither shows nor takes fingers.
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
use crate::input::{Button, Key, KeyEvent, Touch, Wheel, WindowGeom};
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
    /// The chrome is hidden for a controller (`pad::PadState::hides_touch`).
    hidden: bool,
    /// The chrome as the shell last drew it, and a count of its changes.
    drawn: Chrome,
    drawn_hidden: bool,
    pub revision: u64,
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

    /// Whether touch is the main input right now, so the pillarbox holds the chrome and the `widescreen` option is
    /// ignored (`crate::widescreen::width`): on a touch-only platform, with `--touch-emulation`, and while touch is in
    /// use on a computer with a touch screen.
    pub fn blocks_widescreen(&self) -> bool {
        self.touch_only || self.emulate || self.active
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
            Control::Keyboard => self.chrome.keyboard = !self.chrome.keyboard,
            Control::Menu => self.stroke(Key::F(10), keys),
            Control::IdleWorker => self.stroke(Key::F(crate::idle::KEY), keys),
            Control::Group(n) => self.stroke(Key::Digit(n), keys),
        }
    }

    /// After a click: the one-shot latches let go; a locked one stays held.
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

/// One frame's touch step; see the module doc.
pub fn step(c: &mut Core, elapsed: Duration, window: Option<WindowGeom>) {
    c.input.tap_radius = None;
    let fresh = !c.input.touches.is_empty() || !c.input.mouse_touches.is_empty();
    if !(c.touch.active || fresh) {
        return;
    }
    let in_game = game_running(c);
    let raw = std::mem::take(&mut c.input.touches);
    let mice = std::mem::take(&mut c.input.mouse_touches);
    let Some(w) = window else { return };
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
    let hide_chrome = c.pad.hides_touch(&extras.pad());
    // The Pause button shows play while the game is paused.
    c.touch.chrome.paused = in_game && c.sim.as_ref().is_some_and(|s| s.world.paused);
    let Core {
        touch: t,
        input,
        script,
        ..
    } = c;
    t.active = true;
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
    // With a controller connected the chrome may hide (option `pad-hide-touch`): every finger goes to the gesture
    // machine, and the shoulder buttons are the modifiers (`pad`).
    t.hidden = hide_chrome;
    let mut passed = Vec::new();
    for f in fingers {
        match if hide_chrome { Fed::Pass } else { t.chrome.feed(f) } {
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

    /// The `widescreen` option is ignored where touch is the main input (`crate::widescreen`): the pillarbox holds the
    /// chrome, so the screen stays 640 wide.
    #[test]
    fn touch_overrides_the_widescreen_option() {
        use crate::extras::Widescreen;
        let width = |t: &TouchState, mode| crate::widescreen::width(mode, Some((1920, 1080)), t.blocks_widescreen());
        // A mouse and keyboard: the option stands.
        let desktop = TouchState::new(false);
        assert!(!desktop.blocks_widescreen());
        assert_eq!(width(&desktop, Widescreen::Ratio16x9), 864);
        assert_eq!(width(&desktop, Widescreen::Auto), 864);
        // A phone or tablet.
        let mut phone = TouchState::new(false);
        phone.touch_only = true;
        phone.active = true;
        for mode in [Widescreen::Ratio16x10, Widescreen::Ratio16x9, Widescreen::Auto] {
            assert_eq!(width(&phone, mode), 640, "{mode:?}");
        }
        // `--touch-emulation` forces it off, even before a finger has touched.
        let emulated = TouchState::new(true);
        assert!(emulated.blocks_widescreen());
        assert_eq!(width(&emulated, Widescreen::Ratio16x9), 640);
        // A computer with a touch screen: off while touch is in use, back once the mouse is used again.
        let mut hybrid = TouchState::new(false);
        hybrid.active = true;
        assert_eq!(width(&hybrid, Widescreen::Ratio16x9), 640);
        hybrid.active = false;
        assert_eq!(width(&hybrid, Widescreen::Ratio16x9), 864);
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
