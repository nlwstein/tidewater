//! Tidewater: the client core (`tidewater_client::core`) in an SDL 3 window, the program's only shell
//! (`docs/design/client-core.md`, "Platform trait"). It loads the user's installation and runs the startup sequence, a
//! map given on the command line, a `--goto` screen or a scripted scene (`script`); with no game files it shows the
//! first-run screen (`firstrun`), for which it also opens the system's folder and file dialogs and passes on the files
//! dropped on the window.
//!
//! The 640 × 480 screen is drawn at a whole number of physical pixels per screen pixel, black-bordered in a larger
//! window, and the map view shows one map pixel per screen pixel, as the original does (`display`). On a touch
//! platform, with the `scaling` extra at `sharp`, or at its default `auto` where the whole-number scale fills the safe
//! area poorly (a full screen, a resized window), it fills the safe area at any size instead, drawn sharp bilinear
//! (`display::Letterbox::fit_with`). The mouse wheel zooms only with the `--free-zoom` development option.
//! The controls are in the core (`game`).
//!
//! **Touch** (`docs/design/touch.md`). Fingers on a touch screen go in as `input::Touch` events (a trackpad's do not);
//! with `--touch-emulation` the left mouse button and motion go in as a finger instead, and the Alt, Shift and Ctrl
//! held as the button goes down choose the extra fingers. SDL's own mouse-from-touch events are turned off whenever
//! the fingers are read. The core's chrome (modifier latches, right-click toggle, pause, keyboard) is drawn over the
//! picture as one overlay texture, and its keyboard button opens the system's soft keyboard.
//!
//! **Phone** (`docs/design/ios.md`). The same loop runs inside the iOS app (`crates/ios` builds this file and `sdl.rs`
//! as a static library; `platform/ios` is the Xcode project around it). The window fills the screen inside the safe
//! area, touch is on from the start, and SDL's will-enter-background and did-enter-foreground events call
//! [`Core::suspend`] and [`Core::resume`]: after a suspend one more frame runs (the pause takes effect), then nothing
//! runs or draws until the program is back.
//!
//! **Android** (`docs/design/android.md`) is the same: `crates/android` builds this file and `sdl.rs` as `libmain.so`,
//! which SDL's Java activity loads and enters at `SDL_main`; the lifecycle is the same pair of events.
//!
//! The shell does what the design asks of one: the system's key, button, wheel and focus events into the core's input;
//! [`Core::advance`] once a frame; the composed frame into one streaming texture, drawn at the window's whole-number
//! scale with black around it (`display::Letterbox`), with the core's title and system-pointer wish; the core's mixer
//! on the sound device through an SDL audio stream (SDL converts the 22,050 Hz stereo to the device's rate); the
//! desktop's storage (`host::launch`); and the end on `Core::exit` after the settings are written. The mouse lock
//! (`display`, "Mouse lock") uses SDL's relative mode on macOS and its mouse grab elsewhere.
//!
//! A scripted run (`--script`) also records each frame (`script::record`) and, with a window, compares the window's
//! own pixels at each shot (`script::compare_shot`); `--headless` runs it without SDL at all (`host::run_headless`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use tidewater_client::args::Args;
use tidewater_client::audio::SAMPLE_RATE;
use tidewater_client::core::{Core, Platform};
use tidewater_client::display::{self, Letterbox};
use tidewater_client::extras::Scaling;
use tidewater_client::firstrun::PickKind;
use tidewater_client::host::{self, FrameStats, LockedPointer};
use tidewater_client::input::{
    Button, Buttons, Key, KeyEvent, MouseTouch, MouseTouchKind, Touch, TouchPhase, Wheel, WindowGeom,
};
use tidewater_client::script;

use crate::sdl;

/// The file dialogs' filters (the first-run screen's "Choose GOG installer..." and "Choose CD image...").
const INSTALLER_FILTERS: &[(&str, &str)] = &[("Installers", "exe;bin;sh;pkg;dmg"), ("All files", "*")];
const DISC_FILTERS: &[(&str, &str)] = &[("CD images", "iso;bin;cue;img;mdf;nrg"), ("All files", "*")];

/// The audio device's buffer asked for, in sample frames at the device's rate: about 10.7 ms at 48 kHz.
const DEVICE_FRAMES: u32 = 512;

/// The iOS app's `main` (`sdl::tidewater_ios_main`): the same program, with an error told on the console and a failing
/// exit code instead of a returned `Result`.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub fn ios_main() -> i32 {
    match run_program() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("tidewater: {e:#}");
            1
        }
    }
}

/// The Android app's `SDL_main` (`sdl::SDL_main`): the program on the command line the activity gave it, with an
/// error told in the system log and a failing exit code. The app has no command line of its own; `adb shell am start`
/// can set one (`docs/design/android.md`).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn android_main(argv: Vec<std::ffi::OsString>) -> i32 {
    match run_program_from(Args::parse_from(argv)) {
        Ok(()) => 0,
        Err(e) => {
            sdl::log_error(&format!("tidewater: {e:#}"));
            1
        }
    }
}

/// The program, from its command line: what `main` does on a desktop, and what the iOS app's entry calls.
pub fn run_program() -> Result<()> {
    run_program_from(Args::parse())
}

/// The program, from parsed arguments.
fn run_program_from(args: Args) -> Result<()> {
    if let Some(path) = &args.import {
        std::process::exit(host::import_command(path, args.data_dir.as_deref()));
    }
    let l = host::launch(args)?;
    if l.headless {
        host::run_headless(l.core);
        return Ok(());
    }
    run(l)
}

/// A phone or tablet: the touch screen is the main input, so the picture fits the safe area at any scale, and the soft
/// keyboard is closed until asked for.
const MOBILE: bool = cfg!(any(target_os = "ios", target_os = "android"));

/// Whether the mouse lock holds the system's pointer still (macOS can't confine it to the window) rather than
/// confining it.
const HOLD_STILL: bool = cfg!(target_os = "macos");

fn run(l: host::Launch) -> Result<()> {
    let host::Launch {
        mut core,
        title,
        ui_scale,
        ..
    } = l;
    let scripted = core.script.is_some();
    // A newer release is looked for in the background (`update`); on a phone "Update now" opens the release page.
    core.start_update_check();
    let sdl = sdl::Sdl::init(!scripted, DEVICE_FRAMES)?;
    let emulating = core.opts.touch_emulation;
    // A phone's screen is the main input: the fingers are read from the start, and the chrome shows.
    let touch_screen = sdl::has_touch_screen() || MOBILE;
    if emulating || touch_screen {
        sdl::own_the_fingers();
    }
    if MOBILE {
        core.touch.active = true;
        // The touch screen is the main input for good: the `widescreen` option is ignored (`widescreen::width`).
        core.touch.touch_only = true;
    }
    let points = ui_scale.unwrap_or(1);
    let (mut w, mut h) = display::window_size(points);
    // Development aid: a larger scripted window (`TIDEWATER_SCRIPT_WINDOW=1000x600`) to look at the chrome and the
    // sharp fit in; its pixels are no longer the composed frame's, so the shots are not compared.
    let big_script = scripted
        && std::env::var("TIDEWATER_SCRIPT_WINDOW")
            .ok()
            .and_then(|v| {
                v.split_once('x')
                    .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
            })
            .map(|(a, b)| (w, h) = (a, b))
            .is_some();
    // A scripted window has one pixel per point whatever the display, so the screen fills it exactly; it doesn't
    // take the keyboard (its input is the scene's) and stays on top, as the Bevy shell's does.
    let window = sdl::Window::new(
        &sdl,
        &title,
        w,
        h,
        sdl::WindowOpts {
            high_density: !scripted,
            resizable: !scripted,
            always_on_top: scripted,
            not_focusable: scripted,
            // A phone's window fills the screen; a desktop's opens full screen if the Settings screen's Window row says so.
            fullscreen: MOBILE || core.fullscreen_wanted(),
        },
    )?;
    // The app icon in the title bar and taskbar (a phone's launcher icon comes from its app package instead).
    if !MOBILE && let Some((iw, ih, rgba)) = host::window_icon() {
        window.set_icon(iw, ih, &rgba);
    }
    // A phone's window fills the screen: no sizes to pick and nowhere to centre it.
    if !scripted && !MOBILE {
        let d = window.pixel_density();
        let min = |len: u32| (len as f32 / d).ceil() as u32;
        window.set_min_size(min(display::SCREEN.0), min(display::SCREEN.1));
        // Without `--ui-scale`: the largest whole scale that fits the monitor, centred on it.
        if ui_scale.is_none()
            && let Some(((mw, mh), md)) = window.display()
        {
            let physical = ((mw as f32 * md) as u32, (mh as f32 * md) as u32);
            let (pw, ph) = display::window_size(display::monitor_scale(physical, md as f64));
            window.set_size(min(pw), min(ph));
        }
        window.centre();
    }
    let mut renderer = sdl::Renderer::new(&window, display::SCREEN, true)?;
    if !MOBILE {
        window.start_text_input();
    }
    // A scripted run mixes its sound itself (`script::record`) and plays none.
    let audio_stats = Arc::new(sdl::AudioStats::default());
    let audio = if scripted {
        None
    } else {
        let output = core.audio.output();
        match sdl::AudioOut::open(&sdl, SAMPLE_RATE, move |buf| output.mix(buf), audio_stats.clone()) {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("no sound: {e}");
                None
            }
        }
    };
    if let Some(a) = &audio
        && std::env::var_os("TIDEWATER_FRAME_STATS").is_some()
    {
        eprintln!("audio device (rate, channels, sample frames): {:?}", a.device_format());
    }

    let mut shell = Shell {
        window: &window,
        locked: LockedPointer::default(),
        grabbed: false,
        motion: (0.0, 0.0),
        // The mouse lock is the Settings screen's setting unless `--no-mouse-lock` says otherwise; the emulation's
        // dragging "finger" does without it (`Core::mouse_lock_enabled`, followed every frame).
        lock_enabled: core.mouse_lock_enabled(),
        fullscreen: MOBILE || core.fullscreen_wanted(),
        cursor_shown: true,
        emulating,
        mods: (false, false, false),
        left_down: false,
        keyboard: !MOBILE,
        fit: if MOBILE {
            display::Fit::Sharp
        } else {
            display::Fit::Whole
        },
        ppi_per_density: core.opts.touch_dpi.map_or(
            if MOBILE {
                160.0
            } else {
                tidewater_client::input::DEFAULT_POINT_PPI
            },
            |dpi| dpi,
        ),
        ppi_absolute: core.opts.touch_dpi.is_some(),
        screen: display::SCREEN,
    };
    let mut overlay_key: Option<(bool, u64, (u32, u32))> = None;
    let mut stats = FrameStats::default();
    let mut first = true;
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        let elapsed = match (scripted, first) {
            (true, _) => host::scripted_elapsed(first),
            (false, true) => Duration::ZERO,
            (false, false) => now - last,
        };
        let interval = now - last;
        last = now;
        first = false;
        shell.fit = if MOBILE {
            display::Fit::Sharp
        } else {
            match core.extras.scaling() {
                Scaling::Auto => display::Fit::Auto,
                Scaling::Integer => display::Fit::Whole,
                Scaling::Sharp => display::Fit::Sharp,
            }
        };
        let was_suspended = core.suspended;
        let quit = shell.take_events(&mut core);
        // In the background nothing runs: no frame, no drawing (the system may stop the program at any moment), only
        // the events that bring it back, or end it.
        if was_suspended && core.suspended {
            if quit {
                core.store_settings();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            last = Instant::now();
            continue;
        }
        // The Settings screen's rows take effect at once: the mouse lock for this frame's pointer, the window's full
        // screen after the frame (a phone's window is always full screen).
        shell.lock_enabled = core.mouse_lock_enabled();
        core.advance(elapsed, &mut shell);
        if !MOBILE && core.fullscreen_wanted() != shell.fullscreen {
            shell.fullscreen = core.fullscreen_wanted();
            window.set_fullscreen(shell.fullscreen);
        }
        for url in core.take_open_requests() {
            sdl::open_url(&url);
        }
        // The soft keyboard follows the chrome's button (which itself follows the system's hiding it, `take_events`);
        // a desktop keeps its text input open for the real keyboard.
        if core.touch.chrome.keyboard != shell.keyboard && (core.touch.active || MOBILE) {
            shell.keyboard = core.touch.chrome.keyboard;
            if shell.keyboard {
                window.start_text_input();
            } else if MOBILE {
                window.stop_text_input();
            }
        }
        for kind in core.take_pick_requests() {
            match kind {
                PickKind::Folder => window.show_folder_dialog(),
                PickKind::Installer => window.show_file_dialog(INSTALLER_FILTERS),
                PickKind::Disc => window.show_file_dialog(DISC_FILTERS),
            }
        }
        let exiting = std::mem::take(&mut core.exit) || quit;
        if let Some(t) = core.title.take() {
            window.set_title(&t);
        }
        if shell.cursor_shown != core.cursor_visible {
            shell.cursor_shown = core.cursor_visible;
            sdl::show_cursor(core.cursor_visible);
        }
        if exiting {
            core.store_settings();
        }
        // The frame that followed `Core::suspend` lets its pause take effect and draws nothing.
        if core.suspended && !exiting {
            continue;
        }
        // A game with `widescreen` has a wider screen: the picture's texture follows, and the window grows to hold it.
        let screen = (core.frame.width as u32, core.frame.height as u32);
        if screen != shell.screen {
            renderer.set_size(screen)?;
            if !MOBILE && !big_script {
                fit_window(&window, shell.screen, screen, shell.fit, scripted);
            }
            shell.screen = screen;
        }
        let geom = shell
            .window()
            .unwrap_or_else(|| WindowGeom::new(renderer.output_size(), 1.0));
        let lb = geom.letterbox();
        let sharp = (!lb.is_whole()).then(|| display::sharp_steps(lb.scale));
        // The chrome's overlay is rebuilt only when it, or the window, changes. A scripted window shows it only when it
        // is the big development one, so the compared shots stay the composed frames.
        let want_overlay = core.touch.active && (!scripted || big_script);
        let key = (want_overlay, core.touch.revision, geom.size);
        if overlay_key != Some(key) {
            overlay_key = Some(key);
            match want_overlay
                .then(|| core.touch.overlay(geom.size, geom.factor))
                .flatten()
            {
                Some(o) => renderer.set_overlay(Some((o.size, &o.rgba))),
                None => renderer.set_overlay(None),
            }
        }
        renderer.draw(&core.frame.rgba, lb.screen(), sharp);
        let mut finished = false;
        if let Some(run) = core.script.as_mut() {
            let shown = (big_script || lb.scale == 1.0).then_some(&core.frame);
            let asked = script::record(run, shown, &core.audio.output(), exiting);
            for name in asked.screenshots {
                let shot = renderer.read_pixels();
                let shot = shot.as_ref().map(|(w, h, px)| (*w, *h, px.as_slice()));
                script::compare_shot(run, name, shot);
            }
            finished = asked.finished;
        }
        stats.frame(interval, now.elapsed());
        renderer.present();
        if exiting || finished {
            break;
        }
    }
    if audio.is_some() && std::env::var_os("TIDEWATER_FRAME_STATS").is_some() {
        print_audio_stats(&audio_stats);
    }
    drop(audio);
    Ok(())
}

/// Makes the window hold a new screen size. A window that shows whole-number scales grows to keep its scale (the
/// screen is as high as before, so only the width changes) if the display has room; one that fits any size needs
/// nothing. The smallest size follows the screen.
fn fit_window(window: &sdl::Window, old: (u32, u32), new: (u32, u32), fit: display::Fit, scripted: bool) {
    let d = window.pixel_density();
    let points = |len: u32| (len as f32 / d).ceil() as u32;
    if !scripted {
        window.set_min_size(points(new.0), points(new.1));
    }
    let (wx, wy) = window.size_in_pixels();
    // A window that fills at any size (sharp, touch, or `auto` where the whole scale fits poorly, as a full screen
    // does) only changes its letterbox.
    if !scripted && !display::Letterbox::fit_with((wx, wy), window.safe_area(), fit, old).is_whole() {
        return;
    }
    let scale = display::fit_scale_of((wx, wy), old);
    let want = (new.0 * scale, new.1 * scale);
    if want.0 <= wx && want.1 <= wy && !scripted {
        return;
    }
    let (w, h) = if scripted {
        new
    } else {
        (want.0.max(wx), want.1.max(wy))
    };
    if !scripted
        && let Some(((mw, mh), _)) = window.display()
        && (points(w) > mw || points(h) > mh)
    {
        return;
    }
    window.set_size(points(w), points(h));
}

fn print_audio_stats(s: &sdl::AudioStats) {
    use std::sync::atomic::Ordering::Relaxed;
    let calls = s.calls.load(Relaxed);
    let span = s.last_ns.load(Relaxed).saturating_sub(s.first_ns.load(Relaxed)) as f64 / 1e6;
    eprintln!(
        "audio: {calls} callbacks, {} sample frames at 22,050 Hz, mean period {:.2} ms, longest gap {:.2} ms",
        s.frames.load(Relaxed),
        if calls > 1 { span / (calls - 1) as f64 } else { 0.0 },
        s.max_gap_ns.load(Relaxed) as f64 / 1e6
    );
}

/// The window as the core sees it, and the mouse lock's state.
struct Shell<'a> {
    window: &'a sdl::Window<'a>,
    locked: LockedPointer,
    /// The lock holds the system's pointer.
    grabbed: bool,
    /// The mouse's motion since the last frame, in points.
    motion: (f32, f32),
    lock_enabled: bool,
    /// The window is full screen.
    fullscreen: bool,
    cursor_shown: bool,
    /// `--touch-emulation`: the left button and motion are a finger.
    emulating: bool,
    /// Shift, Ctrl and Alt are held (for the emulation's extra fingers).
    mods: (bool, bool, bool),
    left_down: bool,
    /// The soft keyboard (or the desktop's text input) is open.
    keyboard: bool,
    /// How the picture fits the safe area (`display`).
    fit: display::Fit,
    /// Points to the inch (physical pixels per inch is this times the density), or physical pixels per inch itself when
    /// `ppi_absolute` (`--touch-dpi`).
    ppi_per_density: f32,
    ppi_absolute: bool,
    /// The screen's size in screen pixels as the last frame showed it (`widescreen`).
    screen: (u32, u32),
}

impl Shell<'_> {
    /// Hands the window's events to the core's input, as they came; the text a key typed goes with its key event.
    /// Returns whether the user asked to quit.
    fn take_events(&mut self, core: &mut Core) -> bool {
        let mut quit = false;
        let mut pending: Option<KeyEvent> = None;
        let mut keyboard_event = None;
        self.motion = (0.0, 0.0);
        let touching = core.touch.active || self.emulating;
        let (points_w, points_h) = {
            let (w, h) = self.window.size_in_pixels();
            let d = self.window.pixel_density();
            (w as f32 / d, h as f32 / d)
        };
        let input = &mut core.input;
        while let Some(e) = sdl::poll() {
            if let sdl::Event::Text(t) = &e {
                match pending.as_mut() {
                    Some(k) if k.pressed => k.text.get_or_insert_with(String::new).push_str(t),
                    // A soft keyboard types text without keys: a key of its own carries it.
                    _ if touching => {
                        input.key(KeyEvent {
                            key: Key::Other(0),
                            pressed: true,
                            text: Some(t.clone()),
                        });
                        input.key(KeyEvent {
                            key: Key::Other(0),
                            pressed: false,
                            text: None,
                        });
                    }
                    _ => {}
                }
                continue;
            }
            if let Some(k) = pending.take() {
                input.key(k);
            }
            match e {
                sdl::Event::Quit => quit = true,
                sdl::Event::FocusLost => input.focus_lost(),
                sdl::Event::Key { scancode, down } => {
                    let key = key_of(scancode);
                    match key {
                        Key::ControlLeft | Key::ControlRight => self.mods.0 = down,
                        Key::ShiftLeft | Key::ShiftRight => self.mods.1 = down,
                        Key::AltLeft | Key::AltRight => self.mods.2 = down,
                        _ => {}
                    }
                    // Alt chooses the emulation's extra fingers: the game does not see it.
                    if !(self.emulating && matches!(key, Key::AltLeft | Key::AltRight)) {
                        pending = Some(KeyEvent {
                            key,
                            pressed: down,
                            text: None,
                        })
                    }
                }
                sdl::Event::Button { button: 1, down, x, y } if self.emulating => {
                    self.left_down = down;
                    input.mouse_touch(self.mouse_touch(
                        if down { MouseTouchKind::Down } else { MouseTouchKind::Up },
                        x,
                        y,
                    ));
                }
                sdl::Event::Button { button, down, .. } => input.button(button_of(button), down),
                sdl::Event::Wheel { y } => input.wheel(Wheel { y, lines: true }),
                sdl::Event::Motion { dx, dy, x, y } => {
                    self.motion.0 += dx;
                    self.motion.1 += dy;
                    if self.emulating && self.left_down {
                        input.mouse_touch(self.mouse_touch(MouseTouchKind::Move, x, y));
                    }
                }
                sdl::Event::Finger { id, phase, x, y } => input.touch(Touch {
                    id,
                    phase: match phase {
                        0 => TouchPhase::Began,
                        1 => TouchPhase::Moved,
                        2 => TouchPhase::Ended,
                        _ => TouchPhase::Cancelled,
                    },
                    pos: (x * points_w, y * points_h),
                }),
                sdl::Event::Drop(path) => input.dropped(path),
                sdl::Event::ScreenKeyboard(shown) => keyboard_event = Some(shown),
                sdl::Event::Text(_) | sdl::Event::Other => {}
            }
        }
        if let Some(k) = pending {
            input.key(k);
        }
        // The answers of the first-run screen's dialogs.
        for answer in sdl::take_dialog_answers() {
            input.picked(answer);
        }
        // The system hid the soft keyboard itself (its own hide key, Back, a tap elsewhere): the chrome's keyboard
        // button goes dark, and the frame's follow-up closes the text input to match. SDL's own state decides, so a
        // hide followed at once by a show (the keyboard reopened mid-animation) changes nothing.
        if keyboard_event == Some(false)
            && self.keyboard
            && (core.touch.active || MOBILE)
            && !self.window.screen_keyboard_shown()
        {
            core.touch.keyboard_hidden();
        }
        // The event watch noted these as they happened (a poll may come too late).
        for suspend in sdl::take_lifecycle() {
            if suspend {
                core.suspend();
            } else {
                core.resume();
            }
        }
        quit
    }
}

impl Shell<'_> {
    fn mouse_touch(&self, kind: MouseTouchKind, x: f32, y: f32) -> MouseTouch {
        MouseTouch {
            kind,
            pos: (x, y),
            ctrl: self.mods.0,
            shift: self.mods.1,
            alt: self.mods.2,
        }
    }
}

impl Platform for Shell<'_> {
    fn window(&self) -> Option<WindowGeom> {
        let mut g = WindowGeom::new(self.window.size_in_pixels(), self.window.pixel_density());
        g.safe = self.window.safe_area();
        g.ppi = if self.ppi_absolute {
            self.ppi_per_density
        } else {
            self.ppi_per_density * g.factor
        };
        g.fit = self.fit;
        g.screen = self.screen;
        Some(g)
    }

    fn pointer(&mut self, keys: &Buttons<Key>, game_pointer: bool) -> (Option<(f32, f32)>, bool) {
        let w = self.window;
        let focused = w.focused();
        let want = display::lock_wanted(display::LockInput {
            enabled: self.lock_enabled,
            in_game: game_pointer,
            focused,
            held_free: keys.any_pressed([Key::SuperLeft, Key::SuperRight]),
        });
        if want != self.grabbed {
            self.grabbed = want;
            if HOLD_STILL {
                w.set_relative_mouse(want);
            } else {
                w.set_mouse_grab(want);
            }
        }
        if HOLD_STILL {
            let lb = self
                .window()
                .map_or_else(|| Letterbox::fit(w.size_in_pixels()), |g| g.letterbox());
            if let Some((x, y)) = self.locked.update(want, w.mouse(), self.motion, &lb, w.pixel_density()) {
                w.warp_mouse(x, y);
            }
        }
        (self.locked.0.or(w.mouse()), focused)
    }
}

/// A key by its place: SDL's scancodes are the USB HID usage numbers (keyboard page).
fn key_of(code: i32) -> Key {
    match code {
        4..=29 => Key::Letter((b'A' + (code - 4) as u8) as char),
        30..=38 => Key::Digit((code - 29) as u8),
        39 => Key::Digit(0),
        40 => Key::Enter,
        41 => Key::Escape,
        // AC Back: Android's Back button, which is Esc there.
        270 => Key::Escape,
        42 => Key::Backspace,
        43 => Key::Tab,
        44 => Key::Space,
        45 => Key::Minus,
        46 => Key::Equal,
        57 => Key::CapsLock,
        58..=69 => Key::F((code - 57) as u8),
        70 => Key::PrintScreen,
        74 => Key::Home,
        75 => Key::PageUp,
        77 => Key::End,
        78 => Key::PageDown,
        79 => Key::Right,
        80 => Key::Left,
        81 => Key::Down,
        82 => Key::Up,
        86 => Key::NumpadSubtract,
        87 => Key::NumpadAdd,
        88 => Key::NumpadEnter,
        89..=97 => Key::Numpad((code - 88) as u8),
        98 => Key::Numpad(0),
        104..=106 => Key::F((code - 91) as u8),
        224 => Key::ControlLeft,
        225 => Key::ShiftLeft,
        226 => Key::AltLeft,
        227 => Key::SuperLeft,
        228 => Key::ControlRight,
        229 => Key::ShiftRight,
        230 => Key::AltRight,
        231 => Key::SuperRight,
        0 => Key::Other(0),
        _ => host::other_key(&sdl::scancode_name(code)),
    }
}

fn button_of(b: u8) -> Button {
    match b {
        1 => Button::Left,
        2 => Button::Middle,
        3 => Button::Right,
        4 => Button::Other(0),
        5 => Button::Other(1),
        n => Button::Other(n as u16 - 4),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_by_their_usb_place() {
        assert_eq!(key_of(4), Key::Letter('A'));
        assert_eq!(key_of(29), Key::Letter('Z'));
        assert_eq!(key_of(30), Key::Digit(1));
        assert_eq!(key_of(39), Key::Digit(0));
        assert_eq!(key_of(58), Key::F(1));
        assert_eq!(key_of(67), Key::F(10));
        assert_eq!(key_of(104), Key::F(13));
        assert_eq!(key_of(89), Key::Numpad(1));
        assert_eq!(key_of(98), Key::Numpad(0));
        assert_eq!(key_of(227), Key::SuperLeft);
    }

    #[test]
    fn mouse_buttons() {
        assert_eq!(button_of(1), Button::Left);
        assert_eq!(button_of(3), Button::Right);
        assert_eq!(button_of(6), Button::Other(2));
    }
}
