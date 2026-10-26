//! A frame's input from the player, as the core reads it (`docs/design/client-core.md`, "Input model", migration
//! step 4): the keys and mouse buttons held and changed this frame, the typed key events and the wheel. The shell
//! (or a scripted run) puts the system's events in with [`Input::key`], [`Input::button`] and [`Input::wheel`]; the
//! core reads them back. Nothing here depends on an engine.
//!
//! The rules are the ones the client had from Bevy's input, so that it behaves as before:
//! - [`Buttons`]: a key or button pressed this frame is "just pressed" for the frame's systems, then only "pressed";
//!   [`Input::begin_frame`] applies the frame's events in the order they came. A system may take a key away from the
//!   others ([`Buttons::reset`]), as the chat line takes the typing keys.
//! - [`Log`]: the typed key events and wheel steps stay readable for two updates of the log
//!   ([`Input::update_logs`]), and each reader keeps its own place ([`LogCursor`]), so a reader that starts late still
//!   sees what came in just before.
//! - The pointer and the window: the shell sets [`Input::pointer`] once a frame, after the mouse lock, with the
//!   window's size and scale ([`Input::window`]) that turn it into screen and map positions.
//! - Touch: the shell (or a scripted run, or the touch emulation) puts the fingers' events in with [`Input::touch`];
//!   the core's gesture machine (`gesture`) takes them in the frame's input step and turns them into the mouse and
//!   keyboard input above, so the game's rules only ever see the mouse model (`docs/design/touch.md`).
//! - Gamepads: the shell (or a scripted run) puts the pads' events in with [`Input::pad`]; the core's pad step
//!   (`pad`) turns them into a pointer, mouse buttons and keys in the same way (`docs/design/gamepad.md`).

use std::collections::HashSet;
use std::path::PathBuf;

/// A key on the keyboard, by its place (the US layout's name for it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Letter(char),
    Digit(u8),
    Numpad(u8),
    NumpadAdd,
    NumpadSubtract,
    NumpadEnter,
    F(u8),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Escape,
    Backspace,
    Tab,
    Space,
    Minus,
    Equal,
    PrintScreen,
    CapsLock,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    SuperLeft,
    SuperRight,
    /// Any other key, by a number the shell gives it.
    Other(u32),
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Right,
    Middle,
    Other(u16),
}

/// Keys or buttons: held, and pressed or released this frame.
#[derive(Clone, Debug)]
pub struct Buttons<T> {
    pressed: HashSet<T>,
    just_pressed: Vec<T>,
    just_released: Vec<T>,
}

impl<T> Default for Buttons<T> {
    fn default() -> Self {
        Buttons {
            pressed: HashSet::new(),
            just_pressed: Vec::new(),
            just_released: Vec::new(),
        }
    }
}

impl<T: Copy + Eq + std::hash::Hash> Buttons<T> {
    /// A press: just pressed unless it was held already.
    pub fn press(&mut self, b: T) {
        if self.pressed.insert(b) && !self.just_pressed.contains(&b) {
            self.just_pressed.push(b);
        }
    }

    /// A release: just released if it was held.
    pub fn release(&mut self, b: T) {
        if self.pressed.remove(&b) && !self.just_released.contains(&b) {
            self.just_released.push(b);
        }
    }

    /// Releases everything held (the window lost the keyboard).
    pub fn release_all(&mut self) {
        let held: Vec<T> = self.pressed.iter().copied().collect();
        for b in held {
            self.release(b);
        }
    }

    pub fn pressed(&self, b: T) -> bool {
        self.pressed.contains(&b)
    }

    pub fn just_pressed(&self, b: T) -> bool {
        self.just_pressed.contains(&b)
    }

    pub fn just_released(&self, b: T) -> bool {
        self.just_released.contains(&b)
    }

    pub fn any_pressed(&self, bs: impl IntoIterator<Item = T>) -> bool {
        bs.into_iter().any(|b| self.pressed(b))
    }

    pub fn any_just_pressed(&self, bs: impl IntoIterator<Item = T>) -> bool {
        bs.into_iter().any(|b| self.just_pressed(b))
    }

    /// The keys held, in no particular order.
    pub fn get_pressed(&self) -> impl Iterator<Item = &T> {
        self.pressed.iter()
    }

    /// The keys pressed this frame, in the order they were pressed.
    pub fn get_just_pressed(&self) -> impl Iterator<Item = &T> {
        self.just_pressed.iter()
    }

    /// Takes a key away: no longer held, pressed or released for the systems after this one.
    pub fn reset(&mut self, b: T) {
        self.pressed.remove(&b);
        self.just_pressed.retain(|&x| x != b);
        self.just_released.retain(|&x| x != b);
    }

    /// The start of a frame: nothing is just pressed or released yet.
    pub fn clear(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
    }
}

/// A key going down or up, with the text it types.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyEvent {
    pub key: Key,
    pub pressed: bool,
    pub text: Option<String>,
}

/// A step of the mouse wheel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel {
    /// Positive: away from the user.
    pub y: f32,
    /// In lines (a mouse's notches) rather than pixels (a touchpad's).
    pub lines: bool,
}

/// Events kept for two updates, each with its number.
#[derive(Clone, Debug)]
pub struct Log<T> {
    older: Vec<(usize, T)>,
    newer: Vec<(usize, T)>,
    count: usize,
}

impl<T> Default for Log<T> {
    fn default() -> Self {
        Log {
            older: Vec::new(),
            newer: Vec::new(),
            count: 0,
        }
    }
}

/// A reader's place in a [`Log`]: the number of the first event it hasn't read.
#[derive(Clone, Copy, Debug, Default)]
pub struct LogCursor(usize);

impl<T> Log<T> {
    pub fn push(&mut self, e: T) {
        self.newer.push((self.count, e));
        self.count += 1;
    }

    /// The oldest events go; the newer ones become the older.
    pub fn update(&mut self) {
        std::mem::swap(&mut self.older, &mut self.newer);
        self.newer.clear();
    }

    /// Every event goes (their numbers aren't used again).
    pub fn clear(&mut self) {
        self.older.clear();
        self.newer.clear();
    }

    /// The events `cursor` hasn't read yet, oldest first; it has read them all afterwards.
    pub fn read<'a>(&'a self, cursor: &mut LogCursor) -> impl Iterator<Item = &'a T> + 'a {
        let from = cursor.0;
        cursor.0 = self.count;
        self.older
            .iter()
            .chain(&self.newer)
            .filter(move |(n, _)| *n >= from)
            .map(|(_, e)| e)
    }
}

/// A file or folder that came in from the system, for the first-run screen (`firstrun`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileEvent {
    /// Dropped on the window.
    Dropped(PathBuf),
    /// The answer of a system dialog the screen asked for: the chosen path, `None` if it was cancelled.
    Picked(Option<PathBuf>),
}

/// Physical pixels per inch of one logical point's worth of screen when the shell knows nothing better: 140 points
/// to the inch, between a laptop's and a phone's (a phone's point is about 160 to the inch).
pub const DEFAULT_POINT_PPI: f32 = 140.0;

/// The window the screen shows in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowGeom {
    /// Its size in physical pixels.
    pub size: (u32, u32),
    /// Physical pixels per logical point.
    pub factor: f32,
    /// The part of the window the system leaves free (not under a notch, rounded corner or home bar), in physical
    /// pixels: left, top, width, height. The whole window where there is no such thing.
    pub safe: crate::display::Area,
    /// Physical pixels per inch (the shell's best knowledge; it turns millimetres into pixels for touch).
    pub ppi: f32,
    /// How the screen is fitted to the safe area: by a whole number, a fraction of it, or whichever fills better
    /// (`display`, "Fractional scale").
    pub fit: crate::display::Fit,
    /// The screen's size in screen pixels: 640 × 480, or the wider one of a widescreen game. The shell reports the
    /// size of the last frame it showed and the core sets the current one (`Core::advance`) before anything reads it.
    pub screen: (u32, u32),
}

impl WindowGeom {
    /// A window of `size` physical pixels at `factor` physical pixels per point: all of it safe, whole-number scale,
    /// the default pixel density.
    pub fn new(size: (u32, u32), factor: f32) -> Self {
        WindowGeom {
            size,
            factor,
            safe: (0, 0, size.0, size.1),
            ppi: DEFAULT_POINT_PPI * factor,
            fit: crate::display::Fit::Whole,
            screen: crate::display::SCREEN,
        }
    }

    /// Where the screen (640 × 480, or the wider one of a widescreen game) sits in this window.
    pub fn letterbox(&self) -> crate::display::Letterbox {
        crate::display::Letterbox::fit_with(self.size, self.safe, self.fit, self.screen)
    }

    /// Logical points to a millimetre.
    pub fn points_per_mm(&self) -> f32 {
        self.ppi / self.factor.max(0.01) / 25.4
    }
}

/// What a finger did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    Began,
    Moved,
    Ended,
    /// The system took the finger away (a palm, a system gesture): not a lift.
    Cancelled,
}

/// One finger's event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Touch {
    /// Names the finger from its `Began` to its `Ended` or `Cancelled`.
    pub id: u64,
    pub phase: TouchPhase,
    /// In the window's logical points, as [`Input::pointer`] is.
    pub pos: (f32, f32),
}

/// What the left mouse button did, for the touch emulation (`touchemu`): the shell puts these in instead of the
/// button when emulating.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseTouch {
    pub kind: MouseTouchKind,
    /// In the window's logical points.
    pub pos: (f32, f32),
    pub alt: bool,
    pub shift: bool,
    pub ctrl: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseTouchKind {
    Down,
    Move,
    Up,
}

/// A gamepad button, by its place on the pad (SDL's names): the face buttons are south, east, west and north whatever
/// their letters, so a Nintendo-layout pad's A (east) is not the confirm button unless the player swaps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadButton {
    South,
    East,
    West,
    North,
    /// Back, Select, View or Share: the small button left of the middle.
    Back,
    Guide,
    /// Start, Menu or Options: the small button right of the middle.
    Start,
    LeftStick,
    RightStick,
    LeftShoulder,
    RightShoulder,
    Up,
    Down,
    Left,
    Right,
    /// Any other button (paddles, touchpad click).
    Other,
}

/// A gamepad axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadAxis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    LeftTrigger,
    RightTrigger,
}

/// What a gamepad did. `pad` names the pad from its `Added` to its `Removed`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PadEvent {
    Added(u32),
    Removed(u32),
    Button {
        pad: u32,
        button: PadButton,
        down: bool,
    },
    /// A stick from -1 to 1 (positive right and down) or a trigger from 0 to 1.
    Axis {
        pad: u32,
        axis: PadAxis,
        value: f32,
    },
}

/// The player's input.
#[derive(Clone, Debug, Default)]
pub struct Input {
    pub keys: Buttons<Key>,
    pub mouse: Buttons<Button>,
    pub typed: Log<KeyEvent>,
    pub wheel: Log<Wheel>,
    /// The wheel this frame, summed.
    pub scroll: f32,
    /// Whether the last of this frame's wheel steps was in pixels.
    pub scroll_pixels: bool,
    /// The pointer in the window's logical points (the locked pointer while the mouse lock holds the system's still),
    /// `None` when it is outside the window.
    pub pointer: Option<(f32, f32)>,
    /// The window, `None` without one.
    pub window: Option<WindowGeom>,
    /// The window has the keyboard focus (a scripted run counts as focused).
    pub focused: bool,
    /// Files dropped on the window and answers of system dialogs since the last frame that read them
    /// (`firstrun`); the reader takes them.
    pub files: Vec<FileEvent>,
    /// The fingers' events since the last frame, oldest first; the core's gesture step takes them (`gesture`).
    pub touches: Vec<Touch>,
    /// This frame's left release ends a touchscreen tap: the radius of its tolerance in screen pixels (`mouse::tap_snap`).
    pub tap_radius: Option<f32>,
    /// The mouse acting as a finger (`--touch-emulation`): the events since the last frame.
    pub mouse_touches: Vec<MouseTouch>,
    /// The gamepads' events since the last frame, oldest first; the core's pad step takes them (`pad`).
    pub pads: Vec<PadEvent>,
    /// A pad's d-pad up or down in a menu this frame, as a focus step (Shift+Tab or Tab), set by the pad step. A
    /// keyboard never sets it: its arrow keys keep their own meaning (`frontend::menu_input`, the game menu).
    pub(crate) pad_focus: Option<crate::menuscreen::FocusKey>,
    /// A pad's confirm button in a menu this frame, set by the pad step beside the Enter it sends: it also presses the
    /// focused item that Enter leaves alone (a main-menu link, a game dialog's check box or radio). Never a keyboard's.
    pub(crate) pad_confirm: bool,
    /// This frame's events, applied to the keys and buttons by [`Input::begin_frame`].
    pending: Vec<Event>,
}

#[derive(Clone, Debug)]
enum Event {
    Key(Key, bool),
    Button(Button, bool),
    Wheel(Wheel),
    FocusLost,
}

impl Input {
    /// A key went down or up.
    pub fn key(&mut self, e: KeyEvent) {
        self.pending.push(Event::Key(e.key, e.pressed));
        self.typed.push(e);
    }

    /// A mouse button went down or up.
    pub fn button(&mut self, b: Button, pressed: bool) {
        self.pending.push(Event::Button(b, pressed));
    }

    /// The wheel turned.
    pub fn wheel(&mut self, w: Wheel) {
        self.pending.push(Event::Wheel(w));
        self.wheel.push(w);
    }

    /// A finger went down, moved, lifted or was taken away.
    pub fn touch(&mut self, t: Touch) {
        self.touches.push(t);
    }

    /// The mouse's left button or motion, as a finger of the touch emulation.
    pub fn mouse_touch(&mut self, t: MouseTouch) {
        self.mouse_touches.push(t);
    }

    /// A gamepad came, went, or moved a button or an axis.
    pub fn pad(&mut self, e: PadEvent) {
        self.pads.push(e);
    }

    /// A file or folder was dropped on the window.
    pub fn dropped(&mut self, path: PathBuf) {
        self.files.push(FileEvent::Dropped(path));
    }

    /// A system dialog answered: the path chosen, `None` if the user cancelled.
    pub fn picked(&mut self, path: Option<PathBuf>) {
        self.files.push(FileEvent::Picked(path));
    }

    /// The window lost the keyboard: every key held is released.
    pub fn focus_lost(&mut self) {
        self.pending.push(Event::FocusLost);
    }

    /// Drops every event not yet applied or read: a scripted run's input replaces the system's.
    pub fn clear_events(&mut self) {
        self.pending.clear();
        self.touches.clear();
        self.mouse_touches.clear();
        self.pads.clear();
        self.typed.clear();
        self.wheel.clear();
        self.files.clear();
    }

    /// The typed events and wheel steps age by one update (see the module doc).
    pub fn update_logs(&mut self) {
        self.typed.update();
        self.wheel.update();
    }

    /// The start of a frame's systems: the events since the last frame are applied in order.
    pub fn begin_frame(&mut self) {
        self.keys.clear();
        self.mouse.clear();
        self.scroll = 0.0;
        self.scroll_pixels = false;
        let mut lost = false;
        for e in std::mem::take(&mut self.pending) {
            match e {
                Event::Key(k, true) => self.keys.press(k),
                Event::Key(k, false) => self.keys.release(k),
                Event::Button(b, true) => self.mouse.press(b),
                Event::Button(b, false) => self.mouse.release(b),
                Event::Wheel(w) => {
                    self.scroll += w.y;
                    self.scroll_pixels = !w.lines;
                }
                Event::FocusLost => lost = true,
            }
        }
        // After the frame's key events, as they may have come in before the window lost the keyboard.
        if lost {
            self.keys.release_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            text: None,
        }
    }

    fn up(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: false,
            text: None,
        }
    }

    #[test]
    fn a_key_is_just_pressed_for_one_frame() {
        let mut i = Input::default();
        i.key(down(Key::Enter));
        assert!(!i.keys.pressed(Key::Enter), "not before the frame starts");
        i.begin_frame();
        assert!(i.keys.just_pressed(Key::Enter) && i.keys.pressed(Key::Enter));
        i.begin_frame();
        assert!(!i.keys.just_pressed(Key::Enter) && i.keys.pressed(Key::Enter));
        i.key(up(Key::Enter));
        i.begin_frame();
        assert!(i.keys.just_released(Key::Enter) && !i.keys.pressed(Key::Enter));
    }

    #[test]
    fn a_tap_within_one_frame_is_pressed_and_released() {
        let mut i = Input::default();
        i.key(down(Key::Letter('A')));
        i.key(up(Key::Letter('A')));
        i.begin_frame();
        assert!(i.keys.just_pressed(Key::Letter('A')));
        assert!(i.keys.just_released(Key::Letter('A')));
        assert!(!i.keys.pressed(Key::Letter('A')));
    }

    #[test]
    fn keys_pressed_together_come_in_order() {
        let mut i = Input::default();
        for c in ['C', 'A', 'B'] {
            i.key(down(Key::Letter(c)));
        }
        i.begin_frame();
        let got: Vec<Key> = i.keys.get_just_pressed().copied().collect();
        assert_eq!(got, [Key::Letter('C'), Key::Letter('A'), Key::Letter('B')]);
    }

    #[test]
    fn a_reset_key_is_gone_for_the_rest_of_the_frame() {
        let mut i = Input::default();
        i.key(down(Key::Enter));
        i.begin_frame();
        i.keys.reset(Key::Enter);
        assert!(!i.keys.pressed(Key::Enter) && !i.keys.just_pressed(Key::Enter));
        i.key(up(Key::Enter));
        i.begin_frame();
        assert!(!i.keys.just_released(Key::Enter), "a key no longer held isn't released");
    }

    #[test]
    fn losing_focus_releases_every_key() {
        let mut i = Input::default();
        i.key(down(Key::ShiftLeft));
        i.begin_frame();
        i.focus_lost();
        i.key(down(Key::Space));
        i.begin_frame();
        assert!(!i.keys.pressed(Key::ShiftLeft) && i.keys.just_released(Key::ShiftLeft));
        assert!(!i.keys.pressed(Key::Space), "a key pressed in the same frame goes too");
    }

    #[test]
    fn the_logs_keep_events_for_two_updates() {
        let mut log = Log::default();
        let (mut early, mut late) = (LogCursor::default(), LogCursor::default());
        log.push(1);
        assert_eq!(log.read(&mut early).copied().collect::<Vec<_>>(), [1]);
        log.update();
        log.push(2);
        assert_eq!(log.read(&mut early).copied().collect::<Vec<_>>(), [2]);
        // A reader that starts now still sees both.
        assert_eq!(log.read(&mut late).copied().collect::<Vec<_>>(), [1, 2]);
        log.update();
        log.update();
        log.push(3);
        assert_eq!(log.read(&mut LogCursor::default()).copied().collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn the_wheel_is_summed_per_frame() {
        let mut i = Input::default();
        i.wheel(Wheel { y: 1.0, lines: true });
        i.wheel(Wheel { y: 2.0, lines: true });
        i.begin_frame();
        assert_eq!((i.scroll, i.scroll_pixels), (3.0, false));
        i.wheel(Wheel { y: 5.0, lines: false });
        i.begin_frame();
        assert_eq!((i.scroll, i.scroll_pixels), (5.0, true));
        i.begin_frame();
        assert_eq!((i.scroll, i.scroll_pixels), (0.0, false));
    }
}
