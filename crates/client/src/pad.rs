//! The core's pad step (`docs/design/gamepad.md`): a gamepad turned into the pointer, mouse buttons and keys the rest
//! of the core already reads, as the touch step (`touch`) does for fingers. It runs in the frame's input step, before
//! the touch step and before the frame's events are applied, and does nothing at all until a pad's event comes in, so
//! without a pad every frame is exactly what it was.
//!
//! **Control modes.** Option `controls` (`crate::controls`) can leave the pad out (the Touch and Keyboard and mouse
//! modes): its connections are still noted, but its buttons and sticks are dropped and whatever it held lets go, as
//! when it is unplugged. It can also leave fingers out (the Controller mode), so a finger does not take the pointer
//! from the pad then. Automatic leaves out only a device that is not there.
//!
//! **The pointer.** While the pad has it ([`PadState::active`]), the pointer is the pad's: the left stick moves it in
//! screen pixels a second, with a dead zone, an optional curve and friction over units and command buttons. A pad
//! button or a push of a stick takes it, starting where the pointer last was (so a finger or the mouse puts it, and
//! the stick moves it on); a mouse that moves, or a finger that touches, takes it back ([`PadState::place_pointer`]).
//! The shell moves the system's pointer to the pad's ([`PadState::take_warp`]), so the two never jump apart.
//!
//! **The buttons** (by place, `input::PadButton`; the confirm and back buttons swap with option `pad-swap`):
//!
//! | Pad | In a game | Elsewhere |
//! |---|---|---|
//! | South (confirm) | left button; held, a box | Enter, the focused item |
//! | East (back) | right button; in panel mode, back to the map | Esc |
//! | West | panel mode: the pointer on the command buttons, the d-pad stepping between them | - |
//! | North | the pointer to the minimap and back | - |
//! | L1 / R1 | Shift / Ctrl, held | the same |
//! | L2 / R2 | the pointer twice as fast / Alt, held | the same |
//! | D-pad | control groups 1-4 (with R1 set, with R2 centre and recall) | arrow keys; up and down also step the focus |
//! | Right stick | scrolls the map (the arrow keys) | the mouse wheel |
//! | Start | F10, the game menu (Esc while a menu is open) | Enter |
//! | Back / Select | pause | Esc |
//! | Left stick press | the system keyboard, for chat and save names | the same |
//! | Right stick press | F6, the next idle worker (option `idle-worker`) | - |
//!
//! A button's meaning is fixed when it goes down, so its release always lets go of what its press held.

use std::collections::HashMap;
use std::time::Duration;

use crate::core::{Core, game_running};
use crate::extras::PadSettings;
use crate::game::PadAim;
use crate::gesture::Pt;
use crate::hud::Rect;
use crate::input::{Button, Input, Key, KeyEvent, PadAxis, PadButton, PadEvent, Wheel, WindowGeom};
use crate::menuscreen::FocusKey;

/// The pointer's speed at a full push of the stick, in screen pixels a second.
pub const SPEED: f32 = 640.0;
/// The left trigger held multiplies it by this.
pub const FAST: f32 = 2.0;
/// Over a unit or a command button the pointer moves at this share of its speed (with `pad-acceleration`).
pub const FRICTION: f32 = 0.45;
/// A trigger pushed past this counts as held.
pub const TRIGGER: f32 = 0.5;
/// The right stick scrolls past this push and stops below the second (so it doesn't flicker at the edge).
pub const SCROLL_ON: f32 = 0.5;
pub const SCROLL_OFF: f32 = 0.35;
/// Mouse-wheel lines a second at a full push of the right stick, outside a game.
pub const LINES_PER_SEC: f32 = 10.0;
/// A system pointer more than this many window points from where the pad left it has been moved by a mouse.
pub const TAKEOVER_PT: f32 = 1.5;

/// What a pad button does, as decided when it went down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Out {
    Key(Key),
    Mouse(Button),
    Panel,
    PanelBack,
    PanelStep(i32, i32),
    Minimap,
    Pause,
    Keyboard,
    Nothing,
}

/// The core's pad state.
#[derive(Default)]
pub struct PadState {
    /// The pads connected now.
    pub connected: Vec<u32>,
    /// A pad has been connected in this run: the Settings screen shows its Controller page.
    pub seen: bool,
    /// The pad has the pointer.
    pub active: bool,
    /// The pad's pointer, in window points.
    pointer: Option<Pt>,
    /// The pointer as the pad left it last frame, to tell a mouse that moved from the pad's own warp.
    left: Option<Pt>,
    /// The pointer the platform reported last frame.
    last_platform: Option<Option<Pt>>,
    /// The last place any pointer was, where the pad starts.
    last_seen: Option<Pt>,
    /// Where the shell should move the system's pointer to.
    warp: Option<Pt>,
    /// What each button held does.
    bound: HashMap<PadButton, Out>,
    /// The sticks and triggers, by [`axis_index`].
    axes: [f32; 6],
    /// The keys the triggers and the right stick hold.
    level_keys: Vec<Key>,
    /// Panel mode: the command button the pointer is on.
    panel: Option<usize>,
    /// Where the pointer was on the map before it jumped to the panel or the minimap.
    map_spot: Option<Pt>,
    /// Where it last was on the minimap.
    minimap_spot: Option<Pt>,
    wheel_acc: f32,
    pause: bool,
    keyboard: bool,
}

fn axis_index(a: PadAxis) -> usize {
    match a {
        PadAxis::LeftX => 0,
        PadAxis::LeftY => 1,
        PadAxis::RightX => 2,
        PadAxis::RightY => 3,
        PadAxis::LeftTrigger => 4,
        PadAxis::RightTrigger => 5,
    }
}

/// A stick's push past the dead zone, from 0 to 1, and its direction.
pub fn stick(x: f32, y: f32, dead_zone: f32) -> (f32, Pt) {
    let m = (x * x + y * y).sqrt();
    if m <= dead_zone || m == 0.0 {
        return (0.0, (0.0, 0.0));
    }
    (((m - dead_zone) / (1.0 - dead_zone)).min(1.0), (x / m, y / m))
}

/// The pointer's speed in screen pixels a second for a push `n` (0 to 1).
pub fn speed(n: f32, s: &PadSettings, fast: bool, over: bool) -> f32 {
    let curve = if s.acceleration { n * n } else { n };
    let mut v = SPEED * s.speed * curve;
    if fast {
        v *= FAST;
    }
    if over && s.acceleration {
        v *= FRICTION;
    }
    v
}

fn centre(r: Rect) -> Pt {
    (r.x as f32 + r.w as f32 / 2.0, r.y as f32 + r.h as f32 / 2.0)
}

/// The shown command button one step from `from` in direction `d` on the 3 × 3 panel: the nearest in that row or
/// column, past empty ones; `from` itself if there is none.
pub fn panel_step(slots: &[Option<Rect>; 9], from: usize, d: (i32, i32)) -> usize {
    let (mut col, mut row) = ((from % 3) as i32, (from / 3) as i32);
    loop {
        col += d.0;
        row += d.1;
        if !(0..3).contains(&col) || !(0..3).contains(&row) {
            return from;
        }
        let n = (row * 3 + col) as usize;
        if slots[n].is_some() {
            return n;
        }
    }
}

/// The context a button's press is read in.
#[derive(Clone, Copy)]
struct Ctx {
    in_game: bool,
    menu_open: bool,
    panel: bool,
    swap: bool,
}

fn bind(b: PadButton, x: Ctx) -> Out {
    let game = x.in_game && !x.menu_open;
    let (confirm, back) = if x.swap {
        (PadButton::East, PadButton::South)
    } else {
        (PadButton::South, PadButton::East)
    };
    let digit_or_arrow = |n: u8, arrow: Key, d: (i32, i32)| {
        if x.panel {
            Out::PanelStep(d.0, d.1)
        } else if game {
            Out::Key(Key::Digit(n))
        } else {
            Out::Key(arrow)
        }
    };
    match b {
        // In the menus confirm presses the focused item and back is Esc, rather than clicks at the pointer.
        _ if b == confirm && !game => Out::Key(Key::Enter),
        _ if b == back && !game => Out::Key(Key::Escape),
        _ if b == confirm => Out::Mouse(Button::Left),
        _ if b == back => {
            if x.panel {
                Out::PanelBack
            } else {
                Out::Mouse(Button::Right)
            }
        }
        PadButton::West if game => Out::Panel,
        PadButton::North if game => Out::Minimap,
        PadButton::LeftShoulder => Out::Key(Key::ShiftLeft),
        PadButton::RightShoulder => Out::Key(Key::ControlLeft),
        PadButton::Up => digit_or_arrow(1, Key::Up, (0, -1)),
        PadButton::Right => digit_or_arrow(2, Key::Right, (1, 0)),
        PadButton::Down => digit_or_arrow(3, Key::Down, (0, 1)),
        PadButton::Left => digit_or_arrow(4, Key::Left, (-1, 0)),
        PadButton::Start if game => Out::Key(Key::F(10)),
        PadButton::Start if x.menu_open => Out::Key(Key::Escape),
        PadButton::Start => Out::Key(Key::Enter),
        PadButton::Back if game => Out::Pause,
        PadButton::Back => Out::Key(Key::Escape),
        PadButton::LeftStick => Out::Keyboard,
        PadButton::RightStick if game => Out::Key(Key::F(crate::idle::KEY)),
        _ => Out::Nothing,
    }
}

fn key(input: &mut Input, k: Key, pressed: bool) {
    input.key(KeyEvent {
        key: k,
        pressed,
        text: None,
    });
}

impl PadState {
    /// The pad's pointer, while it has it.
    pub fn pointer(&self) -> Option<Pt> {
        self.active.then_some(self.pointer).flatten()
    }

    /// Where the shell should move the system's pointer, if the pad moved it since the last call.
    pub fn take_warp(&mut self) -> Option<Pt> {
        self.warp.take()
    }

    /// Whether the pause button was pressed since the last call.
    pub fn take_pause(&mut self) -> bool {
        std::mem::take(&mut self.pause)
    }

    /// Whether a pause is waiting for the game.
    pub fn pause_pending(&self) -> bool {
        self.pause
    }

    /// Whether the keyboard button was pressed since the last call.
    pub fn take_keyboard(&mut self) -> bool {
        std::mem::take(&mut self.keyboard)
    }

    /// Whether panel mode is on (the pointer on the command buttons).
    pub fn in_panel(&self) -> bool {
        self.panel.is_some()
    }

    /// The pointer after the platform's and the touch step's (called once the frame's pointer is set): while the pad
    /// has it, it is the pad's. A pointer the platform moved away from where the pad left it is a mouse's, which takes
    /// it back.
    pub fn place_pointer(&mut self, pointer: &mut Option<Pt>) {
        let platform = *pointer;
        let last = self.last_platform.replace(platform);
        if self.active {
            let moved = last.is_some_and(|l| l != platform);
            let away = match (platform, self.left) {
                (Some(p), Some(l)) => (p.0 - l.0).abs() > TAKEOVER_PT || (p.1 - l.1).abs() > TAKEOVER_PT,
                (Some(_), None) => true,
                (None, _) => false,
            };
            if moved && away {
                self.active = false;
                self.panel = None;
            } else {
                *pointer = self.pointer;
            }
        }
        if let Some(p) = *pointer {
            self.last_seen = Some(p);
        }
        self.left = self.pointer();
    }

    /// Lets go of every key and button the pad holds.
    fn let_go(&mut self, input: &mut Input) {
        for (_, out) in self.bound.drain() {
            match out {
                Out::Key(k) => key(input, k, false),
                Out::Mouse(b) => input.button(b, false),
                _ => {}
            }
        }
        for k in self.level_keys.drain(..) {
            key(input, k, false);
        }
        self.axes = [0.0; 6];
        self.panel = None;
    }

    /// Takes the pointer, starting where it last was (the window's middle if nowhere).
    fn wake(&mut self, w: WindowGeom) {
        if !self.active {
            self.active = true;
            let lb = w.letterbox();
            self.pointer = Some(lb.keep_on_screen(self.last_seen.unwrap_or_else(|| lb.centre(w.factor)), w.factor));
        }
    }
}

/// One frame's pad step; see the module doc.
pub fn step(c: &mut Core, elapsed: Duration, window: Option<WindowGeom>) {
    c.input.pad_focus = None;
    c.input.pad_confirm = false;
    let events = std::mem::take(&mut c.input.pads);
    let fingers = !c.input.touches.is_empty() || !c.input.mouse_touches.is_empty();
    if events.is_empty() && c.pad.bound.is_empty() && c.pad.level_keys.is_empty() && !c.pad.active {
        return;
    }
    let Some(w) = window else { return };
    let lb = w.letterbox();
    let settings = crate::game::extras(c).pad();
    let in_game = game_running(c);
    let menu_open = c.menu.is_some();
    let scripted = c.script.is_some();
    let focused = c.input.focused || scripted;
    let dt = elapsed.as_secs_f32();

    // The pads' events: connections, the buttons pressed and let go in order, the axes' last values.
    let mut presses: Vec<(PadButton, bool)> = Vec::new();
    {
        let p = &mut c.pad;
        for e in events {
            match e {
                PadEvent::Added(id) => {
                    if !p.connected.contains(&id) {
                        p.connected.push(id);
                    }
                    p.seen = true;
                }
                PadEvent::Removed(id) => {
                    p.connected.retain(|&x| x != id);
                    if p.connected.is_empty() {
                        presses.clear();
                        p.let_go(&mut c.input);
                        p.active = false;
                    }
                }
                PadEvent::Button { button, down, .. } => presses.push((button, down)),
                PadEvent::Axis { axis, value, .. } => p.axes[axis_index(axis)] = value,
            }
        }
        if !focused {
            // The window lost the keyboard: whatever the pad holds lets go, as the keys do.
            presses.clear();
            p.let_go(&mut c.input);
        }
    }
    // The control mode (`controls`): a pad it leaves out is ignored from here, and what it held lets go.
    let mode = crate::controls::mode(c);
    let fingers = fingers && mode.fingers();
    if !mode.pad() {
        presses.clear();
        c.pad.let_go(&mut c.input);
        c.pad.active = false;
    }
    let (push, dir) = stick(c.pad.axes[0], c.pad.axes[1], settings.dead_zone);
    let (rpush, _) = stick(c.pad.axes[2], c.pad.axes[3], settings.dead_zone);
    let triggered = c.pad.axes[4] > TRIGGER || c.pad.axes[5] > TRIGGER;
    if presses.iter().any(|&(_, down)| down) || push > 0.0 || rpush > 0.0 || triggered {
        if fingers {
            // A finger on the screen has the pointer this frame.
        } else {
            c.pad.wake(w);
        }
    }
    if fingers && c.pad.active {
        c.pad.active = false;
        c.pad.panel = None;
    }

    // The game screen as the pad aims at it.
    let at = c.pad.pointer.map(|p| lb.screen_of(p, w.factor));
    let aim: PadAim = if in_game {
        crate::game::pad_aim(c, at, c.pad.active && push > 0.0 && settings.acceleration)
    } else {
        PadAim::default()
    };

    let Core { pad: p, input, .. } = c;
    let to_window = |s: Pt| lb.window_of(s, w.factor);

    // Panel mode ends when there is no panel to be on, and while aiming (the pointer goes back to the map to aim).
    let exit_panel = |p: &mut PadState| {
        if p.panel.take().is_some()
            && let Some(spot) = p.map_spot.take()
        {
            p.pointer = Some(spot);
        }
    };
    if p.panel.is_some() && (!in_game || menu_open || aim.aiming || aim.slots.iter().all(Option::is_none)) {
        exit_panel(p);
    }

    // The buttons, in order.
    for (b, down) in presses {
        if !down {
            match p.bound.remove(&b) {
                Some(Out::Key(k)) => key(input, k, false),
                Some(Out::Mouse(m)) => input.button(m, false),
                _ => {}
            }
            continue;
        }
        if p.bound.contains_key(&b) {
            continue;
        }
        let out = bind(
            b,
            Ctx {
                in_game,
                menu_open,
                panel: p.panel.is_some(),
                swap: settings.swap,
            },
        );
        p.bound.insert(b, out);
        match out {
            Out::Key(k) => {
                key(input, k, true);
                // In the menus the d-pad's up and down also move the focus ring, which the menus move only with Tab.
                match k {
                    Key::Up => input.pad_focus = Some(FocusKey::Back),
                    Key::Down => input.pad_focus = Some(FocusKey::Next),
                    Key::Enter => input.pad_confirm = true,
                    _ => {}
                }
            }
            Out::Mouse(m) => input.button(m, true),
            Out::Panel => {
                if p.panel.is_some() {
                    exit_panel(p);
                } else if let Some(first) = aim.slots.iter().position(Option::is_some) {
                    p.map_spot = p.pointer;
                    p.panel = Some(first);
                }
            }
            Out::PanelBack => exit_panel(p),
            Out::PanelStep(dx, dy) => {
                if let Some(n) = p.panel {
                    p.panel = Some(panel_step(&aim.slots, n, (dx, dy)));
                }
            }
            Out::Minimap => {
                if let Some(mm) = aim.minimap {
                    p.panel = None;
                    let on_minimap = at.is_some_and(|a| mm.contains(a.0, a.1));
                    if on_minimap {
                        p.minimap_spot = p.pointer;
                        p.pointer = p.map_spot.take().or_else(|| aim.viewport.map(|v| to_window(centre(v))));
                    } else {
                        p.map_spot = p.pointer;
                        p.pointer = p.minimap_spot.or_else(|| Some(to_window(centre(mm))));
                    }
                }
            }
            Out::Pause => p.pause = true,
            Out::Keyboard => p.keyboard = true,
            Out::Nothing => {}
        }
    }

    // In panel mode the pointer sits on its button; a button that went (the panel changed) gives way to the first.
    if let Some(n) = p.panel {
        let n = if aim.slots[n].is_some() {
            n
        } else {
            aim.slots.iter().position(Option::is_some).unwrap_or(n)
        };
        p.panel = Some(n);
        if push > 0.0 {
            // The stick frees the pointer from the panel.
            p.panel = None;
        } else if let Some(r) = aim.slots[n] {
            p.pointer = Some(to_window(centre(r)));
        }
    }

    // The left stick moves the pointer.
    if p.active
        && p.panel.is_none()
        && push > 0.0
        && let Some(ptr) = p.pointer
    {
        let v = speed(push, &settings, p.axes[4] > TRIGGER, aim.over) * dt;
        let per_point = lb.scale / w.factor;
        let d = (dir.0 * v * per_point, dir.1 * v * per_point);
        p.pointer = Some(lb.keep_on_screen((ptr.0 + d.0, ptr.1 + d.1), w.factor));
    }
    // The system's pointer follows the pad's, moved or jumped (the panel, the minimap).
    if p.active && p.pointer != p.left {
        p.warp = p.pointer;
    }

    // The triggers and the right stick hold keys while pushed; outside a game the right stick is the wheel.
    let mut want: Vec<Key> = Vec::new();
    if p.axes[5] > TRIGGER {
        want.push(Key::AltLeft);
    }
    let (rx, ry) = (p.axes[2], p.axes[3]);
    if in_game && !menu_open {
        for (value, neg, pos) in [(rx, Key::Left, Key::Right), (ry, Key::Up, Key::Down)] {
            for (k, v) in [(neg, -value), (pos, value)] {
                let held = p.level_keys.contains(&k);
                if v > SCROLL_ON || (held && v > SCROLL_OFF) {
                    want.push(k);
                }
            }
        }
    } else if rpush > 0.0 {
        let (_, rdir) = stick(rx, ry, settings.dead_zone);
        p.wheel_acc += -rdir.1 * rpush * LINES_PER_SEC * dt;
        let n = p.wheel_acc.trunc();
        if n != 0.0 {
            p.wheel_acc -= n;
            input.wheel(Wheel { y: n, lines: true });
        }
    } else {
        p.wheel_acc = 0.0;
    }
    for k in std::mem::take(&mut p.level_keys) {
        if !want.contains(&k) {
            key(input, k, false);
        }
    }
    for k in want {
        if !p.level_keys.contains(&k) {
            key(input, k, true);
        }
        p.level_keys.push(k);
    }

    if std::mem::take(&mut p.keyboard) {
        c.touch.chrome.keyboard = !c.touch.chrome.keyboard;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> PadSettings {
        crate::extras::ClientExtras::default().pad()
    }

    fn ctx(in_game: bool) -> Ctx {
        Ctx {
            in_game,
            menu_open: false,
            panel: false,
            swap: false,
        }
    }

    #[test]
    fn the_stick_has_a_dead_zone_and_reaches_full_push() {
        assert_eq!(stick(0.1, 0.0, 0.15).0, 0.0);
        assert_eq!(stick(0.0, 0.0, 0.15).0, 0.0);
        let (n, d) = stick(1.0, 0.0, 0.15);
        assert_eq!((n, d), (1.0, (1.0, 0.0)));
        // Past the edge (a square stick's corner) it stays full.
        assert_eq!(stick(1.0, 1.0, 0.15).0, 1.0);
        let (half, _) = stick(0.575, 0.0, 0.15);
        assert!((half - 0.5).abs() < 1e-3, "{half}");
    }

    #[test]
    fn acceleration_curves_the_push_and_adds_friction() {
        let s = settings();
        assert_eq!(speed(1.0, &s, false, false), SPEED);
        assert_eq!(speed(0.5, &s, false, false), SPEED * 0.25);
        assert_eq!(speed(1.0, &s, true, false), SPEED * FAST);
        assert_eq!(speed(1.0, &s, false, true), SPEED * FRICTION);
        let even = PadSettings {
            acceleration: false,
            ..s
        };
        assert_eq!(speed(0.5, &even, false, false), SPEED * 0.5);
        assert_eq!(
            speed(1.0, &even, false, true),
            SPEED,
            "no friction without acceleration"
        );
    }

    #[test]
    fn the_buttons_mean_what_the_table_says() {
        let g = ctx(true);
        assert_eq!(bind(PadButton::South, g), Out::Mouse(Button::Left));
        assert_eq!(bind(PadButton::East, g), Out::Mouse(Button::Right));
        assert_eq!(bind(PadButton::West, g), Out::Panel);
        assert_eq!(bind(PadButton::North, g), Out::Minimap);
        assert_eq!(bind(PadButton::LeftShoulder, g), Out::Key(Key::ShiftLeft));
        assert_eq!(bind(PadButton::RightShoulder, g), Out::Key(Key::ControlLeft));
        assert_eq!(bind(PadButton::Up, g), Out::Key(Key::Digit(1)));
        assert_eq!(bind(PadButton::Left, g), Out::Key(Key::Digit(4)));
        assert_eq!(bind(PadButton::Start, g), Out::Key(Key::F(10)));
        assert_eq!(bind(PadButton::Back, g), Out::Pause);
        assert_eq!(bind(PadButton::RightStick, g), Out::Key(Key::F(6)));
        // Outside a game: arrows, Enter and Esc; confirm and back press the focused item and go back; no panel or
        // minimap.
        let m = ctx(false);
        assert_eq!(bind(PadButton::South, m), Out::Key(Key::Enter));
        assert_eq!(bind(PadButton::East, m), Out::Key(Key::Escape));
        assert_eq!(bind(PadButton::Up, m), Out::Key(Key::Up));
        assert_eq!(bind(PadButton::Start, m), Out::Key(Key::Enter));
        assert_eq!(bind(PadButton::Back, m), Out::Key(Key::Escape));
        assert_eq!(bind(PadButton::West, m), Out::Nothing);
        // The game menu open: Start closes it.
        let menu = Ctx { menu_open: true, ..g };
        assert_eq!(bind(PadButton::Start, menu), Out::Key(Key::Escape));
        assert_eq!(bind(PadButton::South, menu), Out::Key(Key::Enter));
        assert_eq!(bind(PadButton::East, menu), Out::Key(Key::Escape));
        let swapped_menu = Ctx { swap: true, ..m };
        assert_eq!(bind(PadButton::East, swapped_menu), Out::Key(Key::Enter));
        assert_eq!(bind(PadButton::South, swapped_menu), Out::Key(Key::Escape));
        // Panel mode: the d-pad steps and back leaves it.
        let panel = Ctx { panel: true, ..g };
        assert_eq!(bind(PadButton::Right, panel), Out::PanelStep(1, 0));
        assert_eq!(bind(PadButton::East, panel), Out::PanelBack);
        // Swapped for a Nintendo-layout pad.
        let swap = Ctx { swap: true, ..g };
        assert_eq!(bind(PadButton::East, swap), Out::Mouse(Button::Left));
        assert_eq!(bind(PadButton::South, swap), Out::Mouse(Button::Right));
    }

    #[test]
    fn panel_steps_skip_empty_buttons_and_stop_at_the_edge() {
        let r = Some(Rect::new(0, 0, 1, 1));
        // Buttons 0, 2, 4 and 8 show.
        let slots = [r, None, r, None, r, None, None, None, r];
        assert_eq!(panel_step(&slots, 0, (1, 0)), 2, "past the empty middle");
        assert_eq!(panel_step(&slots, 2, (1, 0)), 2, "the edge");
        assert_eq!(panel_step(&slots, 0, (0, 1)), 0, "nothing below in that column");
        assert_eq!(panel_step(&slots, 2, (0, 1)), 8);
        assert_eq!(panel_step(&slots, 8, (-1, 0)), 8);
        assert_eq!(panel_step(&slots, 4, (0, -1)), 4);
    }

    #[test]
    fn the_pad_keeps_the_pointer_until_a_mouse_moves_it() {
        let mut p = PadState {
            active: true,
            pointer: Some((100.0, 100.0)),
            ..Default::default()
        };
        // The platform's pointer is elsewhere and still: the pad's stands.
        let mut ptr = Some((10.0, 10.0));
        p.place_pointer(&mut ptr);
        assert_eq!(ptr, Some((100.0, 100.0)));
        let mut ptr = Some((10.0, 10.0));
        p.place_pointer(&mut ptr);
        assert_eq!(ptr, Some((100.0, 100.0)));
        // The shell moved the system's pointer to the pad's: not a mouse.
        let mut ptr = Some((100.0, 100.0));
        p.place_pointer(&mut ptr);
        assert!(p.active);
        // A mouse moved it on: the mouse has it.
        let mut ptr = Some((140.0, 90.0));
        p.place_pointer(&mut ptr);
        assert!(!p.active);
        assert_eq!(ptr, Some((140.0, 90.0)));
        assert_eq!(p.last_seen, Some((140.0, 90.0)));
    }
}
