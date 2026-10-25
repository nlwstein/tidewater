//! A thin safe wrapper over the parts of SDL 3 the shell uses (`sdl3-sys`, the C API as published in SDL's headers;
//! <https://wiki.libsdl.org/SDL3/>): the window, a renderer with one streaming texture, the system's events (including
//! files dropped on the window), fingers on a touch screen, the system's folder and file dialogs, the mouse modes and
//! an audio stream fed by a callback. On iOS it also holds the app's entry point, the lifecycle events and the
//! document picker (SDL has no dialogs there; the picker is a small Objective-C shim in `platform/ios`). On Android
//! it holds the entry `SDL_main` and the activity's picker callback (the picker is `TidewaterActivity.java`). Every
//! `unsafe` block of the shell is here; each owner frees what it made when it is dropped.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_int, c_void};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Result, anyhow};
use sdl3_sys::everything as sys;

/// SDL's message for the last error.
fn error(what: &str) -> anyhow::Error {
    // SAFETY: SDL_GetError returns a valid, NUL-terminated string (empty when there is no error).
    let msg = unsafe { CStr::from_ptr(sys::SDL_GetError()) };
    anyhow!("{what}: {}", msg.to_string_lossy())
}

fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).expect("no NUL bytes left")
}

/// SDL itself, from `SDL_Init` to `SDL_Quit`.
pub struct Sdl(());

impl Sdl {
    /// Starts SDL's video and events, and its audio if `audio`. `sample_frames`: the audio device's buffer asked
    /// for, in sample frames (a hint; the device may choose another).
    pub fn init(audio: bool, sample_frames: u32) -> Result<Sdl> {
        let frames = cstring(&sample_frames.to_string());
        // SAFETY: the hint's name is SDL's own constant and the value a valid C string; SDL copies both.
        unsafe { sys::SDL_SetHint(sys::SDL_HINT_AUDIO_DEVICE_SAMPLE_FRAMES, frames.as_ptr()) };
        // A phone shows the picture in landscape only (iOS says so in its Info.plist; Android's window would otherwise
        // follow the sensor into portrait, as the window is resizable).
        if cfg!(any(target_os = "ios", target_os = "android")) {
            // SAFETY: as above.
            unsafe { sys::SDL_SetHint(sys::SDL_HINT_ORIENTATIONS, c"LandscapeLeft LandscapeRight".as_ptr()) };
        }
        // Android's Back button is Esc (`docs/design/touch.md`): SDL sends it as a key and, trapped, it no longer
        // finishes the activity.
        if cfg!(target_os = "android") {
            // SAFETY: as above.
            unsafe { sys::SDL_SetHint(sys::SDL_HINT_ANDROID_TRAP_BACK_BUTTON, c"1".as_ptr()) };
        }
        let mut flags = sys::SDL_INIT_VIDEO;
        if audio {
            flags |= sys::SDL_INIT_AUDIO;
        }
        // SAFETY: called once, from the main thread, before any other SDL call but the hint.
        if unsafe { sys::SDL_Init(flags) } {
            // SAFETY: the watch is a plain function that uses no userdata (null) and lives for the whole program.
            unsafe { sys::SDL_AddEventWatch(Some(lifecycle_watch), std::ptr::null_mut()) };
            Ok(Sdl(()))
        } else {
            Err(error("SDL_Init"))
        }
    }
}

/// Going to the background (`true`, or the system ending the program) and coming back (`false`), in order. They are
/// noted by an event watch, which SDL calls at once, inside the system's own callback: the queue can be too late, as the
/// system may stop the program as soon as that callback returns.
static LIFECYCLE: Mutex<Vec<bool>> = Mutex::new(Vec::new());

unsafe extern "C" fn lifecycle_watch(_userdata: *mut c_void, event: *mut sys::SDL_Event) -> bool {
    // SAFETY: SDL passes a valid event for the length of the call.
    let t = sys::SDL_EventType(unsafe { (*event).r#type });
    let went = match t {
        sys::SDL_EVENT_WILL_ENTER_BACKGROUND | sys::SDL_EVENT_TERMINATING => Some(true),
        sys::SDL_EVENT_DID_ENTER_FOREGROUND => Some(false),
        _ => None,
    };
    if let (Some(went), Ok(mut l)) = (went, LIFECYCLE.lock()) {
        l.push(went);
    }
    true
}

/// The program's trips to the background and back since the last call: `true` for going, `false` for returning.
pub fn take_lifecycle() -> Vec<bool> {
    LIFECYCLE
        .lock()
        .map(|mut l| std::mem::take(&mut *l))
        .unwrap_or_default()
}

impl Drop for Sdl {
    fn drop(&mut self) {
        // SAFETY: every window, renderer and stream borrows the `Sdl` and is gone by now.
        unsafe { sys::SDL_Quit() };
    }
}

/// How the window opens.
#[derive(Clone, Copy, Default)]
pub struct WindowOpts {
    /// One window pixel per physical pixel (a display with two pixels per point gets twice the pixels).
    pub high_density: bool,
    pub resizable: bool,
    pub always_on_top: bool,
    /// Doesn't take the keyboard from what the user is doing.
    pub not_focusable: bool,
    /// Fills the screen (a phone's window).
    pub fullscreen: bool,
}

/// A window.
pub struct Window<'s> {
    raw: *mut sys::SDL_Window,
    _sdl: PhantomData<&'s Sdl>,
}

impl<'s> Window<'s> {
    /// A window of `w` × `h` points.
    pub fn new(_sdl: &'s Sdl, title: &str, w: u32, h: u32, o: WindowOpts) -> Result<Self> {
        let mut flags = sys::SDL_WindowFlags(0);
        for (on, f) in [
            (o.high_density, sys::SDL_WINDOW_HIGH_PIXEL_DENSITY),
            (o.resizable, sys::SDL_WINDOW_RESIZABLE),
            (o.always_on_top, sys::SDL_WINDOW_ALWAYS_ON_TOP),
            (o.not_focusable, sys::SDL_WINDOW_NOT_FOCUSABLE),
            (o.fullscreen, sys::SDL_WINDOW_FULLSCREEN),
        ] {
            if on {
                flags |= f;
            }
        }
        let title = cstring(title);
        // SAFETY: a valid title string; SDL copies it.
        let raw = unsafe { sys::SDL_CreateWindow(title.as_ptr(), w as c_int, h as c_int, flags) };
        if raw.is_null() {
            return Err(error("SDL_CreateWindow"));
        }
        Ok(Window { raw, _sdl: PhantomData })
    }

    /// The size in physical pixels.
    pub fn size_in_pixels(&self) -> (u32, u32) {
        let (mut w, mut h) = (0, 0);
        // SAFETY: a live window and two valid out pointers.
        unsafe { sys::SDL_GetWindowSizeInPixels(self.raw, &mut w, &mut h) };
        (w.max(0) as u32, h.max(0) as u32)
    }

    /// Physical pixels per point.
    pub fn pixel_density(&self) -> f32 {
        // SAFETY: a live window.
        let d = unsafe { sys::SDL_GetWindowPixelDensity(self.raw) };
        if d > 0.0 { d } else { 1.0 }
    }

    /// The window's display: its size in points and its physical pixels per point.
    pub fn display(&self) -> Option<((u32, u32), f32)> {
        // SAFETY: a live window; the mode SDL returns stays valid until the display changes, and is read at once.
        unsafe {
            let id = sys::SDL_GetDisplayForWindow(self.raw);
            let mode = sys::SDL_GetDesktopDisplayMode(id);
            mode.as_ref()
                .map(|m| ((m.w.max(0) as u32, m.h.max(0) as u32), m.pixel_density.max(1.0)))
        }
    }

    /// Resizes to `w` × `h` points.
    pub fn set_size(&self, w: u32, h: u32) {
        // SAFETY: a live window.
        unsafe { sys::SDL_SetWindowSize(self.raw, w as c_int, h as c_int) };
    }

    pub fn centre(&self) {
        // SAFETY: a live window.
        unsafe { sys::SDL_SetWindowPosition(self.raw, sys::SDL_WINDOWPOS_CENTERED, sys::SDL_WINDOWPOS_CENTERED) };
    }

    /// The smallest size the user can make it, in points.
    pub fn set_min_size(&self, w: u32, h: u32) {
        // SAFETY: a live window.
        unsafe { sys::SDL_SetWindowMinimumSize(self.raw, w as c_int, h as c_int) };
    }

    /// Sets the window's icon (title bar, taskbar, the Dock) from `w` × `h` RGBA pixels. False if SDL refused.
    pub fn set_icon(&self, w: u32, h: u32, rgba: &[u8]) -> bool {
        if rgba.len() != w as usize * h as usize * 4 {
            return false;
        }
        // SAFETY: a live window; the surface only borrows `rgba` (SDL doesn't write to it) and is destroyed before
        // the borrow ends, and SDL_SetWindowIcon keeps its own converted copy.
        unsafe {
            let surface = sys::SDL_CreateSurfaceFrom(
                w as c_int,
                h as c_int,
                sys::SDL_PIXELFORMAT_RGBA32,
                rgba.as_ptr().cast_mut().cast(),
                (w * 4) as c_int,
            );
            if surface.is_null() {
                return false;
            }
            let ok = sys::SDL_SetWindowIcon(self.raw, surface);
            sys::SDL_DestroySurface(surface);
            ok
        }
    }

    pub fn set_title(&self, title: &str) {
        let title = cstring(title);
        // SAFETY: a live window and a valid string; SDL copies it.
        unsafe { sys::SDL_SetWindowTitle(self.raw, title.as_ptr()) };
    }

    /// Full screen on or off (the desktop's Settings screen); SDL keeps the window's windowed size and place for the way
    /// back.
    pub fn set_fullscreen(&self, on: bool) {
        // SAFETY: a live window.
        unsafe { sys::SDL_SetWindowFullscreen(self.raw, on) };
    }

    /// Whether the window has the keyboard.
    pub fn focused(&self) -> bool {
        // SAFETY: a live window.
        let flags = unsafe { sys::SDL_GetWindowFlags(self.raw) };
        flags.0 & sys::SDL_WINDOW_INPUT_FOCUS.0 != 0
    }

    /// The system's pointer in the window's points, `None` when it is over another window.
    pub fn mouse(&self) -> Option<(f32, f32)> {
        // SAFETY: plain queries of SDL's mouse state, with valid out pointers.
        unsafe {
            if sys::SDL_GetMouseFocus() != self.raw {
                return None;
            }
            let (mut x, mut y) = (0.0, 0.0);
            sys::SDL_GetMouseState(&mut x, &mut y);
            Some((x, y))
        }
    }

    /// Relative mode: the system's pointer hidden and held still; the mouse's motion still comes in.
    pub fn set_relative_mouse(&self, on: bool) {
        // SAFETY: a live window.
        unsafe { sys::SDL_SetWindowRelativeMouseMode(self.raw, on) };
    }

    /// Confines the system's pointer to the window.
    pub fn set_mouse_grab(&self, on: bool) {
        // SAFETY: a live window.
        unsafe { sys::SDL_SetWindowMouseGrab(self.raw, on) };
    }

    /// Puts the system's pointer at a point of the window, in points.
    pub fn warp_mouse(&self, x: f32, y: f32) {
        // SAFETY: a live window.
        unsafe { sys::SDL_WarpMouseInWindow(self.raw, x, y) };
    }

    /// Typed text comes in as [`Event::Text`]; on a touch screen this opens the soft keyboard.
    pub fn start_text_input(&self) {
        // SAFETY: a live window.
        unsafe { sys::SDL_StartTextInput(self.raw) };
    }

    /// Whether the system's on-screen keyboard is showing now (SDL's view, kept by the [`Event::ScreenKeyboard`]
    /// events).
    pub fn screen_keyboard_shown(&self) -> bool {
        // SAFETY: a live window.
        unsafe { sys::SDL_ScreenKeyboardShown(self.raw) }
    }

    /// Closes the soft keyboard (and stops [`Event::Text`]).
    pub fn stop_text_input(&self) {
        // SAFETY: a live window.
        unsafe { sys::SDL_StopTextInput(self.raw) };
    }

    /// The part of the window clear of notches, rounded corners and home bars, in physical pixels: left, top, width,
    /// height. The whole window where the system reports nothing.
    pub fn safe_area(&self) -> (u32, u32, u32, u32) {
        let d = self.pixel_density();
        let mut r = sys::SDL_Rect { x: 0, y: 0, w: 0, h: 0 };
        // SAFETY: a live window and a valid out pointer.
        let ok = unsafe { sys::SDL_GetWindowSafeArea(self.raw, &mut r) };
        let (w, h) = self.size_in_pixels();
        if !ok || r.w <= 0 || r.h <= 0 {
            return (0, 0, w, h);
        }
        let px = |v: i32| (v.max(0) as f32 * d).round() as u32;
        let (x, y) = (px(r.x).min(w), px(r.y).min(h));
        (x, y, px(r.w).min(w - x), px(r.h).min(h - y))
    }

    /// Opens the system's dialog for choosing a folder, over this window. The answer comes later (not before this
    /// call returns), through [`take_dialog_answers`].
    pub fn show_folder_dialog(&self) {
        if PICKER {
            phone_pick(true);
            return;
        }
        // SAFETY: a live window; the callback is a plain function that uses no userdata (null).
        unsafe {
            sys::SDL_ShowOpenFolderDialog(
                Some(dialog_done),
                std::ptr::null_mut(),
                self.raw,
                std::ptr::null(),
                false,
            )
        };
    }

    /// Opens the system's dialog for choosing one file, over this window. `filters` are (name, patterns) pairs, the
    /// patterns file extensions without dots separated by `;` (`"iso;bin;cue"`), or `"*"` for any file. The answer
    /// comes later, through [`take_dialog_answers`]. On iOS and Android the picker takes any file (the import says what
    /// it can use) and `filters` are not used.
    pub fn show_file_dialog(&self, filters: &[(&str, &str)]) {
        if PICKER {
            phone_pick(false);
            return;
        }
        // SDL may read the filters until the dialog answers, so they are leaked: a few bytes per dialog opened.
        let list: Vec<sys::SDL_DialogFileFilter> = filters
            .iter()
            .map(|(name, pattern)| sys::SDL_DialogFileFilter {
                name: cstring(name).into_raw(),
                pattern: cstring(pattern).into_raw(),
            })
            .collect();
        let list: &'static [sys::SDL_DialogFileFilter] = Box::leak(list.into_boxed_slice());
        // SAFETY: a live window; `list` lives for the rest of the program and its strings are valid C strings; the
        // callback uses no userdata (null).
        unsafe {
            sys::SDL_ShowOpenFileDialog(
                Some(dialog_done),
                std::ptr::null_mut(),
                self.raw,
                list.as_ptr(),
                list.len() as c_int,
                std::ptr::null(),
                false,
            )
        };
    }
}

/// Whether the system's picker is our own (a phone's document picker) instead of SDL's dialogs.
const PICKER: bool = cfg!(any(target_os = "ios", target_os = "android"));

// The document picker is `platform/ios/Sources/picker.m`, linked into the app beside this library. It shows a
// `UIDocumentPickerViewController` for a folder or for any file, opens the chosen item in place (the app holds the
// security-scoped access until it ends) and calls `done` once, on the main thread, with the path, or NULL if the user
// cancelled.
#[cfg(target_os = "ios")]
unsafe extern "C" {
    fn tidewater_ios_pick(folder: c_int, done: unsafe extern "C" fn(*const std::ffi::c_char));
}

/// Shows the iOS document picker for a folder or a file.
#[cfg(target_os = "ios")]
fn phone_pick(folder: bool) {
    // SAFETY: the shim keeps `done` until it calls it, once, and reads nothing else of ours.
    unsafe { tidewater_ios_pick(folder as c_int, ios_picked) };
}

/// Asks the Android activity (`TidewaterActivity`, `platform/android`) to show the Storage Access Framework picker.
/// SDL 3's own file dialog gives `content:` addresses and has no folder dialog on Android, so the activity picks,
/// copies the chosen item into the app's cache and calls [`Java_dev_tidewater_client_TidewaterActivity_nativePicked`].
#[cfg(target_os = "android")]
fn phone_pick(folder: bool) {
    // Messages from 0x8000 up reach the activity's `onUnhandledMessage`, on the UI thread.
    let command = if folder { ANDROID_PICK_FOLDER } else { ANDROID_PICK_FILE };
    // SAFETY: plain integers.
    if !unsafe { sys::SDL_SendAndroidMessage(command, 0) }
        && let Ok(mut answers) = DIALOG_ANSWERS.lock()
    {
        answers.push(None);
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn phone_pick(_folder: bool) {}

/// The activity's message numbers (`TidewaterActivity.java` has the same two).
#[cfg(target_os = "android")]
const ANDROID_PICK_FOLDER: u32 = 0x8001;
#[cfg(target_os = "android")]
const ANDROID_PICK_FILE: u32 = 0x8002;

/// The activity calls this when its picker has finished, from the UI thread: the chosen item, copied into the app's
/// cache, has its path in `pick-result.txt` in the app's files folder (empty when the user cancelled or the copy
/// failed). It is a JNI function by name, found in `libmain.so` when the activity declares it `native`.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn Java_dev_tidewater_client_TidewaterActivity_nativePicked(_env: *mut c_void, _class: *mut c_void) {
    let answer = android_files_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join("pick-result.txt")).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    if let Ok(mut answers) = DIALOG_ANSWERS.lock() {
        answers.push(answer);
    }
}

/// The app's private files folder (SDL's internal storage path).
#[cfg(target_os = "android")]
fn android_files_dir() -> Option<PathBuf> {
    // SAFETY: SDL returns a string it owns (or NULL), read at once.
    let p = unsafe { sys::SDL_GetAndroidInternalStoragePath() };
    // SAFETY: non-NULL is a valid C string.
    (!p.is_null()).then(|| PathBuf::from(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()))
}

/// Points the program at the app's own folders before it reads its command line: `HOME` is the private files folder
/// (so the data, saved games and settings go under it, as on any Unix), and `TIDEWATER_SHARED_DIR` the external files
/// folder that `adb push` and a computer can fill (the first-run screen scans it).
#[cfg(target_os = "android")]
fn android_environment() {
    if let Some(dir) = android_files_dir() {
        // SAFETY: called first thing in `SDL_main`, before the program starts any thread of its own.
        unsafe { std::env::set_var("HOME", dir) };
    }
    // SAFETY: SDL returns a string it owns (or NULL), read at once.
    let p = unsafe { sys::SDL_GetAndroidExternalStoragePath() };
    if !p.is_null() {
        // SAFETY: non-NULL is a valid C string; as above, no other thread yet.
        unsafe { std::env::set_var("TIDEWATER_SHARED_DIR", CStr::from_ptr(p).to_string_lossy().into_owned()) };
    }
}

/// Writes a line to the system log (`adb logcat -s SDL`) as an error.
pub fn log_error(msg: &str) {
    // SAFETY: a fixed format string and a valid C string for its one argument.
    unsafe {
        sys::SDL_LogError(
            sys::SDL_LOG_CATEGORY_APPLICATION.0,
            c"%s".as_ptr(),
            cstring(msg).as_ptr(),
        )
    };
}

/// The picker's answer: a NUL-terminated UTF-8 path valid during the call, or NULL.
#[cfg(target_os = "ios")]
unsafe extern "C" fn ios_picked(path: *const std::ffi::c_char) {
    let path = if path.is_null() {
        None
    } else {
        // SAFETY: a non-NULL path is a valid C string for the length of the call.
        Some(PathBuf::from(
            unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned(),
        ))
    };
    if let Ok(mut answers) = DIALOG_ANSWERS.lock() {
        answers.push(path);
    }
}

/// The iOS app's entry (`platform/ios/Sources/main.c` hands it to `SDL_RunApp`, which starts UIKit and calls it once
/// the app has launched): the same program as the desktop's.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn tidewater_ios_main(_argc: c_int, _argv: *mut *mut std::ffi::c_char) -> c_int {
    crate::app::ios_main()
}

/// The answers of the dialogs, in the order they came: the path chosen, `None` for a cancelled or failed dialog. SDL
/// calls [`dialog_done`] from the main thread or another one, so they wait here for the frame to take them.
static DIALOG_ANSWERS: Mutex<Vec<Option<PathBuf>>> = Mutex::new(Vec::new());

/// SDL's dialog callback: `filelist` is NULL on an error, else a NULL-terminated list of the chosen paths (empty if
/// the dialog was cancelled).
unsafe extern "C" fn dialog_done(_userdata: *mut c_void, filelist: *const *const std::ffi::c_char, _filter: c_int) {
    let path = if filelist.is_null() {
        None
    } else {
        // SAFETY: a non-NULL list holds NULL-terminated UTF-8 strings and ends with a NULL pointer, valid during the
        // callback; only the first is read, after checking it is not the terminator.
        unsafe {
            let first = *filelist;
            (!first.is_null()).then(|| PathBuf::from(CStr::from_ptr(first).to_string_lossy().into_owned()))
        }
    };
    if let Ok(mut answers) = DIALOG_ANSWERS.lock() {
        answers.push(path);
    }
}

/// The dialogs' answers since the last call.
pub fn take_dialog_answers() -> Vec<Option<PathBuf>> {
    DIALOG_ANSWERS
        .lock()
        .map(|mut a| std::mem::take(&mut *a))
        .unwrap_or_default()
}

impl Drop for Window<'_> {
    fn drop(&mut self) {
        // SAFETY: the renderer borrowed the window and is gone by now.
        unsafe { sys::SDL_DestroyWindow(self.raw) };
    }
}

/// Asks the system to open `url` in its own program: a `file://` address of a folder opens in the file manager (the
/// Settings screen's "Open settings folder"). A failure is told on the console and otherwise ignored.
pub fn open_url(url: &str) {
    let c = cstring(url);
    // SAFETY: a valid C string; SDL copies it before it returns.
    if !unsafe { sys::SDL_OpenURL(c.as_ptr()) } {
        eprintln!("{}", error("can't open the folder"));
    }
}

/// Shows or hides the system's pointer.
pub fn show_cursor(on: bool) {
    // SAFETY: plain calls, after SDL_Init.
    unsafe {
        if on {
            sys::SDL_ShowCursor();
        } else {
            sys::SDL_HideCursor();
        }
    }
}

/// Mouse events SDL makes from touches are dropped once real finger events have been seen (see [`poll`]).
static FINGERS_SEEN: AtomicBool = AtomicBool::new(false);

/// Turns off SDL's mouse-from-touch and touch-from-mouse events: the shell reads the fingers themselves (the touch
/// emulation and touch screens, `docs/design/touch.md`).
pub fn own_the_fingers() {
    // SAFETY: the hints' names are SDL's own constants and the values valid C strings; SDL copies both.
    unsafe {
        sys::SDL_SetHint(sys::SDL_HINT_TOUCH_MOUSE_EVENTS, c"0".as_ptr());
        sys::SDL_SetHint(sys::SDL_HINT_MOUSE_TOUCH_EVENTS, c"0".as_ptr());
    }
}

/// Whether a touch screen (a device the fingers touch directly, not a trackpad) is attached.
pub fn has_touch_screen() -> bool {
    let mut n: c_int = 0;
    // SAFETY: a valid out pointer; the array SDL returns (or null) is ours to free and is read before that.
    unsafe {
        let ids = sys::SDL_GetTouchDevices(&mut n);
        if ids.is_null() {
            return false;
        }
        let found =
            (0..n.max(0) as usize).any(|i| sys::SDL_GetTouchDeviceType(*ids.add(i)) == sys::SDL_TOUCH_DEVICE_DIRECT);
        sys::SDL_free(ids.cast());
        found
    }
}

/// A renderer for a window with one streaming RGBA texture, scaled with the nearest texel, or (the "sharp bilinear"
/// fit for fractional scales) first by a whole number with the nearest texel and then down to size with a linear one.
pub struct Renderer<'w> {
    raw: *mut sys::SDL_Renderer,
    texture: *mut sys::SDL_Texture,
    size: (u32, u32),
    /// The sharp pass's intermediate picture and its whole-number scale.
    sharp: Option<(*mut sys::SDL_Texture, u32)>,
    /// The touch chrome drawn over the picture: its texture and size.
    overlay: Option<(*mut sys::SDL_Texture, (u32, u32))>,
    _window: PhantomData<&'w Window<'w>>,
}

impl<'w> Renderer<'w> {
    /// A renderer with a `size` texture; `vsync`: present waits for the display.
    pub fn new(window: &'w Window<'w>, size: (u32, u32), vsync: bool) -> Result<Self> {
        // SAFETY: a live window; a null name picks SDL's default renderer.
        let raw = unsafe { sys::SDL_CreateRenderer(window.raw, std::ptr::null()) };
        if raw.is_null() {
            return Err(error("SDL_CreateRenderer"));
        }
        // SAFETY: a live renderer.
        let texture = unsafe {
            sys::SDL_SetRenderVSync(raw, if vsync { 1 } else { 0 });
            sys::SDL_CreateTexture(
                raw,
                sys::SDL_PIXELFORMAT_RGBA32,
                sys::SDL_TEXTUREACCESS_STREAMING,
                size.0 as c_int,
                size.1 as c_int,
            )
        };
        if texture.is_null() {
            let e = error("SDL_CreateTexture");
            // SAFETY: the renderer made above, not used again.
            unsafe { sys::SDL_DestroyRenderer(raw) };
            return Err(e);
        }
        // SAFETY: a live texture.
        unsafe { sys::SDL_SetTextureScaleMode(texture, sys::SDL_SCALEMODE_NEAREST) };
        Ok(Renderer {
            raw,
            texture,
            size,
            sharp: None,
            overlay: None,
            _window: PhantomData,
        })
    }

    /// Makes the picture's texture `size` pixels (a game with `widescreen` has a wider screen); the next `draw` gets a
    /// picture of that size. Does nothing if it is the size already.
    pub fn set_size(&mut self, size: (u32, u32)) -> Result<()> {
        if size == self.size {
            return Ok(());
        }
        // SAFETY: a live renderer.
        let texture = unsafe {
            sys::SDL_CreateTexture(
                self.raw,
                sys::SDL_PIXELFORMAT_RGBA32,
                sys::SDL_TEXTUREACCESS_STREAMING,
                size.0 as c_int,
                size.1 as c_int,
            )
        };
        if texture.is_null() {
            return Err(error("SDL_CreateTexture"));
        }
        // SAFETY: a live texture; the old ones were made by this renderer and are not used again.
        unsafe {
            sys::SDL_SetTextureScaleMode(texture, sys::SDL_SCALEMODE_NEAREST);
            sys::SDL_DestroyTexture(self.texture);
            if let Some((t, _)) = self.sharp.take() {
                sys::SDL_DestroyTexture(t);
            }
        }
        self.texture = texture;
        self.size = size;
        Ok(())
    }

    /// The window's drawable size in pixels.
    pub fn output_size(&self) -> (u32, u32) {
        let (mut w, mut h) = (0, 0);
        // SAFETY: a live renderer and valid out pointers.
        unsafe { sys::SDL_GetCurrentRenderOutputSize(self.raw, &mut w, &mut h) };
        (w.max(0) as u32, h.max(0) as u32)
    }

    /// Clears the window to black and draws `rgba` (the texture's size) over `dst` (x, y, width, height in pixels).
    /// `sharp`: the whole-number scale to draw it at first, then scale linearly to `dst` (none: straight to `dst` with
    /// the nearest texel).
    pub fn draw(&mut self, rgba: &[u8], dst: (u32, u32, u32, u32), sharp: Option<u32>) {
        let pitch = self.size.0 as usize * 4;
        assert_eq!(
            rgba.len(),
            pitch * self.size.1 as usize,
            "the picture is the texture's size"
        );
        let dst = sys::SDL_FRect {
            x: dst.0 as f32,
            y: dst.1 as f32,
            w: dst.2 as f32,
            h: dst.3 as f32,
        };
        // SAFETY: a live renderer and texture; `rgba` holds the texture's whole size at `pitch` bytes a row.
        unsafe {
            sys::SDL_UpdateTexture(self.texture, std::ptr::null(), rgba.as_ptr().cast(), pitch as c_int);
        }
        let stepped = sharp.and_then(|n| self.sharp_target(n.max(1)));
        // SAFETY: a live renderer and textures; the target is ours and is unset again before the window is drawn.
        unsafe {
            if let Some(target) = stepped {
                sys::SDL_SetRenderTarget(self.raw, target);
                sys::SDL_RenderTexture(self.raw, self.texture, std::ptr::null(), std::ptr::null());
                sys::SDL_SetRenderTarget(self.raw, std::ptr::null_mut());
            }
            sys::SDL_SetRenderDrawColor(self.raw, 0, 0, 0, 255);
            sys::SDL_RenderClear(self.raw);
            sys::SDL_RenderTexture(self.raw, stepped.unwrap_or(self.texture), std::ptr::null(), &dst);
        }
        self.draw_overlay();
    }

    /// The intermediate picture for a whole-number scale `n`, made when the scale changes.
    fn sharp_target(&mut self, n: u32) -> Option<*mut sys::SDL_Texture> {
        if let Some((t, have)) = self.sharp {
            if have == n {
                return Some(t);
            }
            // SAFETY: made below, not used again.
            unsafe { sys::SDL_DestroyTexture(t) };
            self.sharp = None;
        }
        // SAFETY: a live renderer.
        let t = unsafe {
            sys::SDL_CreateTexture(
                self.raw,
                sys::SDL_PIXELFORMAT_RGBA32,
                sys::SDL_TEXTUREACCESS_TARGET,
                (self.size.0 * n) as c_int,
                (self.size.1 * n) as c_int,
            )
        };
        if t.is_null() {
            return None;
        }
        // SAFETY: a live texture.
        unsafe { sys::SDL_SetTextureScaleMode(t, sys::SDL_SCALEMODE_LINEAR) };
        self.sharp = Some((t, n));
        Some(t)
    }

    /// Sets the chrome's picture (RGBA, `size` pixels, the window's size), or takes it away.
    pub fn set_overlay(&mut self, picture: Option<((u32, u32), &[u8])>) {
        let Some((size, rgba)) = picture else {
            if let Some((t, _)) = self.overlay.take() {
                // SAFETY: made below, not used again.
                unsafe { sys::SDL_DestroyTexture(t) };
            }
            return;
        };
        if rgba.len() != size.0 as usize * size.1 as usize * 4 || size.0 == 0 || size.1 == 0 {
            return;
        }
        if self.overlay.is_some_and(|(_, s)| s != size) {
            self.set_overlay(None);
        }
        if self.overlay.is_none() {
            // SAFETY: a live renderer.
            let t = unsafe {
                sys::SDL_CreateTexture(
                    self.raw,
                    sys::SDL_PIXELFORMAT_RGBA32,
                    sys::SDL_TEXTUREACCESS_STREAMING,
                    size.0 as c_int,
                    size.1 as c_int,
                )
            };
            if t.is_null() {
                return;
            }
            // SAFETY: a live texture.
            unsafe {
                sys::SDL_SetTextureBlendMode(t, sys::SDL_BLENDMODE_BLEND);
                sys::SDL_SetTextureScaleMode(t, sys::SDL_SCALEMODE_NEAREST);
            }
            self.overlay = Some((t, size));
        }
        if let Some((t, _)) = self.overlay {
            // SAFETY: a live texture of `size`; `rgba` holds all of it at `size.0 * 4` bytes a row.
            unsafe { sys::SDL_UpdateTexture(t, std::ptr::null(), rgba.as_ptr().cast(), (size.0 * 4) as c_int) };
        }
    }

    fn draw_overlay(&mut self) {
        if let Some((t, _)) = self.overlay {
            // SAFETY: a live renderer and texture.
            unsafe { sys::SDL_RenderTexture(self.raw, t, std::ptr::null(), std::ptr::null()) };
        }
    }

    /// What was drawn since the last present, as the window will show it: width, height and RGBA bytes.
    pub fn read_pixels(&mut self) -> Option<(usize, usize, Vec<u8>)> {
        // SAFETY: a live renderer; the surfaces SDL returns are ours to free, and their pixels are `pitch` bytes a
        // row for `h` rows.
        unsafe {
            let read = sys::SDL_RenderReadPixels(self.raw, std::ptr::null());
            if read.is_null() {
                return None;
            }
            let rgba = sys::SDL_ConvertSurface(read, sys::SDL_PIXELFORMAT_RGBA32);
            sys::SDL_DestroySurface(read);
            let s = rgba.as_ref()?;
            let (w, h, pitch) = (s.w.max(0) as usize, s.h.max(0) as usize, s.pitch.max(0) as usize);
            let mut out = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                let row = std::slice::from_raw_parts(s.pixels.cast::<u8>().add(y * pitch), w * 4);
                out.extend_from_slice(row);
            }
            sys::SDL_DestroySurface(rgba);
            Some((w, h, out))
        }
    }

    /// Shows what was drawn.
    pub fn present(&mut self) {
        // SAFETY: a live renderer.
        unsafe { sys::SDL_RenderPresent(self.raw) };
    }
}

impl Drop for Renderer<'_> {
    fn drop(&mut self) {
        // SAFETY: made by `new`, not used again; the textures go before their renderer.
        unsafe {
            if let Some((t, _)) = self.sharp {
                sys::SDL_DestroyTexture(t);
            }
            if let Some((t, _)) = self.overlay {
                sys::SDL_DestroyTexture(t);
            }
            sys::SDL_DestroyTexture(self.texture);
            sys::SDL_DestroyRenderer(self.raw);
        }
    }
}

/// The system's events the shell uses.
pub enum Event {
    /// The user asked to quit (closed the window, or the system's quit).
    Quit,
    FocusLost,
    /// A key, by its place (SDL's scancode).
    Key {
        scancode: i32,
        down: bool,
    },
    /// Text typed by the last key.
    Text(String),
    /// A mouse button (SDL's number: 1 left, 2 middle, 3 right, 4 and 5 the side buttons), with the pointer's place in
    /// the window's points.
    Button {
        button: u8,
        down: bool,
        x: f32,
        y: f32,
    },
    /// The wheel, in steps (fractions from a touchpad); positive away from the user.
    Wheel {
        y: f32,
    },
    /// The mouse moved, in points; `x` and `y` are where it is.
    Motion {
        dx: f32,
        dy: f32,
        x: f32,
        y: f32,
    },
    /// A finger on a touch screen: its number, what it did (0 down, 1 moved, 2 up, 3 taken away) and where, as a
    /// fraction of the window (0 to 1 each way).
    Finger {
        id: u64,
        phase: u8,
        x: f32,
        y: f32,
    },
    /// A file or folder dropped on the window.
    Drop(PathBuf),
    /// The system's on-screen keyboard was shown (`true`) or hidden (`false`), by us or by the user (its own hide key,
    /// Android's Back); SDL sends these on iOS and Android.
    ScreenKeyboard(bool),
    Other,
}

/// The next event waiting, if any.
pub fn poll() -> Option<Event> {
    let mut e = std::mem::MaybeUninit::<sys::SDL_Event>::uninit();
    // SAFETY: SDL fills the event when it returns true; each variant is read by the type SDL gave it.
    unsafe {
        if !sys::SDL_PollEvent(e.as_mut_ptr()) {
            return None;
        }
        let e = e.assume_init();
        let t = sys::SDL_EventType(e.r#type);
        Some(match t {
            sys::SDL_EVENT_QUIT | sys::SDL_EVENT_WINDOW_CLOSE_REQUESTED => Event::Quit,
            sys::SDL_EVENT_WINDOW_FOCUS_LOST => Event::FocusLost,
            sys::SDL_EVENT_KEY_DOWN | sys::SDL_EVENT_KEY_UP => Event::Key {
                scancode: e.key.scancode.0,
                down: e.key.down,
            },
            sys::SDL_EVENT_TEXT_INPUT if !e.text.text.is_null() => {
                Event::Text(CStr::from_ptr(e.text.text).to_string_lossy().into_owned())
            }
            // What SDL makes of a finger as a mouse is dropped once the fingers themselves are being read.
            sys::SDL_EVENT_MOUSE_BUTTON_DOWN | sys::SDL_EVENT_MOUSE_BUTTON_UP
                if FINGERS_SEEN.load(Ordering::Relaxed) && e.button.which == sys::SDL_TOUCH_MOUSEID =>
            {
                Event::Other
            }
            sys::SDL_EVENT_MOUSE_MOTION
                if FINGERS_SEEN.load(Ordering::Relaxed) && e.motion.which == sys::SDL_TOUCH_MOUSEID =>
            {
                Event::Other
            }
            sys::SDL_EVENT_MOUSE_BUTTON_DOWN | sys::SDL_EVENT_MOUSE_BUTTON_UP => Event::Button {
                button: e.button.button,
                down: e.button.down,
                x: e.button.x,
                y: e.button.y,
            },
            sys::SDL_EVENT_MOUSE_WHEEL => Event::Wheel { y: e.wheel.y },
            sys::SDL_EVENT_MOUSE_MOTION => Event::Motion {
                dx: e.motion.xrel,
                dy: e.motion.yrel,
                x: e.motion.x,
                y: e.motion.y,
            },
            // Only a screen the fingers touch directly: a trackpad's fingers are not pointers on the picture.
            sys::SDL_EVENT_FINGER_DOWN
            | sys::SDL_EVENT_FINGER_MOTION
            | sys::SDL_EVENT_FINGER_UP
            | sys::SDL_EVENT_FINGER_CANCELED
                if sys::SDL_GetTouchDeviceType(e.tfinger.touchID) == sys::SDL_TOUCH_DEVICE_DIRECT =>
            {
                FINGERS_SEEN.store(true, Ordering::Relaxed);
                Event::Finger {
                    id: e.tfinger.fingerID.into(),
                    phase: match t {
                        sys::SDL_EVENT_FINGER_DOWN => 0,
                        sys::SDL_EVENT_FINGER_MOTION => 1,
                        sys::SDL_EVENT_FINGER_UP => 2,
                        _ => 3,
                    },
                    x: e.tfinger.x,
                    y: e.tfinger.y,
                }
            }
            sys::SDL_EVENT_SCREEN_KEYBOARD_SHOWN => Event::ScreenKeyboard(true),
            sys::SDL_EVENT_SCREEN_KEYBOARD_HIDDEN => Event::ScreenKeyboard(false),
            sys::SDL_EVENT_DROP_FILE if !e.drop.data.is_null() => Event::Drop(PathBuf::from(
                CStr::from_ptr(e.drop.data).to_string_lossy().into_owned(),
            )),
            _ => Event::Other,
        })
    }
}

/// SDL's name for a key.
pub fn scancode_name(scancode: i32) -> String {
    // SAFETY: SDL returns a valid string (empty for a key without a name).
    unsafe { CStr::from_ptr(sys::SDL_GetScancodeName(sys::SDL_Scancode(scancode))) }
        .to_string_lossy()
        .into_owned()
}

/// What the audio callback has done, for measuring.
#[derive(Default)]
pub struct AudioStats {
    pub calls: AtomicU64,
    /// Sample frames handed over.
    pub frames: AtomicU64,
    /// Nanoseconds after `epoch` of the first and last call, and the longest gap between two calls.
    pub first_ns: AtomicU64,
    pub last_ns: AtomicU64,
    pub max_gap_ns: AtomicU64,
}

/// What the callback owns.
struct Feed<F> {
    fill: F,
    buf: Vec<i16>,
    stats: Arc<AudioStats>,
    epoch: Instant,
}

/// An audio stream to the default playback device, 16-bit stereo at a fixed rate (SDL converts to the device's):
/// SDL's audio thread calls `fill` for each piece the device needs, in interleaved samples.
pub struct AudioOut<'s, F> {
    stream: *mut sys::SDL_AudioStream,
    feed: *mut Feed<F>,
    _sdl: PhantomData<&'s Sdl>,
}

unsafe extern "C" fn feed_callback<F: FnMut(&mut [i16])>(
    userdata: *mut c_void,
    stream: *mut sys::SDL_AudioStream,
    additional: c_int,
    _total: c_int,
) {
    // SAFETY: `userdata` is the `Feed` made by `AudioOut::open`, used only by this callback until the stream is
    // destroyed (SDL calls it from one thread at a time).
    let feed = unsafe { &mut *userdata.cast::<Feed<F>>() };
    let now = feed.epoch.elapsed().as_nanos() as u64;
    let s = &feed.stats;
    if s.calls.fetch_add(1, Ordering::Relaxed) == 0 {
        s.first_ns.store(now, Ordering::Relaxed);
    } else {
        let gap = now.saturating_sub(s.last_ns.load(Ordering::Relaxed));
        s.max_gap_ns.fetch_max(gap, Ordering::Relaxed);
    }
    s.last_ns.store(now, Ordering::Relaxed);
    let samples = (additional.max(0) as usize / 2) & !1;
    if samples == 0 {
        return;
    }
    feed.buf.resize(samples, 0);
    (feed.fill)(&mut feed.buf);
    s.frames.fetch_add(samples as u64 / 2, Ordering::Relaxed);
    // SAFETY: a live stream and `samples` valid samples.
    unsafe { sys::SDL_PutAudioStreamData(stream, feed.buf.as_ptr().cast(), (samples * 2) as c_int) };
}

impl<'s, F: FnMut(&mut [i16]) + Send + 'static> AudioOut<'s, F> {
    /// Opens the default playback device at `rate` sample frames a second, stereo, and starts it.
    pub fn open(_sdl: &'s Sdl, rate: u32, fill: F, stats: Arc<AudioStats>) -> Result<Self> {
        let spec = sys::SDL_AudioSpec {
            format: sys::SDL_AUDIO_S16,
            channels: 2,
            freq: rate as c_int,
        };
        let feed = Box::into_raw(Box::new(Feed {
            fill,
            buf: Vec::new(),
            stats,
            epoch: Instant::now(),
        }));
        // SAFETY: a valid spec; `feed` lives until `Drop` destroys the stream, after which SDL no longer calls back.
        let stream = unsafe {
            sys::SDL_OpenAudioDeviceStream(
                sys::SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK,
                &spec,
                Some(feed_callback::<F>),
                feed.cast(),
            )
        };
        if stream.is_null() {
            let e = error("SDL_OpenAudioDeviceStream");
            // SAFETY: SDL didn't take it.
            drop(unsafe { Box::from_raw(feed) });
            return Err(e);
        }
        // SAFETY: a live stream; its device starts paused.
        unsafe { sys::SDL_ResumeAudioStreamDevice(stream) };
        Ok(AudioOut {
            stream,
            feed,
            _sdl: PhantomData,
        })
    }

    /// The device's own format: its rate, channels and buffer in sample frames.
    pub fn device_format(&self) -> Option<(u32, u32, u32)> {
        let mut spec = sys::SDL_AudioSpec {
            format: sys::SDL_AUDIO_S16,
            channels: 0,
            freq: 0,
        };
        let mut frames = 0;
        // SAFETY: a live stream and valid out pointers.
        let ok = unsafe {
            let dev = sys::SDL_GetAudioStreamDevice(self.stream);
            sys::SDL_GetAudioDeviceFormat(dev, &mut spec, &mut frames)
        };
        ok.then_some((
            spec.freq.max(0) as u32,
            spec.channels.max(0) as u32,
            frames.max(0) as u32,
        ))
    }
}

impl<F> Drop for AudioOut<'_, F> {
    fn drop(&mut self) {
        // SAFETY: destroying the stream stops the callback first; then the feed is no longer shared.
        unsafe {
            sys::SDL_DestroyAudioStream(self.stream);
            drop(Box::from_raw(self.feed));
        }
    }
}

/// The Android app's entry. SDL's Java activity loads `libmain.so`, starts a thread and calls this by name with the
/// arguments `TidewaterActivity.getArguments()` gave: the same program as the desktop's.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn SDL_main(argc: c_int, argv: *mut *mut std::ffi::c_char) -> c_int {
    android_environment();
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    for i in 0..argc.max(0) as usize {
        // SAFETY: SDL passes `argc` valid, NUL-terminated strings in `argv`.
        let a = unsafe { *argv.add(i) };
        if !a.is_null() {
            // SAFETY: as above.
            args.push(unsafe { CStr::from_ptr(a) }.to_string_lossy().into_owned().into());
        }
    }
    if args.is_empty() {
        args.push("tidewater".into());
    }
    crate::app::android_main(args)
}
