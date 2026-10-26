//! The first-run screen (`docs/design/import.md`, "First-run screen", phase I2): what the program shows when it has no
//! game files, neither `--install` nor an earlier import. It is not part of the original; it exists only because the
//! game data isn't there yet, so the original's screens are untouched.
//!
//! It draws into the usual 640 × 480 frame with its own small bitmap font ([`font`]: no game font exists before the
//! import) and plain rectangles, and offers:
//! - the installs found automatically ([`search`]), each already classified and validated by `tidewater_import`;
//! - "Choose game folder...", "Choose GOG installer..." and "Choose CD image...", which ask the shell for the system's
//!   folder or file dialog ([`PickKind`], [`crate::core::Core::take_pick_requests`]) and take the answer back as an
//!   input event ([`crate::input::FileEvent::Picked`]);
//! - "Quit"; and a hint that files can be dropped on the window ([`crate::input::FileEvent::Dropped`]).
//!
//! On a phone ([`Intake::SharedFolder`]) the screen reads the app's shared folder instead (files copied in with the
//! Files app or a computer; on Android, the app's external files folder), says a GOG installer can't be used there, offers "Scan again" and has no "Quit".
//!
//! The import runs on a worker thread; its progress comes back over a channel. The importing page names the step
//! ("Step 1 of 3: Reading the CD image", [`tidewater_import::Step`]), fills a bar for that step with the megabytes
//! done, and moves a block along the bar for a step with no size to measure (the check), so it never looks stopped. A finished
//! import shows its result line for a moment and the program goes on into the normal start-up; a failed one shows the
//! import crate's message (written for the player) with a way back to the list. Mouse and keyboard both work: Up,
//! Down and Tab move, Enter chooses, Esc goes back or quits.
//!
//! The work behind the screen is a [`Backend`], so the rules are tested with a fake one (and the scripted scene
//! `firstrun` uses [`DemoBackend`]); the real one is [`DesktopBackend`].

pub mod font;
pub mod search;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use crate::core::Core;
use crate::frontend::AppState;
use crate::input::{FileEvent, Key};
use crate::layers::{Layer, PictureId, Plane, Role};
use crate::scene::Picture;
use tidewater_import::{Progress, Step};

const W: i32 = 640;
const H: i32 = 480;

/// An install found on the computer that can be imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The folder (or file) to import from.
    pub path: PathBuf,
    /// Which game this is, as `tidewater_import` guessed it ("Warcraft II: Battle.net Edition (GOG)").
    pub edition: String,
    /// The videos and music archive (`Install.mpq`) is there.
    pub extras: bool,
    /// The number of multiplayer maps.
    pub maps: usize,
}

impl Candidate {
    /// "Videos and music: yes. Maps: 153."
    pub fn details(&self) -> String {
        format!(
            "Videos and music: {}. Maps: {}.",
            if self.extras { "yes" } else { "no" },
            self.maps
        )
    }

    /// The line shown when an import has finished: "Imported Warcraft II: Battle.net Edition (GOG). Videos and music:
    /// yes. Maps: 153."
    pub fn result_line(&self) -> String {
        format!("Imported {}. {}", self.edition, self.details())
    }
}

/// What the screen needs done outside itself. Both calls run on worker threads.
pub trait Backend: Send + Sync + 'static {
    /// The installs found in the usual places that can be imported.
    fn search(&self) -> Vec<Candidate>;
    /// Imports from `path` (a folder, an installer, a CD image or an archive): `progress` hears each step as it goes
    /// (reading a CD image or unpacking an installer, checking, copying). The error is the message for the player.
    fn import(&self, path: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Candidate, String>;
}

/// The real backend: `tidewater_import`, copying into `<data_dir>/game`.
pub struct DesktopBackend {
    pub data_dir: PathBuf,
}

impl Backend for DesktopBackend {
    fn search(&self) -> Vec<Candidate> {
        let dirs = match Intake::native() {
            Intake::Dialogs => search::candidate_dirs(&search::bases(search::Os::current(), &search::Where::here())),
            Intake::SharedFolder => search::shared_folder_candidates(&search::Where::here()),
        };
        let mut roots: Vec<PathBuf> = Vec::new();
        let mut found = Vec::new();
        for d in dirs {
            let Ok(source) = tidewater_import::classify(&d) else {
                continue;
            };
            if roots.contains(&source.root) {
                continue;
            }
            let report = tidewater_import::validate(&source);
            if !report.is_ok() {
                continue;
            }
            roots.push(source.root.clone());
            found.push(Candidate {
                path: d,
                edition: report.edition.clone(),
                extras: report.has_archive("Install.mpq"),
                maps: report.map_count,
            });
        }
        found
    }

    fn import(&self, path: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Candidate, String> {
        let source = tidewater_import::classify_with(path, progress).map_err(|e| describe(&e))?;
        progress(&Progress {
            step: Step::Checking,
            file: "",
            done: 0,
            total: 0,
        });
        let report = tidewater_import::validate(&source)
            .into_result()
            .map_err(|e| describe(&e))?;
        let manifest =
            tidewater_import::import(&source, &report, &self.data_dir, progress).map_err(|e| describe(&e))?;
        Ok(Candidate {
            path: path.to_path_buf(),
            edition: manifest.edition,
            extras: report.has_archive("Install.mpq"),
            maps: report.map_count,
        })
    }
}

/// The message for the player. On a phone an installer can't be opened (the program that unpacks it can't run
/// there), which the import crate's message about installing that program would not explain.
fn describe(e: &tidewater_import::ImportError) -> String {
    if Intake::native() == Intake::SharedFolder && matches!(e, tidewater_import::ImportError::InnoextractMissing(_)) {
        format!(
            "A GOG installer can't be opened on {}. On a computer, install the game or unpack the installer, then \
             copy the game folder here; or use a CD image.",
            Intake::phone_name()
        )
    } else {
        e.to_string()
    }
}

/// A backend that copies nothing, for the scripted scene `firstrun`: a path with "bad" in it fails with one of the
/// import crate's messages, any other one succeeds at once.
pub struct DemoBackend;

impl DemoBackend {
    /// The installs the scene lists.
    pub fn candidates() -> Vec<Candidate> {
        vec![
            Candidate {
                path: PathBuf::from("/games/gog/Warcraft II BNE"),
                edition: "Warcraft II: Battle.net Edition (GOG)".into(),
                extras: true,
                maps: 153,
            },
            Candidate {
                path: PathBuf::from("/home/<user>/.wine/drive_c/Program Files/Warcraft II Battle.net Edition"),
                edition: "Warcraft II: Battle.net Edition".into(),
                extras: false,
                maps: 92,
            },
            Candidate {
                path: PathBuf::from("/Applications/Warcraft II.app"),
                edition: "Warcraft II: Battle.net Edition".into(),
                extras: true,
                maps: 92,
            },
        ]
    }
}

impl Backend for DemoBackend {
    fn search(&self) -> Vec<Candidate> {
        Self::candidates()
    }

    fn import(&self, path: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Candidate, String> {
        if path.to_string_lossy().contains("bad") {
            return Err(
                "Found Install.mpq but not War2Dat.mpq. If this is the CD, choose the folder where the game was \
                        installed, not the disc."
                    .into(),
            );
        }
        progress(&Progress {
            step: Step::Copying,
            file: "War2Dat.mpq",
            done: 1,
            total: 1,
        });
        Ok(Self::candidates().remove(0))
    }
}

/// How the game files reach the program.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Intake {
    /// Anywhere on the computer: the system's dialogs, files dropped on the window, a search of the usual places.
    #[default]
    Dialogs,
    /// A phone: the app's own shared folder (that the Files app and a computer can fill) and the system's document
    /// picker. No installer (it needs a program a phone can't run), no files dropped on the window and no "Quit".
    SharedFolder,
}

impl Intake {
    /// The way this build gets its game files.
    pub fn native() -> Intake {
        if cfg!(any(target_os = "ios", target_os = "android", target_os = "vita")) {
            Intake::SharedFolder
        } else {
            Intake::Dialogs
        }
    }

    /// The phone system's name, for the screen's messages.
    fn phone_name() -> &'static str {
        if cfg!(target_os = "android") {
            "Android"
        } else if cfg!(target_os = "vita") {
            "the PS Vita"
        } else {
            "iOS"
        }
    }
}

/// Which system dialog the screen asks the shell for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    /// A folder (the game's).
    Folder,
    /// A file: the GOG installer.
    Installer,
    /// A file: a CD image.
    Disc,
}

/// Something that can be chosen on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Candidate(usize),
    Folder,
    Installer,
    Disc,
    /// Search the shared folder again ([`Intake::SharedFolder`]).
    Rescan,
    Quit,
    /// The way back from an error.
    Back,
}

/// A rectangle in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn contains(&self, p: (i32, i32)) -> bool {
        p.0 >= self.x && p.0 < self.x + self.w && p.1 >= self.y && p.1 < self.y + self.h
    }

    fn centre(&self) -> (f32, f32) {
        (self.x as f32 + self.w as f32 / 2.0, self.y as f32 + self.h as f32 / 2.0)
    }
}

/// How many candidates show at once.
const VISIBLE: usize = 4;
const ROW_H: i32 = 28;
const LIST_X: i32 = 20;
const LIST_Y: i32 = 176;
const LIST_W: i32 = 600;
const BUTTON_X: i32 = 130;
const BUTTON_W: i32 = 380;
const BUTTON_H: i32 = 28;
const BUTTON_GAP: i32 = 6;
const BUTTONS_Y: i32 = 300;
/// Frames the result line stays up before the program goes on (about 1.5 s).
const RESULT_FRAMES: u32 = 90;
/// Frames between redraws of the importing page's animation.
const ANIMATION_FRAMES: u32 = 2;
/// Frames each count of the working dots stays up.
const DOT_FRAMES: u32 = 20;
/// The width of the block that slides along the bar for a step with no size, and how far it moves each redraw.
const SLIDER_W: i32 = 80;
const SLIDER_STEP: i32 = 8;

const DESKTOP_INTRO: &str = "Tidewater needs the game files from your own copy of Warcraft II: Battle.net Edition. \
                    Pick the install below, or choose where it is. Tidewater copies the files once.";
const PHONE_INTRO: &str = "Tidewater needs the game files from your own copy of Warcraft II: Battle.net Edition. \
                    Put the game folder or a CD image (.iso .bin .cue .mdf .nrg) in Tidewater's folder with the Files \
                    app or a computer, then tap Scan again. A GOG installer (.exe) can't be used on iOS.";

const ANDROID_INTRO: &str = "Tidewater needs the game files from your own copy of Warcraft II: Battle.net Edition. \
                    Choose the game folder or a CD image (.iso .bin .cue .mdf .nrg) below, or put it in Tidewater's \
                    folder from a computer, then tap Scan again. A GOG installer (.exe) can't be used on Android.";

const VITA_INTRO: &str = "Tidewater needs the game files from your own copy of Warcraft II: Battle.net Edition. \
                    Copy the game folder or a CD image (.iso .bin .cue .mdf .nrg) to ux0:data/Tidewater with VitaShell \
                    or FTP, then choose Scan again. A GOG installer (.exe) can't be used on the PS Vita.";

/// The introduction on a phone.
fn phone_intro() -> &'static str {
    if cfg!(target_os = "android") {
        ANDROID_INTRO
    } else if cfg!(target_os = "vita") {
        VITA_INTRO
    } else {
        PHONE_INTRO
    }
}

/// What the screen is showing.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Page {
    List,
    Importing {
        name: String,
        /// The step the import is on; none before its first report.
        step: Option<Step>,
        /// The import began by reading a CD image or unpacking an installer, so it has three steps, not two.
        unpacked: bool,
        /// The step's bytes done of its total (0 for a step with no size).
        done: u64,
        total: u64,
        file: String,
    },
    Done(String),
    Failed(String),
}

/// What a worker thread sends back.
enum Msg {
    Found(Vec<Candidate>),
    Progress {
        step: Step,
        done: u64,
        total: u64,
        file: String,
    },
    Imported(Result<Candidate, String>),
}

/// What the screen asks of the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Quit,
    /// The import has finished: go on into the normal start-up.
    Continue,
    /// Opened from the Settings screen: leave and go back to the main menu, the files as they were.
    Back,
}

/// The first-run screen's state.
pub struct FirstRun {
    backend: Arc<dyn Backend>,
    /// Whether the work runs on threads (it always does outside tests and scripted scenes, which need their frames to
    /// come out the same every time).
    threads: bool,
    candidates: Vec<Candidate>,
    searching: bool,
    page: Page,
    /// The focused item's place in [`FirstRun::items`].
    focus: usize,
    /// The first candidate in view.
    first: usize,
    /// A system dialog is open: the screen waits for its answer.
    picking: bool,
    requests: Vec<PickKind>,
    rx: Option<Receiver<Msg>>,
    last_pointer: Option<(i32, i32)>,
    intake: Intake,
    /// Frames the result has been up.
    result_frames: u32,
    /// Frames the importing page has been up, for its animation.
    tick: u32,
    dirty: bool,
    picture: Option<PictureId>,
    /// Opened from the Settings screen on a running program rather than at start-up: "Quit" and Esc go back to the main
    /// menu instead of ending the program, and a finished import goes back there too.
    returning: bool,
}

impl FirstRun {
    /// The screen over `backend`, searching the usual places on a thread.
    pub fn searching(backend: Arc<dyn Backend>) -> FirstRun {
        let mut f = FirstRun::blank(backend, true);
        f.start_search();
        f
    }

    /// Looks in the usual places on a thread; the list fills in when it is done.
    fn start_search(&mut self) {
        self.searching = true;
        self.dirty = true;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let b = self.backend.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Found(b.search()));
        });
    }

    /// The screen as the Settings screen's "Import game files again..." opens it: Esc and the last button ("Back")
    /// return to the main menu with the files as they were, on a phone too.
    pub fn returning(mut self) -> FirstRun {
        self.returning = true;
        self.dirty = true;
        self
    }

    /// The screen for a way of getting the files other than the desktop's ([`Intake`]).
    pub fn with_intake(mut self, intake: Intake) -> FirstRun {
        self.intake = intake;
        self.dirty = true;
        self
    }

    /// The screen with the installs `candidates` already listed (no search). `threads`: the copy runs on a thread; if
    /// not, it runs inside the frame that starts it.
    pub fn with_candidates(backend: Arc<dyn Backend>, candidates: Vec<Candidate>, threads: bool) -> FirstRun {
        let mut f = FirstRun::blank(backend, threads);
        f.candidates = candidates;
        f.focus = f.default_focus();
        f
    }

    fn blank(backend: Arc<dyn Backend>, threads: bool) -> FirstRun {
        FirstRun {
            backend,
            threads,
            candidates: Vec::new(),
            searching: false,
            page: Page::List,
            focus: 0,
            first: 0,
            picking: false,
            intake: Intake::Dialogs,
            requests: Vec::new(),
            rx: None,
            last_pointer: None,
            result_frames: 0,
            tick: 0,
            dirty: true,
            picture: None,
            returning: false,
        }
    }

    /// The dialogs asked for since the last call.
    pub(crate) fn take_requests(&mut self) -> Vec<PickKind> {
        std::mem::take(&mut self.requests)
    }

    /// The first install if there is one, else "Choose game folder...".
    fn default_focus(&self) -> usize {
        0
    }

    /// The buttons under the list, top to bottom.
    fn buttons(&self) -> &'static [Item] {
        match (self.intake, self.returning) {
            // The Vita has no system dialogs: the files come through the shared folder only.
            (Intake::SharedFolder, false) if cfg!(target_os = "vita") => &[Item::Rescan],
            (Intake::SharedFolder, true) if cfg!(target_os = "vita") => &[Item::Rescan, Item::Quit],
            (Intake::Dialogs, _) => &[Item::Folder, Item::Installer, Item::Disc, Item::Quit],
            (Intake::SharedFolder, false) => &[Item::Folder, Item::Disc, Item::Rescan],
            (Intake::SharedFolder, true) => &[Item::Folder, Item::Disc, Item::Rescan, Item::Quit],
        }
    }

    /// The items that can be focused, in the order Tab visits them.
    fn items(&self) -> Vec<Item> {
        match self.page {
            Page::List => (0..self.candidates.len())
                .map(Item::Candidate)
                .chain(self.buttons().iter().copied())
                .collect(),
            Page::Failed(_) => vec![Item::Back],
            _ => Vec::new(),
        }
    }

    /// The focused item.
    pub fn focused(&self) -> Option<Item> {
        self.items().get(self.focus).copied()
    }

    /// The items drawn and where, for the pointer and the drawing alike.
    pub fn layout(&self) -> Vec<(Item, Rect)> {
        match self.page {
            Page::List => {
                let mut v = Vec::new();
                let end = (self.first + VISIBLE).min(self.candidates.len());
                for (row, i) in (self.first..end).enumerate() {
                    v.push((
                        Item::Candidate(i),
                        Rect {
                            x: LIST_X,
                            y: LIST_Y + row as i32 * ROW_H,
                            w: LIST_W,
                            h: ROW_H - 2,
                        },
                    ));
                }
                for (n, &item) in self.buttons().iter().enumerate() {
                    v.push((
                        item,
                        Rect {
                            x: BUTTON_X,
                            y: BUTTONS_Y + n as i32 * (BUTTON_H + BUTTON_GAP),
                            w: BUTTON_W,
                            h: BUTTON_H,
                        },
                    ));
                }
                v
            }
            Page::Failed(_) => vec![(
                Item::Back,
                Rect {
                    x: BUTTON_X,
                    y: 400,
                    w: BUTTON_W,
                    h: BUTTON_H,
                },
            )],
            _ => Vec::new(),
        }
    }

    /// The middle of an item on the screen, if it is drawn (for scripted scenes' pointer).
    pub fn centre_of(&self, item: Item) -> Option<(f32, f32)> {
        self.layout()
            .into_iter()
            .find(|(i, _)| *i == item)
            .map(|(_, r)| r.centre())
    }

    // -----------------------------------------------------------------------------------------------------------
    // Input

    /// A key pressed this frame.
    pub(crate) fn key(&mut self, key: Key, shift: bool) -> Option<Outcome> {
        if self.picking {
            return None;
        }
        match &self.page {
            Page::Importing { .. } => None,
            Page::Done(_) => {
                matches!(key, Key::Enter | Key::NumpadEnter | Key::Space | Key::Escape).then_some(Outcome::Continue)
            }
            Page::List | Page::Failed(_) => match key {
                Key::Down | Key::Right => self.step(1),
                Key::Up | Key::Left => self.step(-1),
                Key::Tab => self.step(if shift { -1 } else { 1 }),
                Key::Enter | Key::NumpadEnter | Key::Space => self.focused().and_then(|i| self.activate(i)),
                Key::Escape => match self.page {
                    Page::Failed(_) => self.activate(Item::Back),
                    _ if self.returning => Some(Outcome::Back),
                    // A phone's program doesn't quit.
                    _ if self.intake == Intake::SharedFolder => None,
                    _ => Some(Outcome::Quit),
                },
                _ => None,
            },
        }
    }

    fn step(&mut self, by: isize) -> Option<Outcome> {
        let n = self.items().len();
        if n > 0 {
            self.focus = (self.focus as isize + by).rem_euclid(n as isize) as usize;
            self.scroll_to_focus();
            self.dirty = true;
        }
        None
    }

    /// Keeps the focused candidate in view.
    fn scroll_to_focus(&mut self) {
        if let Some(Item::Candidate(i)) = self.focused() {
            if i < self.first {
                self.first = i;
            } else if i >= self.first + VISIBLE {
                self.first = i + 1 - VISIBLE;
            }
        }
    }

    /// The pointer at a screen pixel (`None` outside the window) and whether the left button went down this frame.
    pub(crate) fn pointer(&mut self, at: Option<(i32, i32)>, click: bool) -> Option<Outcome> {
        if self.picking {
            return None;
        }
        if matches!(self.page, Page::Done(_)) {
            return click.then_some(Outcome::Continue);
        }
        let moved = at != self.last_pointer;
        self.last_pointer = at;
        let p = at?;
        let hit = self.layout().into_iter().find(|(_, r)| r.contains(p)).map(|(i, _)| i);
        let item = hit?;
        if (moved || click)
            && let Some(n) = self.items().iter().position(|&i| i == item)
            && n != self.focus
        {
            self.focus = n;
            self.dirty = true;
        }
        if click { self.activate(item) } else { None }
    }

    /// A file or folder came in: dropped on the window, or chosen in a system dialog.
    pub(crate) fn file(&mut self, event: FileEvent) -> Option<Outcome> {
        match event {
            FileEvent::Dropped(path) => {
                if matches!(self.page, Page::List | Page::Failed(_)) {
                    self.start_import(path);
                }
            }
            FileEvent::Picked(path) => {
                self.picking = false;
                self.dirty = true;
                if let Some(path) = path
                    && matches!(self.page, Page::List | Page::Failed(_))
                {
                    self.start_import(path);
                }
            }
        }
        None
    }

    fn activate(&mut self, item: Item) -> Option<Outcome> {
        match item {
            Item::Candidate(i) => {
                if let Some(c) = self.candidates.get(i) {
                    self.start_import(c.path.clone());
                }
            }
            Item::Folder => self.ask(PickKind::Folder),
            Item::Installer => self.ask(PickKind::Installer),
            Item::Disc => self.ask(PickKind::Disc),
            Item::Rescan => {
                if !self.searching {
                    self.candidates.clear();
                    self.focus = self.default_focus();
                    self.first = 0;
                    self.start_search();
                }
            }
            Item::Quit => return Some(if self.returning { Outcome::Back } else { Outcome::Quit }),
            Item::Back => {
                self.page = Page::List;
                self.focus = self.default_focus();
                self.first = 0;
                self.dirty = true;
            }
        }
        None
    }

    fn ask(&mut self, kind: PickKind) {
        self.requests.push(kind);
        self.picking = true;
        self.dirty = true;
    }

    // -----------------------------------------------------------------------------------------------------------
    // The work

    fn start_import(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.page = Page::Importing {
            name,
            step: None,
            unpacked: false,
            done: 0,
            total: 0,
            file: String::new(),
        };
        self.dirty = true;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let backend = self.backend.clone();
        let work = move || {
            let mut last: Option<(Step, u64, String)> = None;
            let result = backend.import(&path, &mut |p| {
                // About a message per tenth of a percent, and one for each new step or file.
                let permille = p.done.saturating_mul(1000) / p.total.max(1);
                if last
                    .as_ref()
                    .is_none_or(|(step, at, file)| *step != p.step || *at != permille || file != p.file)
                {
                    last = Some((p.step, permille, p.file.to_string()));
                    let _ = tx.send(Msg::Progress {
                        step: p.step,
                        done: p.done,
                        total: p.total,
                        file: p.file.to_string(),
                    });
                }
            });
            let _ = tx.send(Msg::Imported(result));
        };
        if self.threads {
            std::thread::spawn(work);
        } else {
            work();
        }
    }

    /// One frame: takes what the worker thread has sent, and counts the result's time.
    pub(crate) fn poll(&mut self) -> Option<Outcome> {
        while let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Msg::Found(list)) => {
                    self.searching = false;
                    self.candidates = list;
                    self.focus = self.default_focus();
                    self.dirty = true;
                }
                Ok(Msg::Progress {
                    step,
                    done,
                    total,
                    file,
                }) => {
                    if let Page::Importing {
                        step: s,
                        unpacked,
                        done: d,
                        total: t,
                        file: f,
                        ..
                    } = &mut self.page
                    {
                        *unpacked |= matches!(step, Step::ReadingDisc | Step::Unpacking);
                        (*s, *d, *t, *f) = (Some(step), done, total, file);
                        self.dirty = true;
                    }
                }
                Ok(Msg::Imported(result)) => {
                    self.rx = None;
                    self.dirty = true;
                    self.page = match result {
                        Ok(c) => Page::Done(c.result_line()),
                        Err(message) => {
                            self.focus = 0;
                            Page::Failed(message)
                        }
                    };
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.dirty = true;
                    if self.searching {
                        self.searching = false;
                    } else if matches!(self.page, Page::Importing { .. }) {
                        self.focus = 0;
                        self.page = Page::Failed("The import stopped unexpectedly. Try again.".into());
                    }
                    break;
                }
            }
        }
        if matches!(self.page, Page::Importing { .. }) {
            // The working dots and the block of a step with no size move on whether or not a report came.
            self.tick = self.tick.wrapping_add(1);
            if self.tick.is_multiple_of(ANIMATION_FRAMES) {
                self.dirty = true;
            }
        }
        if matches!(self.page, Page::Done(_)) {
            self.result_frames += 1;
            if self.result_frames >= RESULT_FRAMES {
                return Some(Outcome::Continue);
            }
        }
        None
    }

    /// The import's files could not be opened after all: back to the list with the reason.
    pub(crate) fn fail(&mut self, message: String) {
        self.page = Page::Failed(message);
        self.focus = 0;
        self.result_frames = 0;
        self.dirty = true;
    }

    // -----------------------------------------------------------------------------------------------------------
    // Drawing

    /// Draws the screen into `pic` (640 × 480) if anything changed since the last time. Returns whether it drew.
    pub(crate) fn draw(&mut self, pic: &mut Picture) -> bool {
        if !self.dirty {
            return false;
        }
        self.dirty = false;
        let mut c = Canvas { pic };
        c.fill(Rect { x: 0, y: 0, w: W, h: H }, BG);
        c.text_centred(20, 4, TITLE, "TIDEWATER");
        c.text_centred(
            60,
            2,
            DIM,
            if self.returning {
                "Import the game files again"
            } else {
                "First-run setup"
            },
        );
        match &self.page {
            Page::List => self.draw_list(&mut c),
            Page::Importing {
                name,
                step,
                unpacked,
                done,
                total,
                file,
            } => {
                c.text_centred(130, 2, TEXT, "Importing the game files");
                c.text_centred(160, 1, DIM, &fit_middle(name, 76));
                // The dots are padded to a fixed width, so the centred line doesn't shift as they change.
                let dots = ".".repeat((self.tick / DOT_FRAMES % 4) as usize);
                c.text_centred(196, 2, TEXT, &format!("{}{dots:<3}", step_line(*step, *unpacked)));
                let bar = Rect {
                    x: 120,
                    y: 230,
                    w: 400,
                    h: 28,
                };
                c.fill(bar, PANEL);
                c.outline(bar, BORDER);
                let inner = bar.w - 4;
                if *total == 0 {
                    // No size to measure: a block slides to and fro.
                    let room = inner - SLIDER_W;
                    let t = (self.tick / ANIMATION_FRAMES) as i32 * SLIDER_STEP % (2 * room);
                    let x = if t < room { t } else { 2 * room - t };
                    c.fill(
                        Rect {
                            x: bar.x + 2 + x,
                            y: bar.y + 2,
                            w: SLIDER_W,
                            h: bar.h - 4,
                        },
                        BAR,
                    );
                } else {
                    let frac = (*done as f64 / *total as f64).min(1.0);
                    c.fill(
                        Rect {
                            x: bar.x + 2,
                            y: bar.y + 2,
                            w: (inner as f64 * frac) as i32,
                            h: bar.h - 4,
                        },
                        BAR,
                    );
                    c.text_centred(238, 2, TEXT, &format!("{}%", (frac * 100.0) as u32));
                    c.text_centred(
                        274,
                        1,
                        DIM,
                        &format!(
                            "{} of {}",
                            tidewater_import::size_text((*done).max(1)),
                            tidewater_import::size_text(*total)
                        ),
                    );
                }
                c.text_centred(292, 1, DIM, &fit_middle(file, 76));
                c.text_centred(330, 1, DIM, step_note(*step));
            }
            Page::Done(line) => {
                c.text_centred(130, 2, OK, "Done");
                for (n, l) in wrap(line, 38).iter().enumerate().take(8) {
                    c.text_centred(180 + n as i32 * 24, 2, TEXT, l);
                }
                c.text_centred(400, 1, DIM, "Starting Tidewater... (Enter to go on now)");
            }
            Page::Failed(message) => {
                c.text_centred(120, 2, BAD, "That did not work");
                for (n, l) in wrap(message, 38).iter().enumerate().take(10) {
                    c.text_centred(160 + n as i32 * 20, 2, TEXT, l);
                }
                self.draw_items(&mut c);
                c.text_centred(450, 1, DIM, "Enter or Esc: back to the list");
            }
        }
        true
    }

    fn draw_list(&self, c: &mut Canvas) {
        let phone = self.intake == Intake::SharedFolder;
        let text = if phone { phone_intro() } else { DESKTOP_INTRO };
        for (n, l) in wrap(text, 74).iter().enumerate() {
            c.text(LIST_X, 94 + n as i32 * 12, 1, TEXT, l);
        }
        let (looking, none, here) = if phone {
            (
                "Looking in Tidewater's folder...",
                "Nothing found in Tidewater's folder.",
                "Found in Tidewater's folder:",
            )
        } else {
            (
                "Looking for installs...",
                "No install found in the usual places.",
                "Found on this computer:",
            )
        };
        let heading = if self.searching {
            looking.to_string()
        } else if self.candidates.is_empty() {
            none.to_string()
        } else if self.candidates.len() > VISIBLE {
            format!(
                "{} ({}-{} of {}):",
                here.trim_end_matches(':'),
                self.first + 1,
                (self.first + VISIBLE).min(self.candidates.len()),
                self.candidates.len()
            )
        } else {
            here.to_string()
        };
        c.text(
            LIST_X,
            150,
            2,
            if self.candidates.is_empty() { DIM } else { TEXT },
            &heading,
        );
        self.draw_items(c);
        if phone {
            c.text_centred(
                436,
                1,
                TEXT,
                "Tidewater copies the files once. You can delete your copy afterwards.",
            );
        } else {
            c.text_centred(
                436,
                1,
                TEXT,
                "You can also drop a game folder, an installer or a CD image on this window.",
            );
            let hint = if self.returning {
                "Up/Down or Tab: move    Enter: choose    Esc: back, nothing changes"
            } else {
                "Up/Down or Tab: move    Enter: choose    Esc: quit"
            };
            c.text_centred(454, 1, DIM, hint);
        }
        if self.picking {
            c.text_centred(468, 1, BAR, "Waiting for the file dialog...");
        }
    }

    fn draw_items(&self, c: &mut Canvas) {
        let focused = self.focused();
        for (item, r) in self.layout() {
            let on = focused == Some(item);
            c.fill(r, if on { FOCUS } else { PANEL });
            c.outline(r, if on { TITLE } else { BORDER });
            match item {
                Item::Candidate(i) => {
                    let cand = &self.candidates[i];
                    // The edition at the left, what it has at the right, the folder below.
                    let cols = (r.w as usize - 16) / font::SIZE;
                    let details = cand.details();
                    let edition = fit_middle(&cand.edition, cols.saturating_sub(details.chars().count() + 2));
                    c.text(r.x + 8, r.y + 3, 1, TEXT, &edition);
                    let at = r.x + r.w - 8 - (details.chars().count() * font::SIZE) as i32;
                    c.text(at, r.y + 3, 1, DIM, &details);
                    c.text(
                        r.x + 8,
                        r.y + 14,
                        1,
                        DIM,
                        &fit_middle(&cand.path.display().to_string(), cols),
                    );
                }
                Item::Folder => c.text_in(r, 2, TEXT, "Choose game folder..."),
                Item::Installer => c.text_in(r, 2, TEXT, "Choose GOG installer..."),
                Item::Disc => c.text_in(r, 2, TEXT, "Choose CD image..."),
                Item::Rescan => c.text_in(r, 2, TEXT, "Scan again"),
                Item::Quit => c.text_in(r, 2, TEXT, if self.returning { "Back" } else { "Quit" }),
                Item::Back => c.text_in(r, 2, TEXT, "Back"),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The core's side

/// A frame of the first-run screen (`core::frame_systems`): the frame's input goes to the screen, its worker's
/// messages are taken, the picture is drawn and what the screen asked for is done.
pub(crate) fn run(c: &mut Core) {
    let Some(mut fr) = c.first_run.take() else { return };
    let mut outcome = None;
    for event in std::mem::take(&mut c.input.files) {
        outcome = outcome.or(fr.file(event));
    }
    let shift = c.input.keys.any_pressed([Key::ShiftLeft, Key::ShiftRight]);
    let pressed: Vec<Key> = c.input.keys.get_just_pressed().copied().collect();
    for k in pressed {
        outcome = outcome.or(fr.key(k, shift));
    }
    let at = c.input.window.zip(c.input.pointer).map(|(w, p)| {
        let (x, y) = w.letterbox().screen_point(p, w.factor);
        (x as i32, y as i32)
    });
    let click = c.input.mouse.just_pressed(crate::input::Button::Left);
    outcome = outcome.or(fr.pointer(at, click));
    outcome = outcome.or(fr.poll());
    // The picture is one flow layer of this screen; it goes with the screen's other layers when the state is left.
    let id = *fr.picture.get_or_insert_with(|| {
        let id = c.pictures.add(crate::frontend::blank());
        c.layers.add(
            Layer::new(id, Plane::Flow, 0.0),
            Role::Flow(0),
            Some(AppState::FirstRun),
        );
        id
    });
    if let Some(pic) = c.pictures.get_mut(id) {
        fr.draw(pic);
    }
    match outcome {
        Some(Outcome::Quit) => c.exit = true,
        Some(Outcome::Continue | Outcome::Back) => match go_on(c, fr.returning) {
            Ok(()) => return,
            Err(message) => fr.fail(message),
        },
        None => {}
    }
    c.first_run = Some(fr);
}

/// The import is in `<data>/game`: loads the game's screens and fonts and goes on into the normal start-up (or the
/// map or `--goto` screen the command line asked for). The message for the player if the files can't be used.
fn go_on(c: &mut Core, returning: bool) -> Result<(), String> {
    let install = crate::storage::open_install(&c.store.0)
        .map_err(|e| format!("The imported game files can't be opened: {e}."))?;
    let mut front =
        crate::frontend::Front::load(install).map_err(|e| format!("The imported game files can't be used: {e}."))?;
    // Opened from the Settings screen, it returns to the main menu; at start-up the usual start goes on.
    let steps = if returning {
        vec![crate::frontend::Step::MainMenu]
    } else {
        crate::host::start_steps(&c.opts, &c.stored.0, &mut c.flow, &mut front).map_err(|e| e.to_string())?
    };
    c.flow.steps = steps.into();
    c.front = front;
    c.front_screen.ready = false;
    c.states.set(AppState::Boot);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Drawing helpers

type Rgb = [u8; 3];

const BG: Rgb = [16, 20, 36];
const PANEL: Rgb = [28, 34, 58];
const FOCUS: Rgb = [48, 66, 120];
const BORDER: Rgb = [84, 96, 140];
const TEXT: Rgb = [232, 232, 240];
const DIM: Rgb = [146, 152, 176];
const TITLE: Rgb = [240, 200, 90];
const OK: Rgb = [120, 220, 140];
const BAD: Rgb = [240, 120, 110];
const BAR: Rgb = [90, 170, 240];

/// Draws into a 640 × 480 picture with rectangles and the embedded font.
struct Canvas<'a> {
    pic: &'a mut Picture,
}

impl Canvas<'_> {
    fn put(&mut self, x: i32, y: i32, color: Rgb) {
        if (0..W).contains(&x) && (0..H).contains(&y) {
            let i = ((y * W + x) * 4) as usize;
            self.pic.rgba[i..i + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
        }
    }

    fn fill(&mut self, r: Rect, color: Rgb) {
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                self.put(x, y, color);
            }
        }
    }

    fn outline(&mut self, r: Rect, color: Rgb) {
        self.fill(Rect { h: 1, ..r }, color);
        self.fill(
            Rect {
                y: r.y + r.h - 1,
                h: 1,
                ..r
            },
            color,
        );
        self.fill(Rect { w: 1, ..r }, color);
        self.fill(
            Rect {
                x: r.x + r.w - 1,
                w: 1,
                ..r
            },
            color,
        );
    }

    /// `s` with its top-left corner at (`x`, `y`), each font pixel `scale` screen pixels square.
    fn text(&mut self, x: i32, y: i32, scale: i32, color: Rgb, s: &str) {
        for (n, ch) in s.chars().enumerate() {
            let glyph = font::glyph(ch);
            let gx = x + n as i32 * font::SIZE as i32 * scale;
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..font::SIZE {
                    if bits >> col & 1 == 1 {
                        self.fill(
                            Rect {
                                x: gx + col as i32 * scale,
                                y: y + row as i32 * scale,
                                w: scale,
                                h: scale,
                            },
                            color,
                        );
                    }
                }
            }
        }
    }

    fn text_centred(&mut self, y: i32, scale: i32, color: Rgb, s: &str) {
        let w = s.chars().count() as i32 * font::SIZE as i32 * scale;
        self.text((W - w) / 2, y, scale, color, s);
    }

    /// `s` centred in `r`.
    fn text_in(&mut self, r: Rect, scale: i32, color: Rgb, s: &str) {
        let w = s.chars().count() as i32 * font::SIZE as i32 * scale;
        let h = font::SIZE as i32 * scale;
        self.text(r.x + (r.w - w) / 2, r.y + (r.h - h) / 2, scale, color, s);
    }
}

/// `s` broken into lines of at most `cols` characters at spaces; a word longer than a line is broken where it falls.
pub(crate) fn wrap(s: &str, cols: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let mut word = word.to_string();
        loop {
            let len = line.chars().count();
            let wlen = word.chars().count();
            let need = if line.is_empty() { wlen } else { len + 1 + wlen };
            if need <= cols {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&word);
                break;
            }
            if line.is_empty() {
                // Longer than a line on its own.
                let head: String = word.chars().take(cols).collect();
                word = word.chars().skip(cols).collect();
                lines.push(head);
            } else {
                lines.push(std::mem::take(&mut line));
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The importing page's step line: "Step 1 of 3: Reading the CD image". An import that reads a CD image or unpacks
/// an installer (`unpacked`) has three steps, any other two; before the first report it is getting ready.
pub(crate) fn step_line(step: Option<Step>, unpacked: bool) -> String {
    let (n, what) = match step {
        None => return "Getting ready".to_string(),
        Some(Step::ReadingDisc) => (1, "Reading the CD image"),
        Some(Step::Unpacking) => (1, "Unpacking the installer"),
        Some(Step::Checking) => (1, "Checking the files"),
        Some(Step::Copying) => (2, "Copying the files"),
    };
    let first = matches!(step, Some(Step::ReadingDisc | Step::Unpacking));
    if unpacked || first {
        format!("Step {} of 3: {what}", if first { 1 } else { n + 1 })
    } else {
        format!("Step {n} of 2: {what}")
    }
}

/// The line under the bar that says what the step does and how long it may take.
pub(crate) fn step_note(step: Option<Step>) -> &'static str {
    match step {
        None => "Looking at what you chose.",
        Some(Step::ReadingDisc) => "Reading the files out of the CD image. This can take a minute or two.",
        Some(Step::Unpacking) => "Unpacking with innoextract. This can take a few minutes.",
        Some(Step::Checking) => "Checking that the archives open and hold what the game needs.",
        Some(Step::Copying) => "Copying once, into Tidewater's own folder. Your files are not changed.",
    }
}

/// `s` cut to at most `cols` characters by replacing the middle with "...".
pub(crate) fn fit_middle(s: &str, cols: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= cols || cols < 8 {
        return s.chars().take(cols).collect();
    }
    let keep = cols - 3;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push_str("...");
    out.extend(&chars[chars.len() - tail..]);
    out
}

#[cfg(test)]
mod tests;
