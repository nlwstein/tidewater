//! The client core (`docs/design/client-core.md`, step 9): the whole program but the window, the sound device and the
//! files, with no engine types. The platform shell (the SDL 3 one, `shell/main.rs`) puts the system's input in
//! ([`Core::input`]), calls [`Core::advance`] once a frame with the real time since the last one, shows the frame it
//! returns, plays [`Core::audio`]'s output and gives the core its [`Storage`](crate::storage::Storage).
//!
//! A frame runs in the one order step 5 fixed (the design doc's "Time and system order" lists the pairs that share
//! data and why each is in its place):
//!
//! 1. The input's typed events and wheel steps age after a frame that ran a game cycle; the clock takes the frame's
//!    time (`clock`).
//! 2. A scripted run's input replaces the system's (`script`); the pads' and the fingers' events become mouse and
//!    keyboard input (`pad`, `touch`); the frame's input is applied; the pointer is placed (the platform's, after its
//!    mouse lock, or the scene's, then the fingers' or the pad's); the chat line takes its keys (`chat`).
//! 3. The flow's change of screen, if one was asked for: the leaving screen's hooks, its layers and sounds, the
//!    entering screen's hooks (`states`).
//! 4. The frame's game cycles (`clock`, `game::tick`).
//! 5. The frame's systems ([`frame_systems`]), with the points where their deferred changes ([`Commands`]) take effect.
//! 6. The map view's frame ends (`mapview`); the screen is composed ([`compose_screen`]); a Print Screen picture is
//!    written (`screenshot`); a scripted run notes the sounds started.
//!
//! The Bevy app (the shell until SDL 3 replaced it) ran the same steps as schedules (`First`, `PreUpdate`,
//! `StateTransition`, `RunFixedMainLoop`, `Update`, `PostUpdate`, `Last`); a system's deferred changes (Bevy's commands) took effect at the same points as
//! [`Commands`]' do here, and a group of systems with a run condition checked it once, before its first system, as
//! [`frame_systems`] does.

use std::time::Duration;

use crate::args::Args;
use crate::audio::Audio;
use crate::chat;
use crate::cheats;
pub use crate::chrome::Chrome;
use crate::clock::FrameClock;
use crate::compose::{Clip, Frame, Placed};
use crate::frontend::{self, AppState, Flow, Front, FrontScreen, OutcomeDialog};
use crate::game;
use crate::gamemenu::{self, GameMenu, Stored};
use crate::input::{Buttons, Input, Key, LogCursor, WindowGeom};
use crate::jukebox::{self, Jukebox};
use crate::layers::{Entry, Layer, PictureId, Pictures, Plane, Role, Stack};
use crate::mapview::MapView;
use crate::savegame::{GameClock, GameOrigin, PauseCounts, ResumeLook};
use crate::screenshot::{self, PendingShot};
use crate::script;
use crate::states::{self, Hook, Machine};
use crate::storage::Store;

/// Changes a system asks for that take effect later, at the frame's fixed points ([`Core::apply_commands`]), in the
/// order they were asked for: what Bevy's commands were.
#[derive(Default)]
pub struct Commands(Vec<Deferred>);

/// One deferred change.
type Deferred = Box<dyn FnOnce(&mut Core) + Send + Sync>;

impl Commands {
    pub fn queue(&mut self, f: impl FnOnce(&mut Core) + Send + Sync + 'static) {
        self.0.push(Box::new(f));
    }
}

/// Adds a layer when the commands take effect.
pub fn add_layer(commands: &mut Commands, layer: Layer, role: Role, owner: Option<AppState>) {
    commands.queue(move |c| c.layers.add(layer, role, owner));
}

/// Removes the layers `gone` picks, and their pictures, when the commands take effect.
pub fn remove_layers(commands: &mut Commands, gone: impl Fn(&Entry) -> bool + Send + Sync + 'static) {
    commands.queue(move |c| c.remove_layers_now(gone));
}

/// What the core asks the platform for during a frame.
pub trait Platform {
    /// The window: its size in physical pixels and its scale, `None` without one.
    fn window(&self) -> Option<WindowGeom>;
    /// The pointer in the window's logical points (`None` outside the window) and whether the window has the keyboard
    /// focus, once a frame after the frame's keys are applied. `game_pointer`: the game runs and draws its own
    /// pointer, so the platform may lock the system's (`display::lock_wanted`); `keys` are the keys held.
    fn pointer(&mut self, keys: &Buttons<Key>, game_pointer: bool) -> (Option<(f32, f32)>, bool);
    /// Whether there is no real window (a scripted run without one): the window it reports is then exactly the
    /// screen, whatever width the game gave it.
    fn headless(&self) -> bool {
        false
    }
}

/// The program's state.
pub struct Core {
    // The platform's side.
    /// The player's input; the platform puts the system's events in between frames.
    pub input: Input,
    /// The mixer; the platform plays its output.
    pub audio: Audio,
    pub time: FrameClock,
    pub store: Store,
    /// The command line.
    pub opts: Args,
    /// The frame the screen shows, composed at the end of [`Core::advance`].
    pub frame: Frame,
    /// Whether the system's pointer shows (the screen draws none of its own).
    pub cursor_visible: bool,
    /// The window title a game asks for.
    pub title: Option<String>,
    /// The program asked to end normally.
    pub exit: bool,
    /// The program is in the background ([`Core::suspend`]).
    pub suspended: bool,

    // The screen.
    pub pictures: Pictures,
    pub layers: Stack,
    pub front_screen: FrontScreen,
    /// The screen's size in pixels this frame: 640 x 480, or wider in a game with `widescreen` (`crate::widescreen`).
    pub screen: (u32, u32),

    // The flow.
    pub states: Machine,
    pub flow: Flow,
    pub front: Front,
    pub screens: frontend::Screens,
    pub dialog: Option<OutcomeDialog>,
    /// The game is being left, for the main menu or a restart: its fade of all sound runs (`frontend::run_quit`).
    pub quitting: Option<frontend::Leave>,
    pub jukebox: Jukebox,
    pub stored: Stored,
    /// The optional improvements for this run (`extras`): the settings file's, then the command line's. The C+L ones
    /// are decided per game by [`game::extras`].
    pub extras: crate::extras::ClientExtras,
    pub menu: Option<GameMenu>,
    pub menu_input: gamemenu::MenuInput,
    pub cheats: cheats::Queue,
    /// A game's world came in and the cheats haven't started its flags yet (`cheats::run`).
    pub sim_new: bool,
    pub local: chat::LocalSettings,
    /// The chat line's place in the typed events.
    pub chat_typed: LogCursor,

    // The game (`game`). Each is there from the game's start; the ones `game::end_game` doesn't drop stay until the
    // next game replaces them.
    pub loaded: Option<game::Loaded>,
    pub map_size: Option<game::MapSize>,
    pub sim: Option<game::Sim>,
    /// Where the starting game's commands come from, from `game::load_game` until `game::setup` gives it to the sim.
    pub next_session: Option<Box<dyn crate::session::Session>>,
    pub speaker: Option<game::Speaker>,
    pub panel_ui: Option<game::PanelUi>,
    pub screen_ui: Option<game::ScreenUi>,
    pub info_panel: Option<game::InfoPanel>,
    pub selection: Option<game::Selection>,
    pub terrain_art: Option<game::TerrainArt>,
    pub cycling: Option<game::Cycling>,
    pub art: Option<game::Art>,
    pub map_zoom: Option<game::MapZoom>,
    pub hud: Option<game::Hud>,
    pub view: Option<MapView>,
    pub origin: Option<GameOrigin>,
    pub resume_look: Option<ResumeLook>,
    pub game_clock: Option<GameClock>,
    pub pause_counts: Option<PauseCounts>,
    pub pending_shot: Option<PendingShot>,

    /// A scripted run (`script`).
    pub script: Option<script::Run>,
    pub commands: Commands,

    /// The first-run screen's state, while there are no game files (`firstrun`).
    pub first_run: Option<crate::firstrun::FirstRun>,
    /// Touch input: the gesture machine, the chrome, the emulation (`touch`).
    pub touch: crate::touch::TouchState,
    /// Gamepad input: the pad's pointer, buttons and sticks (`pad`).
    pub pad: crate::pad::PadState,
    /// The start-up check for a newer release, and the install (`update`).
    pub updater: crate::update::Updater,
    /// Addresses the core wants the system to open (the Settings screen's "Open settings folder", a phone's "Update
    /// now"); the shell opens each and clears the list ([`Core::take_open_requests`]).
    pub(crate) open_requests: Vec<String>,
}

impl Core {
    /// The core over `store`, with the flow's first steps and the stored settings in force.
    pub fn new(
        opts: Args,
        store: Store,
        front: Front,
        flow: Flow,
        stored: Stored,
        script: Option<script::Run>,
    ) -> Self {
        // The settings file's extras, then the command line's, then a scripted scene's own.
        let mut stored = stored;
        let mut extras = stored.0.extras;
        opts.extras.apply(&mut extras);
        if let Some(run) = &script
            && !run.script.scene.keys.is_empty()
        {
            // A scripted scene's own `[keys]` section (`keymap`).
            let lines: Vec<String> = run.script.scene.keys.lines().map(String::from).collect();
            (stored.0.keys, stored.0.key_problems) = crate::keymap::KeyMap::parse(&lines);
        }
        if let Some(run) = &script {
            crate::extras::ExtraFlags::from_pairs(run.script.scene.extras).apply(&mut extras);
        }
        let emulate = opts.touch_emulation;
        let mut audio = Audio::default();
        audio.set_trims(extras.trims());
        Core {
            input: Input::default(),
            audio,
            time: FrameClock::default(),
            store,
            opts,
            frame: Frame::black(),
            cursor_visible: true,
            title: None,
            exit: false,
            suspended: false,
            pictures: Pictures::default(),
            layers: Stack::default(),
            front_screen: FrontScreen::default(),
            screen: crate::display::SCREEN,
            states: Machine::default(),
            flow,
            front,
            screens: frontend::Screens::default(),
            dialog: None,
            quitting: None,
            jukebox: Jukebox::default(),
            stored,
            extras,
            menu: None,
            menu_input: gamemenu::MenuInput::default(),
            cheats: cheats::Queue::default(),
            sim_new: false,
            local: chat::LocalSettings::default(),
            chat_typed: LogCursor::default(),
            loaded: None,
            map_size: None,
            sim: None,
            next_session: None,
            speaker: None,
            panel_ui: None,
            screen_ui: None,
            info_panel: None,
            selection: None,
            terrain_art: None,
            cycling: None,
            art: None,
            map_zoom: None,
            hud: None,
            view: None,
            origin: None,
            resume_look: None,
            game_clock: None,
            pause_counts: None,
            pending_shot: None,
            script,
            commands: Commands::default(),
            first_run: None,
            touch: crate::touch::TouchState::new(emulate),
            pad: crate::pad::PadState::default(),
            updater: crate::update::Updater::default(),
            open_requests: Vec::new(),
        }
    }

    /// Whether the mouse lock may hold the pointer during a game: the `mouse_lock` setting, unless `--no-mouse-lock`
    /// says not to for this run, or the touch emulation, whose dragging finger the lock fights. The shell follows it
    /// every frame.
    pub fn mouse_lock_enabled(&self) -> bool {
        self.stored.0.mouse_lock && !self.opts.no_mouse_lock && !self.opts.touch_emulation
    }

    /// Whether the desktop window is to be full screen: the `fullscreen` setting. A phone's window always fills the
    /// screen and a scripted run's is fixed, so neither follows it. The shell follows it every frame.
    pub fn fullscreen_wanted(&self) -> bool {
        self.stored.0.fullscreen && !self.touch.touch_only && self.script.is_none()
    }

    /// The addresses the core asked the system to open since the last call (`file://` URLs of folders).
    pub fn take_open_requests(&mut self) -> Vec<String> {
        std::mem::take(&mut self.open_requests)
    }

    /// Asks the shell to open `url` in the system's own program.
    pub(crate) fn request_open(&mut self, url: String) {
        self.open_requests.push(url);
    }

    /// Starts the background check for a newer release, unless `--no-update-check`, the `update_check` setting, a
    /// development build or a scripted run says not to (`update`). The shell calls it once, at start-up.
    pub fn start_update_check(&self) {
        self.updater.start_check(
            self.opts.no_update_check,
            self.stored.0.update_check,
            self.script.is_some(),
        );
        let on = !self.opts.no_update_check && self.stored.0.update_check && self.script.is_none();
        self.updater.check_location(self.stored.0.move_notice_seen, on);
    }

    /// Runs one frame, `elapsed` real time after the last, and returns the screen it composed.
    pub fn advance(&mut self, elapsed: Duration, platform: &mut dyn Platform) -> &Frame {
        let c = self;
        // 1. Time.
        if c.time.logs_age() {
            c.input.update_logs();
        }
        c.time.advance(elapsed);
        // 2. Input.
        let headless = platform.headless();
        let mut window = platform.window();
        c.screen = screen_size(c, window);
        window = stamp_screen(window, c.screen, headless);
        if c.script.is_some() {
            script::drive(c, window);
        }
        crate::pad::step(c, elapsed, window);
        crate::touch::step(c, elapsed, window);
        c.input.begin_frame();
        match &c.script {
            Some(run) => {
                c.input.window = window;
                c.input.pointer = window.and(run.pointer());
                c.input.focused = true;
            }
            None => {
                let game_pointer = game_running(c) && c.hud.as_ref().is_some_and(|h| h.art.has_pointers());
                let (pointer, focused) = platform.pointer(&c.input.keys, game_pointer);
                c.input.window = window;
                c.input.pointer = pointer;
                c.input.focused = focused;
            }
        }
        if c.touch.active {
            let scripted = c.script.is_some();
            c.touch.place_pointer(&mut c.input.pointer, scripted);
        }
        c.pad.place_pointer(&mut c.input.pointer);
        if c.states.get() == AppState::Game {
            chat::chat_keys(c);
            game::net_keys(c);
        }
        // 3. The flow's change of screen.
        run_transitions(c);
        // A game starting or ending changes the screen's width.
        let screen = screen_size(c, window);
        if screen != c.screen {
            c.screen = screen;
            c.input.window = stamp_screen(c.input.window, screen, headless);
        }
        // 4. The game cycles.
        c.time.accumulate();
        while c.time.expend() {
            // A network game waiting for a turn does not spend the step (a short wait is caught up).
            if game_ticking(c) && !game::frozen_by_focus(c) && game::tick(c) {
                c.time.unspend();
                break;
            }
        }
        // 5. The frame's systems.
        frame_systems(c);
        // 6. The frame's end.
        if let Some(v) = &mut c.view {
            v.end_frame();
        }
        c.front_screen.ready = true;
        compose_screen(c);
        crate::touch::draw_pulse(c);
        screenshot::write_shot(c);
        if c.script.is_some() {
            script::note_sounds(c);
        }
        &c.frame
    }

    /// The deferred changes take effect, in the order they were asked for.
    pub fn apply_commands(&mut self) {
        while !self.commands.0.is_empty() {
            for f in std::mem::take(&mut self.commands.0) {
                f(self);
            }
        }
    }

    /// Removes the layers `gone` picks, and their pictures.
    pub fn remove_layers_now(&mut self, gone: impl Fn(&Entry) -> bool) {
        self.layers.remove(&mut self.pictures, gone);
    }

    /// The system dialogs the first-run screen has asked for since the last call; the shell shows each and gives the
    /// answer back with [`Input::picked`](crate::input::Input::picked).
    pub fn take_pick_requests(&mut self) -> Vec<crate::firstrun::PickKind> {
        self.first_run.as_mut().map(|f| f.take_requests()).unwrap_or_default()
    }

    /// The program was sent to the background (a phone's home gesture, a call): the game pauses as the pause key would,
    /// the mixer stops and the settings are written, as the system may end the program without another word
    /// (`docs/design/client-core.md`, "Lifecycle"). The shell runs one more frame so the pause takes effect, then stops
    /// calling [`Core::advance`] until [`Core::resume`].
    pub fn suspend(&mut self) {
        if self.suspended {
            return;
        }
        self.suspended = true;
        self.audio.set_suspended(true);
        game::pause_for_suspend(self);
        self.store_settings();
    }

    /// The program is in front again: the mixer restarts. A paused game stays paused until the player resumes it. Keys
    /// and fingers held when it left are let go.
    pub fn resume(&mut self) {
        if !self.suspended {
            return;
        }
        self.suspended = false;
        self.audio.set_suspended(false);
        self.input.focus_lost();
    }

    /// "Saving": the settings are written when the program exits normally, not on an error exit.
    pub fn store_settings(&self) {
        // Nothing has been set up yet on the first-run screen.
        if self.first_run.is_some() {
            return;
        }
        if let Err(e) = self.stored.0.store(&*self.store.0) {
            eprintln!("can't write the settings: {e}");
        }
    }
}

/// Whether the game screen runs (not while the outcome dialog or, in single player, a game menu is up, nor while the
/// game is being left for the main menu).
pub fn game_running(c: &Core) -> bool {
    c.states.get() == AppState::Game && c.dialog.is_none() && c.menu.is_none() && c.quitting.is_none()
}

/// Whether the game's cycles run this frame: whenever the game screen runs ([`game_running`]), and in a network game
/// also under the game menu and its dialogs (`ui/game-menu.md`, "Multiplayer": opening a menu doesn't stop the game,
/// only pause does), so the other players never wait for the one who opened it. The menu holder's game input stays off
/// meanwhile, as [`game_running`] is still false. Single player stops under the menu.
pub fn game_ticking(c: &Core) -> bool {
    let screen = c.states.get() == AppState::Game && c.dialog.is_none() && c.quitting.is_none();
    let network = c.sim.as_ref().is_some_and(|s| s.session.is_network());
    ticks_under(screen, c.menu.is_some(), network)
}

/// [`game_ticking`]'s rule: the game screen shows with no outcome dialog and isn't being left, and either no menu is
/// open or the game is a network one.
fn ticks_under(screen: bool, menu: bool, network: bool) -> bool {
    screen && (!menu || network)
}

fn in_game(c: &Core) -> bool {
    c.states.get() == AppState::Game
}

/// The flow's change of screen, if one was asked for (`states`): the leaving screen's hooks, its own layers and
/// sounds (the last made first), then the entering screen's hooks. Each hook's deferred changes take effect as it ends.
fn run_transitions(c: &mut Core) {
    let Some(t) = c.states.take_transition() else {
        return;
    };
    for &hook in states::on_exit(t.exited) {
        run_hook(c, hook);
    }
    c.layers.remove(&mut c.pictures, |e| e.owner == Some(t.exited));
    c.audio.stop_screen(t.exited);
    for &hook in states::on_enter(t.entered) {
        run_hook(c, hook);
    }
    frontend::choose_pointer(c);
}

fn run_hook(c: &mut Core, hook: Hook) {
    use Hook::*;
    let f: fn(&mut Core) = match hook {
        StopMusic => frontend::stop_music,
        CameraClear => frontend::camera_clear,
        StartGame => game::start_game,
        SetupGame => game::setup,
        EndGame => game::end_game,
        CloseDialog => frontend::close_dialog,
        CloseGameMenu => gamemenu::close_on_exit,
        CloseMenuLoad => frontend::close_menu_load,
        EnterVideo => frontend::enter_video,
        EnterTitle => frontend::enter_title,
        EnterMenu => frontend::enter_menu,
        EnterSinglePlayer => frontend::enter_single_player,
        EnterNewCampaign => frontend::enter_new_campaign,
        EnterCustomSetup => frontend::enter_custom_setup,
        EnterSettings => frontend::enter_settings,
        EnterMultiplayer => frontend::enter_multiplayer,
        EnterAct => frontend::enter_act,
        EnterBriefing => frontend::enter_briefing,
        EnterScore => frontend::enter_score,
        EnterFinale => frontend::enter_finale,
        EnterCredits => frontend::enter_credits,
    };
    f(c);
    c.apply_commands();
}

/// The frame's systems, in the one order they run (`docs/design/client-core.md`, step 5). It is the order Bevy's
/// schedule gave them, with the music before the flow's screens; the order of each pair that shares data is fixed (the
/// list of pairs and why each is in its place is in the design doc, "System order").
///
/// The flow's screens each run in their own state, so at most one of them runs in a frame. Input and game-time
/// systems stop while a menu or outcome dialog is open; drawing goes on, so the map and panels stay drawn under the
/// dialog. The game menu's systems come last, so a key that opens or closes a menu is seen by one side only.
fn frame_systems(c: &mut Core) {
    jukebox::play_music(c);
    frontend::apply_fades(c);
    // Leaving a game (for the main menu or a restart), the outcome dialog, then the check for a game's end.
    if c.quitting.is_some() {
        frontend::run_quit(c);
    }
    if c.dialog.is_some() && c.menu.is_none() {
        frontend::run_dialog(c);
    }
    if in_game(c) {
        frontend::watch_outcome(c);
    }
    frontend::screen_systems(c);
    if c.states.get() == AppState::FirstRun {
        crate::firstrun::run(c);
    }
    if in_game(c) {
        gamemenu::demo_save_load(c);
    }
    if in_game(c) {
        cheats::play_disco(c);
    }
    if in_game(c) {
        game::hud_layout(c);
        if game_running(c) {
            game::scroll_view(c);
        }
    }
    if in_game(c) {
        cheats::run(c);
    }
    if in_game(c) {
        game::stop_clock(c);
    }
    if in_game(c) {
        screenshot::print_screen(c);
    }
    if in_game(c) {
        if game_running(c) {
            if c.touch.active || c.pad.pause_pending() {
                game::touch_view(c);
            }
            game::panel_input(c);
            game::info_input(c);
            game::keys_input(c);
            game::mouse_input(c);
            game::pan_and_zoom(c);
        }
        // The cycles' sounds and messages, under a network game's menu too (`game_ticking`).
        if game_ticking(c) {
            game::play_sounds(c);
            c.apply_commands();
            game::update_messages(c);
        }
        if !game_running(c) && game_ticking(c) {
            // The game goes on under the Settings screen: its sounds and messages are not let pile up.
            game::play_sounds(c);
            c.apply_commands();
            game::update_messages(c);
        }
        game::cycle_palette(c);
        game::redraw_terrain(c);
        game::draw_view(c);
        game::draw_placement(c);
        game::update_info(c);
        game::draw_hud(c);
    }
    // The game menu's systems, with the points where their deferred changes take effect.
    if game_running(c) {
        gamemenu::open_requests(c);
    }
    c.apply_commands();
    if c.dialog.is_some() && c.menu.is_none() {
        gamemenu::open_from_outcome(c);
    }
    c.apply_commands();
    if c.menu.is_some() {
        gamemenu::gather_saving_input(c);
    }
    if c.menu.is_some() {
        gamemenu::run_menu(c);
    }
    c.apply_commands();
    if c.menu.is_some() {
        gamemenu::run_saving(c);
    }
    c.apply_commands();
    if c.menu.is_some() {
        gamemenu::run_settings(c);
    }
    c.apply_commands();
    if c.menu.is_some() {
        gamemenu::service_saving(c);
    }
    if in_game(c) {
        gamemenu::sync_settings(c);
    }
    c.apply_commands();
}

/// A layer to compose: its picture and its place on the screen.
struct Placing {
    picture: PictureId,
    z: f32,
    /// Its place in the order the layers were made.
    made: usize,
    origin: (f64, f64),
    scale: (f64, f64),
    clip: Clip,
    tint: f32,
}

/// The screen's size this frame (`crate::widescreen`): the game screen is as wide as the `widescreen` option says
/// (as the game decides it, for a network game's lobby), 640 wide anywhere else. `window`: what `auto` matches.
pub(crate) fn screen_size(c: &Core, window: Option<WindowGeom>) -> (u32, u32) {
    let original = crate::display::SCREEN;
    if c.states.get() != AppState::Game || c.hud.is_none() || c.sim.is_none() {
        return original;
    }
    let area = window.map(|w| (w.safe.2, w.safe.3));
    let width = crate::widescreen::width(game::extras(c).widescreen(), area, c.touch.blocks_widescreen());
    (width, original.1)
}

/// The window with the screen's size set (`WindowGeom::screen`); a run without a window gets one exactly the screen's.
fn stamp_screen(window: Option<WindowGeom>, screen: (u32, u32), headless: bool) -> Option<WindowGeom> {
    window.map(|mut w| {
        if headless {
            w = WindowGeom::new(screen, w.factor);
        }
        w.screen = screen;
        w
    })
}

/// Composes the screen from the layers (`layers`, `compose`), in this order:
///
/// | Order | Plane (`layers::Plane`) | Layers | Placed |
/// |---|---|---|---|
/// | 1 | Map | Terrain, map view, placement image (by z) | In the map view (`hud::Layout::viewport`), around the view's centre, at the zoom |
/// | 2 | Interface | The interface picture (`Hud`) | Centred on the screen, one picture pixel per screen pixel |
/// | 3 | Flow | The flow's layers (`frontend`, `gamemenu`: screens, dialogs, menus), by z | Centred on the screen at their size, with their fade |
///
/// Layers of a plane at the same z go in the order they were made. Outside a game the flow's screen is cleared before
/// its layers (`frontend::FrontScreen`), so only they show there.
///
/// The pointer goes on top of everything: while the game runs the interface picture has it (`hud`); over a game's
/// menus and dialogs the screen draws the local race's normal arrow (screen spec, "Pointer shapes"); outside a game it
/// draws the flow screen's own pointer, or none, and none through the screen's fades (`frontend::screen_pointer`,
/// "Pointers outside a game"). The system's pointer shows only where the screen wants a pointer and has none.
fn compose_screen(c: &mut Core) {
    let pictures = &c.pictures;
    // Outside a game the flow's screen clears the screen first (`frontend::camera_clear`).
    let front_clears = c.front_screen.clears;
    let (center_x, center_y) = (c.screen.0 as f64 / 2.0, c.screen.1 as f64 / 2.0);
    let mut map = Vec::new();
    let mut interface = Vec::new();
    let mut front = Vec::new();
    // The map view: its rectangle on the screen, the map pixel at its centre, and map pixels per screen pixel.
    let view = match (c.hud.as_ref().and_then(|h| h.layout), &c.view) {
        (Some(layout), Some(mv)) => {
            let v = layout.viewport;
            Some((
                Clip::new(v.x.max(0), v.y.max(0), v.w.max(1) as i32, v.h.max(1) as i32),
                (mv.centre.0 as f64, mv.centre.1 as f64),
                mv.zoom as f64,
            ))
        }
        _ => None,
    };
    for (made, l) in c.layers.iter().map(|e| &e.layer).enumerate() {
        if !l.visible {
            continue;
        }
        let Some(pic) = pictures.get(l.picture) else { continue };
        let (w, h) = l.drawn_size(pic);
        let (tw, th) = (pic.width as f64, pic.height as f64);
        let mut layer = Placing {
            picture: l.picture,
            z: l.z,
            made,
            origin: (0.0, 0.0),
            scale: (1.0, 1.0),
            clip: Clip::SCREEN,
            tint: l.tint,
        };
        match l.plane {
            Plane::Map => {
                let Some((clip, centre, per_px)) = view else { continue };
                let (vx, vy) = (
                    clip.x0 as f64 + (clip.x1 - clip.x0) as f64 / 2.0,
                    clip.y0 as f64 + (clip.y1 - clip.y0) as f64 / 2.0,
                );
                // The picture's top-left corner in map pixels.
                let (mx, my) = (l.at.0 as f64 - w as f64 / 2.0, l.at.1 as f64 - h as f64 / 2.0);
                layer.origin = (vx + (mx - centre.0) / per_px, vy + (my - centre.1) / per_px);
                layer.scale = (w as f64 / tw / per_px, h as f64 / th / per_px);
                layer.clip = clip;
                map.push(layer);
            }
            Plane::Interface => {
                // One picture pixel per screen pixel, centred.
                layer.origin = (center_x - tw / 2.0, center_y - th / 2.0);
                interface.push(layer);
            }
            Plane::Flow => {
                layer.origin = (
                    center_x + l.at.0 as f64 - w as f64 / 2.0,
                    center_y + l.at.1 as f64 - h as f64 / 2.0,
                );
                layer.scale = (w as f64 / tw, h as f64 / th);
                front.push(layer);
            }
        }
    }
    let by_z = |a: &Placing, b: &Placing| a.z.total_cmp(&b.z).then(a.made.cmp(&b.made));
    map.sort_by(by_z);
    interface.sort_by(by_z);
    front.sort_by(by_z);
    let order: Vec<&Placing> = if front_clears {
        front.iter().collect()
    } else {
        map.iter().chain(&interface).chain(&front).collect()
    };
    let mut frame = Frame::black_sized(c.screen.0 as usize, c.screen.1 as usize);
    for l in order {
        let Some(pic) = pictures.get(l.picture) else { continue };
        frame.draw(&Placed {
            rgba: &pic.rgba,
            width: pic.width,
            height: pic.height,
            origin: l.origin,
            scale: l.scale,
            clip: l.clip,
            tint: l.tint,
        });
    }
    // The pointer: the interface picture has it while the game runs; otherwise the normal arrow goes on top.
    let in_game = in_game(c);
    let hud = &c.hud;
    let hud_draws = hud.as_ref().is_some_and(|h| h.art.has_pointers()) && game_running(c);
    // Outside a game the screen's own pointer, gone through its fades; `hidden` when it shows none.
    let (arrow, hidden) = match hud {
        _ if hud_draws => (None, false),
        Some(h) if in_game => (h.art.pointer(crate::screen::Pointer::Arrow), false),
        _ if !in_game => match frontend::screen_pointer(
            &c.front_screen,
            &c.front,
            &c.layers,
            frontend::pointer_leaving(&c.states),
        ) {
            Ok(p) => (p, false),
            Err(()) => (None, true),
        },
        _ => (None, false),
    };
    if let (Some((pic, hot)), Some(p)) = (arrow, c.input.pointer)
        && let Some(w) = c.input.window
    {
        let (x, y) = w.letterbox().screen_point(p, w.factor);
        let (x, y) = (x.floor() as i32 - hot.0, y.floor() as i32 - hot.1);
        frame.draw(&Placed {
            rgba: &pic.rgba,
            width: pic.width,
            height: pic.height,
            origin: (x as f64, y as f64),
            scale: (1.0, 1.0),
            clip: Clip::SCREEN,
            tint: 1.0,
        });
    }
    // The system's pointer shows only where the screen wants a pointer and has none of its own.
    c.cursor_visible = !hud_draws && !hidden && arrow.is_none();
    c.frame = frame;
}

#[cfg(test)]
mod tests {
    use super::ticks_under;

    #[test]
    fn a_network_game_ticks_under_the_menu_and_single_player_stops() {
        // No menu: the game runs, single player or network.
        assert!(ticks_under(true, false, false));
        assert!(ticks_under(true, false, true));
        // The menu open: single player stops, a network game goes on (`ui/game-menu.md`, "Multiplayer").
        assert!(!ticks_under(true, true, false));
        assert!(ticks_under(true, true, true));
        // An outcome dialog, leaving the game, or another screen: nothing ticks.
        assert!(!ticks_under(false, false, true));
        assert!(!ticks_under(false, true, true));
    }
}
