//! The first-run screen's rules, driven through the core's input with a fake import backend.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;

use super::*;
use crate::args::Args;
use crate::core::Platform;
use crate::input::{Button, Buttons, KeyEvent, WindowGeom};
use crate::storage::{Desktop, Store};

/// Imports nothing: records the paths it was asked for, and fails for a path with "bad" in it.
#[derive(Default)]
struct Fake {
    found: Vec<Candidate>,
    calls: Mutex<Vec<PathBuf>>,
    searches: AtomicUsize,
}

impl Backend for Fake {
    fn search(&self) -> Vec<Candidate> {
        self.searches.fetch_add(1, Ordering::SeqCst);
        self.found.clone()
    }

    fn import(&self, path: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Candidate, String> {
        self.calls.lock().unwrap().push(path.to_path_buf());
        for (step, done, total, file) in [
            (Step::ReadingDisc, 0, 200, "Install.mpq"),
            (Step::ReadingDisc, 200, 200, "War2Dat.mpq"),
            (Step::Checking, 0, 0, ""),
            (Step::Copying, 50, 100, "War2Dat.mpq"),
            (Step::Copying, 100, 100, "Install.mpq"),
        ] {
            progress(&Progress {
                step,
                file,
                done,
                total,
            });
        }
        if path.to_string_lossy().contains("bad") {
            Err("This looks like the 1995 DOS edition (it has .WAR files).".to_string())
        } else {
            Ok(candidate("Warcraft II: Battle.net Edition (GOG)", path))
        }
    }
}

fn candidate(edition: &str, path: impl Into<PathBuf>) -> Candidate {
    Candidate {
        path: path.into(),
        edition: edition.to_string(),
        extras: true,
        maps: 153,
    }
}

/// The window and a pointer a test sets.
#[derive(Default)]
struct Window {
    pointer: Option<(f32, f32)>,
}

impl Platform for Window {
    fn window(&self) -> Option<WindowGeom> {
        Some(WindowGeom::new((640, 480), 1.0))
    }

    fn pointer(&mut self, _keys: &Buttons<Key>, _game_pointer: bool) -> (Option<(f32, f32)>, bool) {
        (self.pointer, true)
    }
}

struct Rig {
    core: Core,
    fake: Arc<Fake>,
    window: Window,
    dir: tempfile::TempDir,
}

fn args(dir: &Path) -> Args {
    let mut a = Args::parse_from(["tidewater", "--data-dir", &dir.to_string_lossy()]);
    // The environment's `TIDEWATER_INSTALL` (set for the full check) is not what these tests are about.
    a.install = None;
    a
}

fn store(dir: &Path) -> Store {
    Store(Arc::new(Desktop::from_data_dir(
        dir,
        dir.join("Save"),
        dir.join("settings.txt"),
        dir.to_path_buf(),
    )))
}

/// Two installs listed; the copy runs inside the frame that starts it.
fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        found: vec![
            candidate("Warcraft II: Battle.net Edition (GOG)", "/games/one"),
            candidate("Warcraft II: Battle.net Edition", "/games/two"),
        ],
        ..Fake::default()
    });
    let list = fake.found.clone();
    let core = crate::host::first_run_core(args(dir.path()), store(dir.path()), fake.clone(), Some(list), None);
    let mut rig = Rig {
        core,
        fake,
        window: Window::default(),
        dir,
    };
    rig.frame();
    rig
}

impl Rig {
    fn frame(&mut self) {
        self.core.advance(Duration::from_millis(16), &mut self.window);
    }

    /// A key pressed and released, with a frame for each.
    fn tap(&mut self, key: Key) {
        for pressed in [true, false] {
            self.core.input.key(KeyEvent {
                key,
                pressed,
                text: None,
            });
            self.frame();
        }
    }

    fn screen(&self) -> &FirstRun {
        self.core.first_run.as_ref().expect("the first-run screen")
    }

    fn page(&self) -> Page {
        self.screen().page.clone()
    }

    fn drop_path(&mut self, path: &str) {
        self.core.input.dropped(path.into());
        self.frame();
    }

    /// Frames until `done` holds (the copy runs on a thread), for at most a few seconds.
    fn wait_for(&mut self, done: impl Fn(&Rig) -> bool) {
        for _ in 0..1000 {
            if done(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(3));
            self.frame();
        }
        panic!("the screen never got there");
    }

    /// The screen pixel at the middle of `item`, as a window pointer.
    fn point_at(&mut self, item: Item) {
        self.window.pointer = self.screen().centre_of(item);
    }
}

#[test]
fn the_core_starts_on_the_first_run_screen_with_no_game_files() {
    let r = rig();
    assert_eq!(r.core.states.get(), AppState::FirstRun);
    assert_eq!(r.page(), Page::List);
    assert_eq!(r.screen().candidates.len(), 2);
    assert!(r.core.layers.iter().count() >= 1, "the screen's picture is a layer");
    assert!(
        r.core.frame.rgba.chunks(4).any(|p| p[..3] != [0, 0, 0]),
        "something is drawn"
    );
}

#[test]
fn up_down_and_tab_move_the_focus_and_wrap() {
    let mut r = rig();
    assert_eq!(r.screen().focused(), Some(Item::Candidate(0)));
    r.tap(Key::Down);
    assert_eq!(r.screen().focused(), Some(Item::Candidate(1)));
    r.tap(Key::Tab);
    assert_eq!(r.screen().focused(), Some(Item::Folder));
    r.tap(Key::Up);
    assert_eq!(r.screen().focused(), Some(Item::Candidate(1)));
    // Shift+Tab goes back.
    r.core.input.key(KeyEvent {
        key: Key::ShiftLeft,
        pressed: true,
        text: None,
    });
    r.tap(Key::Tab);
    assert_eq!(r.screen().focused(), Some(Item::Candidate(0)));
    r.tap(Key::Up);
    assert_eq!(r.screen().focused(), Some(Item::Quit), "wraps to the last button");
}

#[test]
fn enter_on_an_install_imports_it_and_shows_the_result_line() {
    let mut r = rig();
    r.tap(Key::Enter);
    assert_eq!(*r.fake.calls.lock().unwrap(), [PathBuf::from("/games/one")]);
    assert_eq!(
        r.page(),
        Page::Done("Imported Warcraft II: Battle.net Edition (GOG). Videos and music: yes. Maps: 153.".into())
    );
}

#[test]
fn the_result_line_says_when_there_are_no_videos_and_music() {
    let mut c = candidate("Warcraft II: Battle.net Edition", "/x");
    c.extras = false;
    c.maps = 92;
    assert_eq!(
        c.result_line(),
        "Imported Warcraft II: Battle.net Edition. Videos and music: no. Maps: 92."
    );
}

#[test]
fn the_progress_bar_follows_the_copy() {
    let mut r = rig();
    let mut s = FirstRun::with_candidates(r.fake.clone(), Vec::new(), true);
    let bar = |s: &mut FirstRun| {
        let mut pic = crate::frontend::blank();
        s.dirty = true;
        s.draw(&mut pic);
        pic.rgba.chunks(4).filter(|p| p[..3] == BAR).count()
    };
    s.page = Page::Importing {
        name: "x".into(),
        step: Some(Step::Copying),
        unpacked: false,
        done: 0,
        total: 100,
        file: String::new(),
    };
    let empty = bar(&mut s);
    s.page = Page::Importing {
        name: "x".into(),
        step: Some(Step::Copying),
        unpacked: false,
        done: 50,
        total: 100,
        file: "War2Dat.mpq".into(),
    };
    let half = bar(&mut s);
    s.page = Page::Importing {
        name: "x".into(),
        step: Some(Step::Copying),
        unpacked: false,
        done: 100,
        total: 100,
        file: String::new(),
    };
    let full = bar(&mut s);
    assert!(empty < half && half < full, "{empty} {half} {full}");
    // The importing screen shows the copy's progress on the core's frame too.
    r.core.first_run.as_mut().unwrap().page = Page::Importing {
        name: "x".into(),
        step: Some(Step::Copying),
        unpacked: false,
        done: 40,
        total: 100,
        file: String::new(),
    };
    r.core.first_run.as_mut().unwrap().dirty = true;
    r.frame();
    assert!(r.core.frame.rgba.chunks(4).any(|p| p[..3] == BAR));
}

#[test]
fn the_copy_on_a_thread_reports_progress_and_ends() {
    let mut r = rig();
    r.core.first_run = Some(FirstRun::with_candidates(r.fake.clone(), r.fake.found.clone(), true));
    r.tap(Key::Enter);
    r.wait_for(|r| matches!(r.page(), Page::Done(_)));
    assert_eq!(r.fake.calls.lock().unwrap().len(), 1);
}

#[test]
fn a_failed_import_shows_the_message_and_a_way_back() {
    let mut r = rig();
    r.drop_path("/downloads/bad-folder");
    let Page::Failed(message) = r.page() else {
        panic!("{:?}", r.page())
    };
    assert!(message.contains("1995 DOS edition"));
    assert_eq!(r.screen().focused(), Some(Item::Back));
    // Esc goes back to the list, not out of the program.
    r.tap(Key::Escape);
    assert_eq!(r.page(), Page::List);
    assert!(!r.core.exit);
    // Enter on Back does the same.
    r.drop_path("/downloads/bad-again");
    r.tap(Key::Enter);
    assert_eq!(r.page(), Page::List);
}

#[test]
fn a_long_message_is_wrapped_to_the_screen() {
    let lines = wrap(
        "Found Install.mpq but not War2Dat.mpq. If this is the CD, choose the folder where the game was installed, not \
         the disc.",
        38,
    );
    assert!(lines.len() >= 3);
    assert!(lines.iter().all(|l| l.chars().count() <= 38), "{lines:?}");
    // Nothing is lost.
    assert_eq!(lines.join(" ").split_whitespace().count(), 21);
    // A word longer than a line is broken.
    let long = "x".repeat(100);
    let broken = wrap(&long, 38);
    assert_eq!(broken.iter().map(|l| l.len()).sum::<usize>(), 100);
    assert!(broken.iter().all(|l| l.len() <= 38));
    assert_eq!(fit_middle("/short", 20), "/short");
    let cut = fit_middle("/Users/<user>/a/very/long/path/to/Warcraft II", 20);
    assert_eq!(cut.chars().count(), 20);
    assert!(
        cut.starts_with("/Users/") && cut.ends_with("raft II") && cut.contains("..."),
        "{cut}"
    );
}

#[test]
fn escape_quits_from_the_list() {
    let mut r = rig();
    assert!(!r.core.exit);
    r.tap(Key::Escape);
    assert!(r.core.exit);
}

#[test]
fn the_quit_button_quits() {
    let mut r = rig();
    r.tap(Key::Up); // wraps to Quit
    assert_eq!(r.screen().focused(), Some(Item::Quit));
    r.tap(Key::Enter);
    assert!(r.core.exit);
    // Quitting from here doesn't leave a settings file behind.
    r.core.store_settings();
    assert!(!r.dir.path().join("settings.txt").exists());
}

#[test]
fn the_choose_buttons_ask_the_shell_for_a_dialog() {
    let mut r = rig();
    for (item, kind) in [
        (Item::Folder, PickKind::Folder),
        (Item::Installer, PickKind::Installer),
        (Item::Disc, PickKind::Disc),
    ] {
        r.point_at(item);
        r.frame();
        r.core.input.button(Button::Left, true);
        r.frame();
        r.core.input.button(Button::Left, false);
        r.frame();
        assert_eq!(r.core.take_pick_requests(), [kind], "{item:?}");
        assert!(r.core.take_pick_requests().is_empty(), "taken once");
        // A cancelled dialog changes nothing.
        r.core.input.picked(None);
        r.frame();
        assert_eq!(r.page(), Page::List);
        assert!(!r.screen().picking);
    }
}

#[test]
fn the_screen_waits_while_a_dialog_is_open() {
    let mut r = rig();
    r.tap(Key::Tab);
    r.tap(Key::Tab);
    r.tap(Key::Enter); // Choose GOG installer
    assert!(r.screen().picking);
    let focus = r.screen().focus;
    r.tap(Key::Down);
    r.tap(Key::Escape);
    assert_eq!(r.screen().focus, focus, "keys are ignored while the dialog is up");
    assert!(!r.core.exit);
    // The answer imports the chosen path (an installer: the import crate says what it thinks of it).
    r.core.input.picked(Some("/downloads/setup_war2.exe".into()));
    r.frame();
    assert_eq!(
        *r.fake.calls.lock().unwrap(),
        [PathBuf::from("/downloads/setup_war2.exe")]
    );
    assert!(matches!(r.page(), Page::Done(_)));
}

#[test]
fn a_dropped_path_is_imported() {
    let mut r = rig();
    r.drop_path("/downloads/Warcraft II");
    assert_eq!(*r.fake.calls.lock().unwrap(), [PathBuf::from("/downloads/Warcraft II")]);
    assert!(matches!(r.page(), Page::Done(_)));
}

#[test]
fn a_drop_during_an_import_is_ignored() {
    let mut r = rig();
    r.core.first_run = Some(FirstRun::with_candidates(r.fake.clone(), Vec::new(), true));
    r.drop_path("/downloads/first");
    // Whether or not the first has finished, a second drop while it runs must not start another copy.
    let mid = matches!(r.page(), Page::Importing { .. });
    r.drop_path("/downloads/second");
    r.wait_for(|r| matches!(r.page(), Page::Done(_) | Page::Failed(_)));
    let calls = r.fake.calls.lock().unwrap().clone();
    assert_eq!(calls[0], PathBuf::from("/downloads/first"));
    if mid {
        assert_eq!(calls.len(), 1, "{calls:?}");
    }
}

#[test]
fn the_hint_about_dropping_is_on_the_screen() {
    // The text is drawn with the font; check by drawing the same words alone and finding their pixels.
    let r = rig();
    let mut line = crate::frontend::blank();
    line.rgba.chunks_mut(4).for_each(|p| p.copy_from_slice(&[0, 0, 0, 255]));
    let hint = "You can also drop a game folder, an installer or a CD image on this window.";
    Canvas { pic: &mut line }.text_centred(436, 1, TEXT, hint);
    let (y0, y1) = (436 * 640 * 4, 444 * 640 * 4);
    let on = |rgba: &[u8]| {
        rgba[y0..y1]
            .chunks(4)
            .enumerate()
            .filter(|(_, p)| p[..3] == TEXT)
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    let wanted = on(&line.rgba);
    assert!(!wanted.is_empty());
    let shown = on(&r.core.frame.rgba);
    assert!(
        wanted.iter().all(|i| shown.contains(i)),
        "the hint's pixels are in the frame"
    );
}

#[test]
fn the_mouse_moves_the_focus_and_a_click_chooses() {
    let mut r = rig();
    r.point_at(Item::Candidate(1));
    r.frame();
    assert_eq!(r.screen().focused(), Some(Item::Candidate(1)), "hovering focuses");
    // The keyboard still works while the pointer rests.
    r.tap(Key::Down);
    assert_eq!(r.screen().focused(), Some(Item::Folder));
    r.core.input.button(Button::Left, true);
    r.frame();
    r.core.input.button(Button::Left, false);
    r.frame();
    assert_eq!(
        *r.fake.calls.lock().unwrap(),
        [PathBuf::from("/games/two")],
        "the click chose the hovered item"
    );
}

#[test]
fn the_result_goes_on_when_the_time_is_up_or_a_key_is_pressed() {
    // The fake imports nothing, so the game files can't be opened afterwards: the screen says so.
    let mut r = rig();
    r.tap(Key::Enter);
    assert!(matches!(r.page(), Page::Done(_)));
    r.tap(Key::Enter);
    let Page::Failed(m) = r.page() else {
        panic!("{:?}", r.page())
    };
    assert!(m.contains("can't be opened"), "{m}");
    assert_eq!(r.core.states.get(), AppState::FirstRun);

    let mut r = rig();
    r.tap(Key::Enter);
    for _ in 0..RESULT_FRAMES + 1 {
        r.frame();
    }
    assert!(matches!(r.page(), Page::Failed(_)), "{:?}", r.page());
}

#[test]
fn a_good_import_goes_on_into_the_normal_start_up() {
    let Some(install) = std::env::var_os("TIDEWATER_INSTALL") else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let store = Store(Arc::new(Desktop::new(
        install,
        dir.path().join("Save"),
        dir.path().join("settings.txt"),
        dir.path().to_path_buf(),
    )));
    let fake = Arc::new(Fake::default());
    let list = vec![candidate("Warcraft II: Battle.net Edition (GOG)", "/games/one")];
    let mut core = crate::host::first_run_core(args(dir.path()), store, fake, Some(list), None);
    let mut w = Window::default();
    core.advance(Duration::from_millis(16), &mut w);
    for pressed in [true, false] {
        core.input.key(KeyEvent {
            key: Key::Enter,
            pressed,
            text: None,
        });
        core.advance(Duration::from_millis(16), &mut w);
    }
    // The result is up; Enter goes on.
    for pressed in [true, false] {
        core.input.key(KeyEvent {
            key: Key::Enter,
            pressed,
            text: None,
        });
        core.advance(Duration::from_millis(16), &mut w);
    }
    assert!(core.first_run.is_none(), "the first-run screen is gone");
    for _ in 0..3 {
        core.advance(Duration::from_millis(16), &mut w);
    }
    assert!(
        matches!(
            core.states.get(),
            AppState::Video | AppState::Title | AppState::MainMenu
        ),
        "{:?}",
        core.states.get()
    );
}

#[test]
fn the_automatic_search_runs_on_a_thread_and_fills_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        found: vec![candidate("Warcraft II: Battle.net Edition (GOG)", "/games/found")],
        ..Fake::default()
    });
    let mut core = crate::host::first_run_core(args(dir.path()), store(dir.path()), fake, None, None);
    let mut w = Window::default();
    for _ in 0..1000 {
        core.advance(Duration::from_millis(16), &mut w);
        if !core.first_run.as_ref().unwrap().searching {
            break;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    let f = core.first_run.as_ref().unwrap();
    assert!(!f.searching);
    assert_eq!(f.candidates.len(), 1);
    assert_eq!(f.focused(), Some(Item::Candidate(0)));
}

#[test]
fn with_no_installs_found_the_focus_starts_on_choose_game_folder() {
    let s = FirstRun::with_candidates(Arc::new(Fake::default()), Vec::new(), false);
    assert_eq!(s.focused(), Some(Item::Folder));
}

#[test]
fn a_long_list_scrolls_with_the_focus() {
    let list: Vec<Candidate> = (0..7)
        .map(|i| candidate("Warcraft II", format!("/games/{i}")))
        .collect();
    let mut s = FirstRun::with_candidates(Arc::new(Fake::default()), list, false);
    for _ in 0..5 {
        s.key(Key::Down, false);
    }
    assert_eq!(s.focused(), Some(Item::Candidate(5)));
    assert_eq!(s.first, 2);
    let shown: Vec<Item> = s.layout().into_iter().map(|(i, _)| i).collect();
    assert!(shown.contains(&Item::Candidate(5)) && !shown.contains(&Item::Candidate(0)));
    // Up past the first goes to the last button, and back to the top of the list shows the first again.
    s.key(Key::Up, false);
    s.key(Key::Up, false);
    s.key(Key::Up, false);
    s.key(Key::Up, false);
    s.key(Key::Up, false);
    assert_eq!(s.focused(), Some(Item::Candidate(0)));
    assert_eq!(s.first, 0);
}

#[test]
fn without_a_window_a_headless_or_scripted_run_keeps_the_message_and_exits() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Args::parse_from([
        "tidewater",
        "--data-dir",
        &dir.path().to_string_lossy(),
        "--script",
        "flow",
        "--script-out",
        &dir.path().join("out").to_string_lossy(),
        "--headless",
    ]);
    a.install = None;
    let err = crate::host::launch(a).err().expect("no game files");
    assert!(err.to_string().contains("tidewater --import"), "{err}");
}

#[test]
fn the_first_run_scene_needs_no_game_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Args::parse_from([
        "tidewater",
        "--data-dir",
        &dir.path().join("data").to_string_lossy(),
        "--script",
        crate::script::FIRST_RUN,
        "--script-out",
        &dir.path().join("out").to_string_lossy(),
        "--headless",
    ]);
    a.install = None;
    let l = crate::host::launch(a).expect("the scene starts");
    assert!(l.headless);
    assert_eq!(l.core.states.get(), AppState::Boot, "the first frame enters the screen");
    assert!(l.core.first_run.is_some());
    crate::host::run_headless(l.core);
    let text = std::fs::read_to_string(dir.path().join("out/firstrun.txt")).expect("the scene's fingerprints");
    assert!(text.contains("shot\t") && text.contains("\terror\t"));
}

/// The screen as a phone shows it: the shared folder, no installer, no Quit.
fn phone_rig() -> Rig {
    let mut r = rig();
    let fr = r.core.first_run.take().unwrap().with_intake(Intake::SharedFolder);
    r.core.first_run = Some(fr);
    r.frame();
    r
}

#[test]
fn a_phone_offers_the_folder_the_disc_and_a_rescan_but_no_installer_or_quit() {
    let mut r = phone_rig();
    let items: Vec<Item> = r.screen().items();
    assert_eq!(
        items,
        [
            Item::Candidate(0),
            Item::Candidate(1),
            Item::Folder,
            Item::Disc,
            Item::Rescan
        ]
    );
    // Escape does not quit a phone's program.
    r.tap(Key::Escape);
    assert!(!r.core.exit);
    assert_eq!(r.page(), Page::List);
    // The explanation says an installer can't be used.
    assert!(PHONE_INTRO.contains("GOG installer"));
    assert!(wrap(PHONE_INTRO, 74).len() <= 4, "the intro fits above the list");
    assert!(ANDROID_INTRO.contains("GOG installer"));
    assert!(
        wrap(ANDROID_INTRO, 74).len() <= 4,
        "the Android intro fits above the list"
    );
}

#[test]
fn scanning_again_looks_in_the_folder_anew() {
    let mut r = phone_rig();
    r.wait_for(|r| !r.screen().searching);
    let before = r.fake.searches.load(Ordering::SeqCst);
    r.point_at(Item::Rescan);
    r.frame();
    r.core.input.button(Button::Left, true);
    r.frame();
    r.core.input.button(Button::Left, false);
    // The search runs on a thread and the fake answers at once, so it may already be over by now: wait for it to end
    // rather than catching it running.
    r.wait_for(|r| !r.screen().searching);
    assert_eq!(
        r.fake.searches.load(Ordering::SeqCst),
        before + 1,
        "the search runs again"
    );
    assert_eq!(r.screen().candidates.len(), 2, "the fake finds the same two");
}

#[test]
fn suspending_and_resuming_flip_the_core_once() {
    let mut r = rig();
    assert!(!r.core.suspended);
    r.core.suspend();
    assert!(r.core.suspended);
    r.core.suspend();
    r.core.resume();
    assert!(!r.core.suspended);
}

#[test]
fn opened_from_the_settings_screen_esc_and_the_last_button_go_back_to_the_menu() {
    let mut r = rig();
    let fr = r.core.first_run.take().unwrap().returning();
    r.core.first_run = Some(fr);
    r.frame();
    // Esc leaves, with nothing imported and the program running on.
    let fr = r.core.first_run.as_mut().unwrap();
    assert_eq!(fr.key(Key::Escape, false), Some(Outcome::Back));
    // The last button says "Back" and does the same.
    assert_eq!(fr.activate(Item::Quit), Some(Outcome::Back));
    assert!(fr.items().contains(&Item::Quit));
    // Without the flag Esc and the button still quit, as at start-up.
    let mut plain = rig();
    let fr = plain.core.first_run.as_mut().unwrap();
    assert_eq!(fr.key(Key::Escape, false), Some(Outcome::Quit));
    assert_eq!(fr.activate(Item::Quit), Some(Outcome::Quit));
}

#[test]
fn a_phone_that_came_from_the_settings_screen_has_a_way_back() {
    let mut r = phone_rig();
    let fr = r.core.first_run.take().unwrap().returning();
    r.core.first_run = Some(fr);
    r.frame();
    let items = r.screen().items();
    assert_eq!(items.last(), Some(&Item::Quit), "a Back button after the rescan");
    let fr = r.core.first_run.as_mut().unwrap();
    assert_eq!(fr.key(Key::Escape, false), Some(Outcome::Back));
}

#[test]
fn the_step_line_counts_three_steps_when_the_import_unpacks_first() {
    assert_eq!(step_line(None, false), "Getting ready");
    // A CD image or an installer: read or unpack, check, copy.
    assert_eq!(
        step_line(Some(Step::ReadingDisc), false),
        "Step 1 of 3: Reading the CD image"
    );
    assert_eq!(
        step_line(Some(Step::Unpacking), false),
        "Step 1 of 3: Unpacking the installer"
    );
    assert_eq!(step_line(Some(Step::Checking), true), "Step 2 of 3: Checking the files");
    assert_eq!(step_line(Some(Step::Copying), true), "Step 3 of 3: Copying the files");
    // A game folder: check, copy.
    assert_eq!(
        step_line(Some(Step::Checking), false),
        "Step 1 of 2: Checking the files"
    );
    assert_eq!(step_line(Some(Step::Copying), false), "Step 2 of 2: Copying the files");
    // Every line fits the screen at the size it is drawn (with the three dots).
    for step in [
        None,
        Some(Step::ReadingDisc),
        Some(Step::Unpacking),
        Some(Step::Checking),
        Some(Step::Copying),
    ] {
        for unpacked in [false, true] {
            let w = (step_line(step, unpacked).chars().count() + 3) as i32 * font::SIZE as i32 * 2;
            assert!(w <= W, "{step:?}");
        }
        assert!(step_note(step).chars().count() <= 76, "{step:?}");
    }
}

/// The importing page's step, whether it unpacked, bytes and file.
fn importing(s: &FirstRun) -> (Option<Step>, bool, u64, u64, String) {
    match &s.page {
        Page::Importing {
            step,
            unpacked,
            done,
            total,
            file,
            ..
        } => (*step, *unpacked, *done, *total, file.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn each_report_moves_the_page_to_its_step() {
    let r = rig();
    let mut s = FirstRun::with_candidates(r.fake.clone(), Vec::new(), true);
    s.page = Page::Importing {
        name: "Game.cue".into(),
        step: None,
        unpacked: false,
        done: 0,
        total: 0,
        file: String::new(),
    };
    let (tx, rx) = channel();
    s.rx = Some(rx);
    let send = |step, done, total, file: &str| {
        tx.send(Msg::Progress {
            step,
            done,
            total,
            file: file.into(),
        })
        .unwrap();
    };
    send(Step::ReadingDisc, 120, 490, "Install.mpq");
    s.poll();
    assert_eq!(
        importing(&s),
        (Some(Step::ReadingDisc), true, 120, 490, "Install.mpq".into())
    );
    send(Step::Checking, 0, 0, "");
    s.poll();
    assert_eq!(importing(&s), (Some(Step::Checking), true, 0, 0, String::new()));
    send(Step::Copying, 7, 490, "Install.mpq");
    s.poll();
    let (step, unpacked, ..) = importing(&s);
    assert_eq!(step_line(step, unpacked), "Step 3 of 3: Copying the files");
}

#[test]
fn the_worker_passes_on_every_step_change() {
    let r = rig();
    let mut s = FirstRun::with_candidates(r.fake.clone(), Vec::new(), false);
    s.start_import(PathBuf::from("/downloads/Game.cue"));
    // Without threads the import ran inside the call; its messages wait in the channel, in order.
    let mut steps = Vec::new();
    while let Ok(m) = s.rx.as_ref().unwrap().try_recv() {
        if let Msg::Progress { step, done, total, .. } = m {
            steps.push((step, done, total));
        }
    }
    assert_eq!(
        steps,
        [
            (Step::ReadingDisc, 0, 200),
            (Step::ReadingDisc, 200, 200),
            (Step::Checking, 0, 0),
            (Step::Copying, 50, 100),
            (Step::Copying, 100, 100),
        ]
    );
}

#[test]
fn a_step_with_no_size_moves_a_block_along_the_bar() {
    let r = rig();
    let mut s = FirstRun::with_candidates(r.fake.clone(), Vec::new(), true);
    s.page = Page::Importing {
        name: "x".into(),
        step: Some(Step::Checking),
        unpacked: false,
        done: 0,
        total: 0,
        file: String::new(),
    };
    // The leftmost lit column of the bar's middle row.
    let block = |s: &mut FirstRun| {
        let mut pic = crate::frontend::blank();
        assert!(s.draw(&mut pic), "the page redrew");
        let y = 244usize;
        (120..520).find(|&x| pic.rgba[(y * 640 + x) * 4..][..3] == BAR)
    };
    let first = block(&mut s);
    assert!(first.is_some(), "a block is drawn");
    // With no report at all, the page asks to be drawn again within a few frames, and the block has moved.
    for _ in 0..ANIMATION_FRAMES {
        s.poll();
    }
    let later = block(&mut s);
    assert_ne!(first, later);
    assert!(later.is_some());
}
