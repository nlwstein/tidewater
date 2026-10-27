//! Which input devices drive the game (option `controls`; `docs/design/gamepad.md`, "Control modes"). SDL-free.
//!
//! The player picks a [`Mode`]; [`Devices`] says what is there now; [`Devices::resolve`] gives the mode in force. The
//! pad step (`pad`) and the touch step (`touch`) ask [`mode`] each frame and drop the events of a device the mode
//! leaves out. Mouse and keyboard events are never dropped.
//!
//! **Automatic is today's behaviour.** It picks the controller and touch together when both are there, the
//! controller alone, touch alone, or the keyboard and mouse. A device it leaves out is one that is not there, so
//! there is nothing to drop and every frame is what it was before this option existed. The touch side bars follow
//! the same rule as before: hidden while a controller is connected and `pad-hide-touch` is on.
//!
//! **Availability.** A choice whose devices are missing falls back to Automatic until they are back, so a setting
//! made on a handheld with a pad does not leave a phone without touch. The Settings screen offers only the choices
//! [`Devices::available`] allows.
//!
//! **Seen devices.** Touch counts as there on a phone or tablet, with `--touch-emulation`, and once a finger has
//! touched in this run ([`crate::touch::TouchState::seen`]); a mouse counts as there everywhere but on a phone or
//! tablet, where a real one has to have moved first ([`crate::touch::TouchState::mouse_seen`]).

use crate::core::Core;
use crate::pad::PadState;
use crate::touch::TouchState;

/// A choice of the `controls` option, in the order of the option's values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Whatever devices there are.
    #[default]
    Auto,
    /// The keyboard and mouse; a controller and fingers are ignored.
    Mouse,
    /// Fingers (and the keyboard and mouse); a controller is ignored.
    Touch,
    /// The controller (and the keyboard and mouse); fingers are ignored.
    Pad,
    /// The controller and fingers (and the keyboard and mouse).
    PadTouch,
}

impl Mode {
    /// The choice at a place in the option's list (the stored number); Automatic for any other.
    pub fn from_index(i: u8) -> Mode {
        match i {
            1 => Mode::Mouse,
            2 => Mode::Touch,
            3 => Mode::Pad,
            4 => Mode::PadTouch,
            _ => Mode::Auto,
        }
    }

    /// Whether the controller works. Only meaningful for a resolved mode ([`Devices::resolve`]).
    pub fn pad(self) -> bool {
        matches!(self, Mode::Pad | Mode::PadTouch)
    }

    /// Whether fingers (and the mouse as a finger with `--touch-emulation`) work.
    pub fn fingers(self) -> bool {
        matches!(self, Mode::Touch | Mode::PadTouch)
    }

    /// Whether the touch side bars show: always with Touch, never with the keyboard and mouse or the controller
    /// alone, and with both unless `hide_touch` (option `pad-hide-touch`) hides them.
    pub fn chrome_shown(self, hide_touch: bool) -> bool {
        match self {
            Mode::Touch => true,
            Mode::PadTouch => !hide_touch,
            Mode::Auto | Mode::Mouse | Mode::Pad => false,
        }
    }
}

/// The input devices there are now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Devices {
    /// A controller is connected.
    pub pad: bool,
    /// Touch is there: a phone or tablet, `--touch-emulation`, or a finger has been seen.
    pub touch: bool,
    /// A mouse is there: any computer, or a phone or tablet whose mouse has moved.
    pub mouse: bool,
}

impl Devices {
    /// The devices now, from the core's pad and touch state.
    pub fn of(pad: &PadState, touch: &TouchState) -> Devices {
        Devices {
            pad: !pad.connected.is_empty(),
            touch: touch.touch_only || touch.emulate || touch.seen,
            mouse: !touch.touch_only || touch.mouse_seen,
        }
    }

    /// Whether a choice's devices are all there (Automatic always is).
    pub fn available(self, choice: Mode) -> bool {
        match choice {
            Mode::Auto => true,
            Mode::Mouse => self.mouse,
            Mode::Touch => self.touch,
            Mode::Pad => self.pad,
            Mode::PadTouch => self.pad && self.touch,
        }
    }

    /// The mode in force for a choice: the choice if its devices are there, else what Automatic picks. Never
    /// [`Mode::Auto`].
    pub fn resolve(self, choice: Mode) -> Mode {
        if choice != Mode::Auto && self.available(choice) {
            return choice;
        }
        match (self.pad, self.touch) {
            (true, true) => Mode::PadTouch,
            (true, false) => Mode::Pad,
            (false, true) => Mode::Touch,
            (false, false) => Mode::Mouse,
        }
    }
}

/// The mode in force this frame.
pub fn mode(c: &Core) -> Mode {
    Devices::of(&c.pad, &c.touch).resolve(crate::game::extras(c).controls())
}

/// Notes a finger that came in this frame, before the modes are read, so the first finger of a run is never dropped
/// for want of being seen.
pub fn note_fingers(c: &mut Core) {
    if !c.input.touches.is_empty() {
        c.touch.seen = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Mode; 5] = [Mode::Auto, Mode::Mouse, Mode::Touch, Mode::Pad, Mode::PadTouch];

    fn dev(pad: bool, touch: bool, mouse: bool) -> Devices {
        Devices { pad, touch, mouse }
    }

    #[test]
    fn the_choices_follow_the_option_list() {
        let names = crate::extras::CONTROLS_MODES;
        assert_eq!(names, ["auto", "mouse", "touch", "pad", "pad-touch"]);
        for (i, m) in ALL.into_iter().enumerate() {
            assert_eq!(Mode::from_index(i as u8), m);
        }
        assert_eq!(Mode::from_index(200), Mode::Auto);
    }

    #[test]
    fn automatic_picks_the_devices_there_are() {
        assert_eq!(dev(true, true, true).resolve(Mode::Auto), Mode::PadTouch);
        assert_eq!(dev(true, false, true).resolve(Mode::Auto), Mode::Pad);
        assert_eq!(dev(false, true, true).resolve(Mode::Auto), Mode::Touch);
        assert_eq!(dev(false, false, true).resolve(Mode::Auto), Mode::Mouse);
        // A phone with no mouse seen and nothing else is still touch.
        assert_eq!(dev(false, true, false).resolve(Mode::Auto), Mode::Touch);
    }

    #[test]
    fn a_choice_whose_devices_are_there_is_kept() {
        let all = dev(true, true, true);
        for m in [Mode::Mouse, Mode::Touch, Mode::Pad, Mode::PadTouch] {
            assert_eq!(all.resolve(m), m);
        }
    }

    #[test]
    fn a_choice_whose_device_is_gone_falls_back_to_automatic() {
        // Controller chosen, controller unplugged: touch and the mouse remain.
        assert_eq!(dev(false, true, true).resolve(Mode::Pad), Mode::Touch);
        assert_eq!(dev(false, true, true).resolve(Mode::PadTouch), Mode::Touch);
        assert_eq!(dev(false, false, true).resolve(Mode::PadTouch), Mode::Mouse);
        // Touch chosen on a computer with no finger seen.
        assert_eq!(dev(true, false, true).resolve(Mode::Touch), Mode::Pad);
        // Keyboard and mouse chosen on a phone whose mouse has not been used.
        assert_eq!(dev(false, true, false).resolve(Mode::Mouse), Mode::Touch);
        // Controller and touch needs both.
        assert_eq!(dev(true, false, true).resolve(Mode::PadTouch), Mode::Pad);
        assert_eq!(dev(false, true, true).resolve(Mode::PadTouch), Mode::Touch);
        // The resolved mode is never Automatic.
        for pad in [false, true] {
            for touch in [false, true] {
                for mouse in [false, true] {
                    for m in ALL {
                        assert_ne!(dev(pad, touch, mouse).resolve(m), Mode::Auto);
                    }
                }
            }
        }
    }

    #[test]
    fn availability_follows_the_table() {
        let none = dev(false, false, false);
        assert!(none.available(Mode::Auto));
        for m in [Mode::Mouse, Mode::Touch, Mode::Pad, Mode::PadTouch] {
            assert!(!none.available(m));
        }
        assert!(dev(false, false, true).available(Mode::Mouse));
        assert!(dev(false, true, false).available(Mode::Touch));
        assert!(dev(true, false, false).available(Mode::Pad));
        assert!(!dev(true, false, false).available(Mode::PadTouch));
        assert!(!dev(false, true, false).available(Mode::PadTouch));
        assert!(dev(true, true, false).available(Mode::PadTouch));
    }

    #[test]
    fn each_mode_says_which_devices_work_and_whether_the_bars_show() {
        assert!(!Mode::Mouse.pad() && !Mode::Mouse.fingers());
        assert!(!Mode::Touch.pad() && Mode::Touch.fingers());
        assert!(Mode::Pad.pad() && !Mode::Pad.fingers());
        assert!(Mode::PadTouch.pad() && Mode::PadTouch.fingers());
        for hide in [false, true] {
            assert!(Mode::Touch.chrome_shown(hide), "touch always shows the bars");
            assert!(!Mode::Mouse.chrome_shown(hide));
            assert!(!Mode::Pad.chrome_shown(hide));
        }
        assert!(!Mode::PadTouch.chrome_shown(true));
        assert!(Mode::PadTouch.chrome_shown(false));
    }

    #[test]
    fn devices_come_from_the_pads_and_the_touch_state() {
        let mut pad = PadState::default();
        let mut touch = TouchState::new(false);
        assert_eq!(Devices::of(&pad, &touch), dev(false, false, true));
        pad.connected.push(0);
        touch.seen = true;
        assert_eq!(Devices::of(&pad, &touch), dev(true, true, true));
        // A phone: touch, and a mouse only once one has been used.
        let mut phone = TouchState::new(false);
        phone.touch_only = true;
        assert_eq!(Devices::of(&PadState::default(), &phone), dev(false, true, false));
        phone.mouse_seen = true;
        assert!(Devices::of(&PadState::default(), &phone).mouse);
        // `--touch-emulation` counts as touch before a finger.
        assert!(Devices::of(&PadState::default(), &TouchState::new(true)).touch);
    }

    #[test]
    fn the_bars_hide_for_a_pad_or_a_wide_screen() {
        let hidden = |mode: Mode, hide: bool, d: Devices, w: u32| crate::touch::chrome_hidden(d.resolve(mode), hide, w);
        let with_pad = dev(true, true, false);
        let without = dev(false, true, false);
        // A 4:3 game screen: as the mode says.
        assert!(hidden(Mode::Auto, true, with_pad, 640), "a pad hides them");
        assert!(
            !hidden(Mode::Auto, false, with_pad, 640),
            "unless pad-hide-touch is off"
        );
        assert!(!hidden(Mode::Auto, true, without, 640), "touch alone shows them");
        assert!(!hidden(Mode::Touch, true, with_pad, 640));
        assert!(hidden(Mode::Pad, true, with_pad, 640));
        // A wider screen (option `widescreen`) takes the pillarbox in every mode.
        for (mode, d) in [
            (Mode::Auto, without),
            (Mode::Touch, with_pad),
            (Mode::PadTouch, with_pad),
        ] {
            assert!(hidden(mode, false, d, 768), "{mode:?}");
            assert!(hidden(mode, false, d, 864), "{mode:?}");
        }
    }

    // The steps at the core level: a core on the first-run screen (no game files needed), driven frame by frame.

    use std::sync::Arc;
    use std::time::Duration;

    use clap::Parser;

    use crate::extras::Id;
    use crate::input::{Button, Key, KeyEvent, PadButton, PadEvent, Touch, TouchPhase, WindowGeom};

    struct NoFiles;

    impl crate::firstrun::Backend for NoFiles {
        fn search(&self) -> Vec<crate::firstrun::Candidate> {
            Vec::new()
        }

        fn import(
            &self,
            _: &std::path::Path,
            _: &mut dyn FnMut(&tidewater_import::Progress),
        ) -> Result<crate::firstrun::Candidate, String> {
            Err("no".to_string())
        }
    }

    struct Rig {
        core: Core,
        window: WindowGeom,
        _dir: tempfile::TempDir,
    }

    fn rig(choice: Mode) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let mut args = crate::args::Args::parse_from(["tidewater", "--data-dir", &dir.path().to_string_lossy()]);
        args.install = None;
        let store = crate::storage::Store(Arc::new(crate::storage::Desktop::from_data_dir(
            dir.path(),
            dir.path().join("Save"),
            dir.path().join("settings.txt"),
            dir.path().to_path_buf(),
        )));
        let mut core = crate::host::first_run_core(args, store, Arc::new(NoFiles), Some(Vec::new()), None);
        core.extras.set(Id::Controls, choice as u8);
        Rig {
            core,
            window: WindowGeom::new((640, 480), 1.0),
            _dir: dir,
        }
    }

    impl Rig {
        /// One frame's pad and touch steps, then the frame's events applied; returns whether a press is held and
        /// whether one began this frame. A press is the left button or Enter: outside a game, as here, the pad's
        /// confirm sends Enter.
        fn frame(&mut self) -> (bool, bool) {
            let c = &mut self.core;
            c.input.focused = true;
            crate::controls::note_fingers(c);
            crate::pad::step(c, Duration::from_millis(16), Some(self.window));
            crate::touch::step(c, Duration::from_millis(16), Some(self.window));
            c.input.begin_frame();
            (
                c.input.mouse.pressed(Button::Left) || c.input.keys.pressed(Key::Enter),
                c.input.mouse.just_pressed(Button::Left) || c.input.keys.just_pressed(Key::Enter),
            )
        }

        fn frames(&mut self, n: usize) -> bool {
            (0..n).fold(false, |any, _| any | self.frame().1)
        }

        fn pad(&mut self, e: PadEvent) {
            self.core.input.pad(e);
        }

        fn south(&mut self, down: bool) {
            self.pad(PadEvent::Button {
                pad: 0,
                button: PadButton::South,
                down,
            });
        }

        fn finger(&mut self, phase: TouchPhase) {
            self.core.input.touch(Touch {
                id: 1,
                phase,
                pos: (300.0, 200.0),
            });
        }

        /// A tap: the frames it takes and some after; whether the left button went down at any of them.
        fn tap(&mut self) -> bool {
            self.finger(TouchPhase::Began);
            let mut down = self.frame().1;
            down |= self.frames(2);
            self.finger(TouchPhase::Ended);
            down | self.frames(8)
        }

        fn press_south(&mut self) -> bool {
            self.pad(PadEvent::Added(0));
            self.south(true);
            let down = self.frame().1;
            self.south(false);
            self.frames(2);
            down
        }
    }

    #[test]
    fn automatic_lets_a_pad_and_a_finger_both_work_with_the_bars_hidden() {
        let mut r = rig(Mode::Auto);
        assert!(r.press_south(), "the pad presses");
        assert!(r.tap(), "the finger taps");
        assert!(r.core.touch.active);
        let size = (640, 480);
        assert!(
            r.core.touch.overlay(size, 1.0).is_none(),
            "pad-hide-touch hides the bars"
        );
    }

    #[test]
    fn automatic_without_a_pad_shows_the_bars_for_fingers() {
        let mut r = rig(Mode::Auto);
        assert!(r.tap());
        assert!(r.core.touch.overlay((640, 480), 1.0).is_some());
        assert_eq!(mode(&r.core), Mode::Touch);
    }

    #[test]
    fn in_pad_mode_a_finger_does_nothing_and_the_pad_works() {
        let mut r = rig(Mode::Pad);
        assert!(r.press_south());
        assert!(!r.tap(), "a finger is dropped");
        assert!(!r.core.touch.active);
        assert_eq!(
            mode(&r.core),
            Mode::Pad,
            "a pad is there; touch has been seen by the finger"
        );
    }

    #[test]
    fn in_touch_mode_a_pad_button_does_nothing_and_the_bars_show_with_a_pad_connected() {
        let mut r = rig(Mode::Touch);
        // A finger first, so touch is there for the option.
        assert!(r.tap());
        assert!(!r.press_south(), "the pad is ignored");
        assert!(!r.core.pad.active);
        assert!(
            r.core.touch.overlay((640, 480), 1.0).is_some(),
            "the bars show beside a pad"
        );
        assert!(r.tap(), "fingers go on working");
    }

    #[test]
    fn in_mouse_mode_neither_works_but_a_key_does() {
        let mut r = rig(Mode::Mouse);
        r.core.touch.seen = true;
        assert!(!r.press_south());
        assert!(!r.tap());
        assert!(!r.core.touch.active && !r.core.pad.active);
        r.core.input.key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            text: None,
        });
        r.frame();
        assert!(r.core.input.keys.pressed(Key::Enter));
    }

    #[test]
    fn a_mode_change_mid_press_lets_go_what_the_ignored_device_held() {
        let mut r = rig(Mode::Auto);
        r.pad(PadEvent::Added(0));
        r.south(true);
        let (held, _) = r.frame();
        assert!(held, "the pad holds its press");
        // The player picks Keyboard and mouse while it is held.
        r.core.extras.set(Id::Controls, Mode::Mouse as u8);
        r.core.touch.seen = true;
        let (held, _) = r.frame();
        assert!(!held, "the button came up");
        assert!(r.core.pad.connected.len() == 1, "the pad is still known");
        // A finger held across the change.
        let mut r = rig(Mode::Touch);
        r.finger(TouchPhase::Began);
        r.frames(3);
        assert!(r.core.touch.active);
        r.core.extras.set(Id::Controls, Mode::Mouse as u8);
        r.frame();
        assert!(!r.core.touch.active, "touch is parked");
        // Back to touch: it comes back.
        r.core.extras.set(Id::Controls, Mode::Touch as u8);
        r.frame();
        assert!(r.core.touch.active);
        assert!(r.tap());
    }
}
