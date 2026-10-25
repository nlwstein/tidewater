//! Scripted runs: a scene's input, fed to the program frame by frame, on a fixed frame time and a fixed wall clock,
//! so that a run comes out the same every time. Development aid (`--script NAME --script-out DIR`, which opens the
//! window and saves what it shows at the scene's shots) and the driver of the frame snapshot (`snapshot`, no window).
//!
//! The input goes in where the system's input comes in: the scene's key, button and wheel events replace the system's
//! in the player's input (`input::Input`) before the frame's input is applied, so keys and buttons behave as typed ones
//! do. The pointer is the scene's ([`Run::pointer`]), put where the system's would be; the mouse lock is off in a
//! scripted run. The window counts as focused.

use glam::Vec2;

use crate::compose::Frame;
use crate::core::Core;
use crate::frontend::{AppState, Step};
use crate::input::{Button, Key, KeyEvent, Touch, TouchPhase, Wheel, WindowGeom};

/// One step of a scene's input.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    /// Let this many frames pass.
    Wait(u32),
    /// Wait until the program shows this state.
    Until(AppState),
    /// Wait until the game has run this many cycles.
    UntilCycle(u32),
    /// Wait until the outcome dialog (victory or defeat) is up.
    UntilOutcome,
    /// The pointer goes to this screen pixel (the screen is 640 × 480, or wider in a game with `widescreen`).
    Move(f32, f32),
    Down(Button),
    Up(Button),
    Press(Key),
    Release(Key),
    /// Text typed (each character a press and release with its text).
    Type(&'static str),
    /// Mouse-wheel lines (positive: away from the user).
    Wheel(f32),
    /// A file or folder is dropped on the window (`firstrun`).
    Drop(&'static str),
    /// A finger (by number) does something at this screen pixel (640 × 480): the touch input (`gesture`).
    Finger(u64, TouchPhase, f32, f32),
    /// The frame is kept under this name; the next commands run from the next frame.
    Shot(&'static str),
    /// Wait until the Local Network game list shows at least this many games (network scenes: they wait on another
    /// program, so they wait on what it did, never on a count of frames).
    UntilGames(usize),
    /// Wait until the open game setup lobby has at least this many humans and this many observers.
    UntilLobby(usize, usize),
    /// Wait until the game list's selected game may take an observer (Watch Game shows).
    UntilWatchable,
    /// Wait until the creator's Start Game can be pressed (every joiner has the map, and in Classic mode is ready).
    UntilStartable,
    /// Wait until a message containing this text has shown (the message list or the notice line).
    UntilMessage(&'static str),
    /// From now on the run is a phone's or tablet's (`TouchState::touch_only`), as on iOS and Android.
    TouchDevice,
}

/// Where the multiplayer screens stand, for the network scenes' waits (`frontend::Screens::mp_progress`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MpProgress {
    /// Games on the Local Network game list, while it shows.
    pub games: usize,
    /// The selected game may take an observer.
    pub watchable: bool,
    /// The open lobby, if one is.
    pub lobby: Option<LobbyProgress>,
}

/// Where an open lobby stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LobbyProgress {
    pub humans: usize,
    pub watchers: usize,
    /// The creator's Start Game can be pressed.
    pub startable: bool,
}

/// A scripted scene: where the flow starts and the input.
#[derive(Clone, Debug)]
pub struct Scene {
    pub name: &'static str,
    pub start: Vec<Step>,
    pub cmds: Vec<Cmd>,
    /// Optional improvements (`extras`) the scene runs with, as (name, value) pairs; none in every scene but the ones
    /// that show an improvement, so the others show the game as it is.
    pub extras: &'static [(&'static str, &'static str)],
    /// The text of a `[keys]` section (`keymap`) the scene runs with; empty in every scene but the ones that show a
    /// rebinding, so the others use the original's keys.
    pub keys: &'static str,
}

/// The name of the scene that shows the first-run screen (`firstrun`) over a demo backend, with no game files.
pub const FIRST_RUN: &str = "firstrun";

/// The frame time of a scripted run.
pub const FRAME_SECS: f64 = 1.0 / 60.0;

/// The wall clock of a scripted run (seconds since 1970), so its games draw the same seed.
pub const WALL_SECS: u64 = 1_700_000_000;

/// A key pressed and released a frame later.
fn tap(k: Key) -> [Cmd; 3] {
    [Cmd::Press(k), Cmd::Wait(1), Cmd::Release(k)]
}

/// The left button pressed and released a frame later at a screen pixel.
fn click(x: f32, y: f32) -> [Cmd; 4] {
    [
        Cmd::Move(x, y),
        Cmd::Down(Button::Left),
        Cmd::Wait(1),
        Cmd::Up(Button::Left),
    ]
}

/// The scenes, by name.
pub fn scenes() -> Vec<Scene> {
    use Cmd::*;
    use Key as K;
    let mut campaign = vec![
        Move(320.0, 240.0),
        Until(AppState::Title),
        Wait(2),
        Shot("title-fade"),
        Wait(60),
    ];
    campaign.extend([Shot("title")]);
    campaign.extend(tap(K::Space));
    campaign.extend([Until(AppState::MainMenu), Wait(6), Shot("menu-fade"), Wait(20)]);
    campaign.extend([Move(320.0, 254.0), Wait(2), Shot("menu-hover")]);
    campaign.extend(click(320.0, 254.0));
    campaign.extend([Until(AppState::SinglePlayer), Wait(3), Shot("single-player")]);
    campaign.extend(click(320.0, 254.0));
    campaign.extend([
        Until(AppState::NewCampaign),
        Wait(2),
        Move(320.0, 300.0),
        Wait(1),
        Shot("campaigns"),
    ]);
    campaign.extend(click(320.0, 300.0));
    campaign.extend([
        Until(AppState::Act),
        Wait(20),
        Shot("act-text"),
        Wait(100),
        Shot("act-picture"),
    ]);
    campaign.extend(tap(K::Space));
    campaign.extend([
        Until(AppState::Briefing),
        Wait(10),
        Shot("briefing"),
        Wait(240),
        Shot("briefing-later"),
    ]);
    // Campaign flow, "Closing": Continue or Enter closes the briefing.
    campaign.extend(tap(K::Enter));
    campaign.extend([
        Until(AppState::Game),
        Move(400.0, 240.0),
        Wait(1),
        Shot("game-first"),
        Wait(15),
        Shot("game-fade"),
        Wait(60),
        Shot("game"),
    ]);
    // Selecting with a drag over the map view, then a right-click command.
    campaign.extend([
        Move(190.0, 30.0),
        Down(Button::Left),
        Wait(2),
        Move(400.0, 200.0),
        Wait(2),
        Shot("drag"),
        Move(620.0, 460.0),
        Wait(2),
        Up(Button::Left),
        Wait(2),
        Shot("selected"),
        Move(420.0, 120.0),
        Down(Button::Right),
        Wait(1),
        Up(Button::Right),
        Wait(3),
        Shot("commanded"),
        Wait(60),
        Shot("moving"),
    ]);
    // Scrolling with the keys and at the screen edge, and the minimap.
    campaign.extend([
        Press(K::Down),
        Wait(30),
        Release(K::Down),
        Wait(2),
        Shot("scrolled-keys"),
    ]);
    campaign.extend([Move(639.0, 240.0), Wait(40), Shot("scrolled-edge"), Move(400.0, 240.0)]);
    campaign.extend(click(60.0, 70.0));
    campaign.extend([Wait(3), Shot("minimap-click"), UntilCycle(300), Shot("cycle-300")]);
    // The game menu, its options, and back.
    campaign.extend(tap(K::F(10)));
    campaign.extend([Wait(3), Shot("game-menu")]);
    campaign.extend(tap(K::F(5)));
    campaign.extend([Wait(3), Shot("options")]);
    campaign.extend(tap(K::Escape));
    campaign.extend([Wait(3), Shot("back-to-menu")]);
    campaign.extend(tap(K::Escape));
    campaign.extend([Wait(3), Shot("menu-closed"), Wait(30)]);
    // The save dialog: a name typed and saved.
    campaign.extend(tap(K::F(11)));
    campaign.extend([
        Wait(3),
        Shot("save-dialog"),
        Wheel(-1.0),
        Wait(1),
        Type("snap"),
        Wait(2),
        Shot("save-typed"),
    ]);
    campaign.extend(tap(K::Enter));
    campaign.extend([Wait(3), Shot("saved"), Wait(30), Shot("end")]);
    vec![
        Scene {
            name: "campaign",
            start: vec![Step::Title, Step::MainMenu],
            cmds: campaign,
            extras: &[],
            keys: "",
        },
        Scene {
            name: "outcome",
            start: vec![play(0)],
            cmds: outcome(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "ending",
            start: vec![play(1)],
            cmds: ending(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "load",
            start: vec![play(0)],
            cmds: load(),
            extras: &[],
            keys: "",
        },
        // The Custom Scenario screen: opened from the main menu, its options and the Select scenario dialog changed, a game
        // started with them, and left for the main menu again.
        Scene {
            name: "custom",
            start: vec![Step::MainMenu],
            cmds: custom(),
            extras: &[],
            keys: "",
        },
        // Tidewater's Settings screen: opened by F2 and by the link on the main menu, each page, a drop-down, Defaults,
        // Cancel and OK.
        Scene {
            name: "settings-screen",
            start: vec![Step::MainMenu],
            cmds: settings(),
            extras: &[],
            keys: "",
        },
        // Tidewater's Settings screen over a game: Ctrl+G opens it (F2 is the first remembered view), the Gameplay page
        // shows the game's own rules and cannot change them, a value is changed and OK applies it; the Options
        // dialog's link opens it too.
        Scene {
            name: "settings-ingame",
            start: vec![play(0)],
            cmds: settings_ingame(),
            extras: &[("rally-points", "on"), ("selection-limit", "12")],
            keys: "",
        },
        // Magnifying zoom (`map-zoom`): the wheel over the map magnifies around the pointer up to 4 times and back, and
        // does nothing over the interface.
        Scene {
            name: "extras-zoom",
            start: vec![play(0)],
            cmds: extras_zoom(),
            extras: &[("map-zoom", "wheel")],
            keys: "",
        },
        // The multiplayer menus, up to the game list.
        Scene {
            name: "multi",
            start: vec![Step::MainMenu],
            cmds: multi(),
            extras: &[],
            keys: "",
        },
        // The game setup lobby a creator sees: Direct IP's Call, the name, the lobby, a row's list, a chat line.
        Scene {
            name: "lobby",
            start: vec![Step::MainMenu],
            cmds: lobby(),
            extras: &[],
            keys: "",
        },
        // The same lobby with an optional improvement on, so its Extras box shows: the list with the options the host
        // sets for everyone (the C+L ones among them), and the Observers list.
        Scene {
            name: "lobby-extras",
            start: vec![Step::MainMenu],
            cmds: lobby_extras(),
            extras: &[("health-bars", "hovered"), ("widescreen", "16:9")],
            keys: "",
        },
        Scene {
            name: "build",
            start: vec![play(0)],
            cmds: build(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "carry",
            start: vec![play(0)],
            cmds: carry(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "flow",
            start: crate::frontend::startup_steps(false, false, true),
            cmds: flow(),
            extras: &[],
            keys: "",
        },
        // Optional improvements switched on: every health bar, the idle-worker button and key, the clock with APM.
        Scene {
            name: "extras",
            start: vec![play(0)],
            cmds: extras(),
            extras: &[
                ("health-bars", "always"),
                ("idle-worker", "on"),
                ("game-clock", "clock-apm"),
                ("volume-voices", "40"),
                ("volume-effects", "60"),
                ("volume-music", "50"),
            ],
            keys: "",
        },
        // Hit point bars only over the unit under the pointer.
        Scene {
            name: "extras-bars",
            start: vec![play(0)],
            cmds: extras_bars(),
            extras: &[("health-bars", "hovered")],
            keys: "",
        },
        // The control-group bar: groups set by keys, the strip over the view's corner, a click on it recalls a group.
        Scene {
            name: "extras-groups",
            start: vec![play(0)],
            cmds: extras_groups(),
            extras: &[("group-bar", "on")],
            keys: "",
        },
        // Smart attack-move: a right-click on open ground with a fighting unit selected.
        Scene {
            name: "extras-attack",
            start: vec![play(0)],
            cmds: extras_attack(),
            extras: &[("smart-attack-move", "on")],
            keys: "",
        },
        // Rally points and the production queue: a barracks-type building selected, a rally flag, jobs waiting.
        Scene {
            name: "extras-sim",
            start: vec![play(0)],
            cmds: extras_sim(),
            extras: &[
                ("rally-points", "on"),
                ("production-queue", "on"),
                ("selection-limit", "unlimited"),
            ],
            keys: "",
        },
        // Rebound keys: the Move button on J, the pause key on Ctrl+U (Ctrl+P no longer pauses), the idle key on Ctrl+I.
        Scene {
            name: "extras-keys",
            start: vec![play(0)],
            cmds: extras_keys(),
            extras: &[("idle-worker", "on")],
            keys: "pause=ctrl+u\nidle-worker=ctrl+i\nbutton-move=j",
        },
        // Widescreen (`widescreen`, `docs/design/client-core.md`): the 864 x 480 game screen with the interface against
        // the left edge, a selection box past the old right edge, the minimap's view rectangle, the edge scroll at the
        // new edge, and the game menu in the middle with black beside it.
        Scene {
            name: "widescreen",
            start: vec![play(0)],
            cmds: widescreen(),
            extras: &[("widescreen", "16:9"), ("health-bars", "always")],
            keys: "",
        },
        // The 768 x 480 screen (16:10), the same start.
        Scene {
            name: "widescreen-16x10",
            start: vec![play(0)],
            cmds: widescreen_16x10(),
            extras: &[("widescreen", "16:10")],
            keys: "",
        },
        // Touch: a tap near a unit selects it; a long press orders; a hold then a drag boxes; a pinch zooms; two
        // fingers pan, and so does one with a glide; three fingers tapping open the game menu (`gesture`,
        // `docs/design/touch.md`); a phone's first game shows the Touch Controls page (`touchhelp`).
        Scene {
            name: "touch-select",
            start: vec![play(0)],
            cmds: touch_select(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "touch-order",
            start: vec![play(0)],
            cmds: touch_order(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "touch-drag",
            start: vec![play(0)],
            cmds: touch_drag(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "touch-pinch",
            start: vec![play(0)],
            cmds: touch_pinch(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "touch-pan",
            start: vec![play(0)],
            cmds: touch_pan(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "touch-swipe",
            start: vec![play(0)],
            cmds: touch_swipe(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "touch-help",
            start: vec![play(0)],
            cmds: touch_help(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "touch-help-menu",
            start: vec![Step::MainMenu],
            cmds: touch_help_menu(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "touch-menu",
            start: vec![play(0)],
            cmds: touch_menu(),
            extras: &[],
            keys: "",
        },
        // Needs no game files: `host::launch` shows the first-run screen over a demo backend.
        Scene {
            name: FIRST_RUN,
            start: Vec::new(),
            cmds: first_run(),
            extras: &[],
            keys: "",
        },
        // Not in the frame snapshot: `jukebox`'s tests run these to check the music at every frame.
        Scene {
            name: "music-quit",
            start: vec![Step::MainMenu],
            cmds: music_quit(),
            extras: &[],
            keys: "",
        },
        Scene {
            name: "music-back",
            start: vec![Step::MainMenu],
            cmds: music_back(),
            extras: &[],
            keys: "",
        },
    ]
    .into_iter()
    .chain(mp_scenes())
    .collect()
}

/// The flow's state changes: the startup videos (one video straight after another), the title, the main menu and the
/// campaign choices and back, a Beyond the Dark Portal campaign with its intro video, act screen and briefing. Then in
/// the game: an unused Escape (nothing happens), the game menu opened and closed with Escape, the `noglues` cheat so
/// that a restart, a chosen mission and a surrender's replay go from one game straight into the next, and the quit to
/// the main menu.
fn flow() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(320.0, 240.0), Until(AppState::Video), Wait(60), Shot("logo")];
    v.extend(tap(K::Escape));
    v.extend([Wait(5), Shot("intro-start"), Wait(120), Shot("intro")]);
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::Title), Wait(30), Shot("title")]);
    v.extend(tap(K::Space));
    v.extend([Until(AppState::MainMenu), Wait(25)]);
    v.extend(tap(K::Letter('S')));
    v.extend([Until(AppState::SinglePlayer), Wait(3), Shot("single-player")]);
    v.extend(tap(K::Letter('N')));
    v.extend([Until(AppState::NewCampaign), Wait(2), Shot("campaigns")]);
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::SinglePlayer), Wait(3), Shot("single-player-back")]);
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::MainMenu), Wait(25), Shot("menu-again")]);
    // Multi Player Game opens the multiplayer menus; Escape returns (the screens are in the `multi` scene).
    v.extend(tap(K::Letter('M')));
    v.extend([Until(AppState::Multiplayer), Wait(3), Shot("multi-open")]);
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::MainMenu), Wait(25)]);
    // Tab to Multi Player Game, Shift+Tab back past Single Player Game to Exit Program (wrapping), Tab to Single
    // Player Game.
    v.extend(tap(K::Tab));
    v.extend([Wait(2), Shot("menu-tab"), Press(K::ShiftLeft)]);
    v.extend(tap(K::Tab));
    v.push(Wait(1));
    v.extend(tap(K::Tab));
    v.extend([Release(K::ShiftLeft), Wait(2), Shot("menu-shift-tab")]);
    v.extend(tap(K::Tab));
    v.extend([Wait(2)]);
    v.extend(open_campaigns());
    // Over Previous Menu, then Beyond the Dark Portal's human campaign.
    v.extend([Move(320.0, 434.0), Wait(1)]);
    v.extend(click(320.0, 398.0));
    v.extend([Until(AppState::Video), Wait(60), Shot("bdp-intro")]);
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::Act), Wait(40), Shot("act")]);
    v.extend(tap(K::Space));
    v.extend([Until(AppState::Briefing), Wait(10), Shot("briefing")]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::Game), Move(400.0, 240.0), Wait(30), Shot("game")]);
    // An Escape nothing uses does nothing; in the game menu it returns to the game.
    v.extend(tap(K::Escape));
    v.extend([Wait(3), Shot("escape-unused")]);
    v.extend(tap(K::F(10)));
    v.extend([Wait(3), Shot("game-menu")]);
    v.extend(tap(K::Escape));
    v.extend([Wait(3), Shot("menu-closed")]);
    v.extend(chat("noglues"));
    v.extend([Wait(5), Press(K::ControlLeft), Wait(1)]);
    v.extend(tap(K::Letter('R')));
    v.extend([Release(K::ControlLeft), Wait(3), Shot("restart-confirm")]);
    // Restart fades all sound first with the game's picture kept and the game stopped (about 536 ms), then the
    // mission starts again.
    v.extend(tap(K::Enter));
    v.extend([Wait(10), Shot("restart-fading"), Wait(50), Shot("restarted")]);
    v.extend(chat("tigerlily"));
    v.extend(chat("human2"));
    v.extend([Wait(30), Shot("chosen-mission")]);
    v.extend(tap(K::F(10)));
    v.extend(tap(K::Letter('E')));
    v.extend([Wait(3), Shot("end-menu"), Move(320.0, 226.0), Wait(1)]);
    v.extend(click(320.0, 226.0)); // Surrender
    v.extend([Wait(3), Shot("surrender-confirm")]);
    v.extend(tap(K::Enter));
    v.extend([UntilOutcome, Wait(5), Shot("defeat")]);
    v.extend(tap(K::Enter));
    v.extend([Wait(60), Shot("replay")]);
    v.extend(tap(K::F(10)));
    v.extend(tap(K::Letter('E')));
    v.extend([Wait(3), Move(320.0, 258.0), Wait(1)]);
    v.extend(click(320.0, 258.0)); // Quit to Main Menu
    v.extend([Wait(3), Shot("quit-confirm")]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::MainMenu), Wait(25), Shot("menu-end")]);
    v
}

/// The campaign menu's rows (screen spec, "Main menu and campaign menu"): the Tides of Darkness orc and human
/// campaigns.
const TOD_ORC_ROW: f32 = 264.0;
const TOD_HUMAN_ROW: f32 = 300.0;
/// The main menu's Single Player Game button, and the single-player menu's New Campaign button: the same place.
const NEW_CAMPAIGN_ROW: f32 = 254.0;

/// From the main menu: Single Player Game, then New Campaign, to the campaign menu.
fn open_campaigns() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = click(320.0, NEW_CAMPAIGN_ROW).to_vec();
    v.extend([Until(AppState::SinglePlayer), Wait(3)]);
    v.extend(click(320.0, NEW_CAMPAIGN_ROW));
    v.extend([Until(AppState::NewCampaign), Wait(2)]);
    v
}

/// From the main menu: New Campaign, the campaign in `row`, its act screen and briefing (closed by Space and Enter), to
/// the game.
fn new_campaign(row: f32) -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Until(AppState::MainMenu), Wait(25)];
    v.extend(open_campaigns());
    v.extend(click(320.0, row));
    v.extend([Until(AppState::Act), Wait(40)]);
    v.extend(tap(Key::Space));
    v.extend([Until(AppState::Briefing), Wait(10)]);
    v.extend(tap(Key::Enter));
    v.extend([Until(AppState::Game), Wait(60)]);
    v
}

/// Ctrl+`k`.
fn ctrl(k: Key) -> Vec<Cmd> {
    let mut v = vec![Cmd::Press(Key::ControlLeft), Cmd::Wait(1)];
    v.extend(tap(k));
    v.extend([Cmd::Release(Key::ControlLeft), Cmd::Wait(3)]);
    v
}

/// The music playtest: the human campaign's first mission, Ctrl+Q and its confirm back to the main menu, then the orc
/// campaign through its act screen and briefing into its game.
fn music_quit() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(320.0, 240.0)];
    v.extend(new_campaign(TOD_HUMAN_ROW));
    v.push(Shot("first-game"));
    v.extend(ctrl(Key::Letter('Q')));
    v.extend(tap(Key::Enter));
    v.extend(new_campaign(TOD_ORC_ROW));
    v.push(Shot("second-game"));
    v
}

/// The other ways back: the campaign choice's Escape, an act screen closed by Escape, a briefing where Escape does
/// nothing (Enter closes it), a restart
/// (Ctrl+R), the game menu's end menu "Quit to Main Menu", and a campaign started with Ctrl held (no act screen or
/// briefing, so the last game's track is still playing when the next game starts).
fn music_back() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(320.0, 240.0), Until(AppState::MainMenu), Wait(25)];
    v.extend(open_campaigns());
    // Escape on the campaign menu and again on the single-player menu go back, and track 22 plays on.
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::SinglePlayer), Wait(3)]);
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::MainMenu), Wait(25)]);
    v.extend(open_campaigns());
    v.extend(click(320.0, TOD_ORC_ROW));
    v.extend([Until(AppState::Act), Wait(40)]);
    v.extend(tap(K::Escape));
    // Escape does nothing on the briefing; Enter closes it.
    v.extend([Until(AppState::Briefing), Wait(10)]);
    v.extend(tap(K::Escape));
    v.extend([Wait(30), Shot("briefing-escape")]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::Game), Wait(60), Shot("orc-game")]);
    // A restart shows the mission's briefing again.
    v.extend(ctrl(K::Letter('R')));
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::Briefing), Wait(10)]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::Game), Wait(60), Shot("restarted")]);
    v.extend(tap(K::F(10)));
    v.extend(tap(K::Letter('E')));
    v.extend([Wait(3), Move(320.0, 258.0), Wait(1)]);
    v.extend(click(320.0, 258.0)); // Quit to Main Menu
    v.extend([Wait(3)]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::MainMenu), Wait(25)]);
    v.extend(open_campaigns());
    v.extend([Move(320.0, TOD_HUMAN_ROW), Wait(1)]);
    v.extend([Press(K::ControlLeft), Wait(1)]);
    v.extend(click(320.0, TOD_HUMAN_ROW));
    v.extend([
        Until(AppState::Game),
        Release(K::ControlLeft),
        Wait(60),
        Shot("human-game"),
    ]);
    v
}

/// A campaign mission's map, started at once.
fn play(entry: u8) -> Step {
    Step::Play {
        map: crate::campaign::map_name(entry),
        setup: None,
    }
}

/// A finger touching the screen at a screen pixel.
fn touch(id: u64, x: f32, y: f32) -> Cmd {
    Cmd::Finger(id, TouchPhase::Began, x, y)
}

/// A finger moving to a screen pixel.
fn slide(id: u64, x: f32, y: f32) -> Cmd {
    Cmd::Finger(id, TouchPhase::Moved, x, y)
}

/// A finger lifting at a screen pixel.
fn lift(id: u64, x: f32, y: f32) -> Cmd {
    Cmd::Finger(id, TouchPhase::Ended, x, y)
}

/// A finger tapping a screen pixel: down, a frame, up.
fn finger_tap(x: f32, y: f32) -> [Cmd; 3] {
    [touch(1, x, y), Cmd::Wait(1), lift(1, x, y)]
}

/// The game's first frames, then a tap a finger's width beside the first peasant, which selects it (the tap
/// tolerance), and a tap on empty ground, which sends it there (the smart tap: a right click with a unit of yours
/// selected).
fn touch_select() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Until(AppState::Game), Wait(5), Shot("game")];
    v.extend(finger_tap(444.0, 318.0));
    v.extend([Wait(4), Shot("tapped")]);
    v.extend(finger_tap(300.0, 200.0));
    v.extend([Wait(4), Shot("tapped-ground")]);
    v
}

/// Select a peasant with a tap, then press and hold on open ground: a right click that sends it there, with a pulse.
fn touch_order() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Until(AppState::Game), Wait(5)];
    v.extend(finger_tap(416.0, 318.0));
    v.extend([Wait(4), Shot("selected")]);
    v.extend([
        touch(1, 520.0, 400.0),
        Wait(10),
        Shot("held"),
        Wait(26),
        Shot("pulse"),
        Wait(10),
    ]);
    v.extend([
        lift(1, 520.0, 400.0),
        Wait(3),
        Shot("ordered"),
        UntilCycle(120),
        Shot("moved"),
    ]);
    v
}

/// Hold a finger on open ground until it arms (the ring), then drag it over the peasants: the box select.
fn touch_drag() -> Vec<Cmd> {
    use Cmd::*;
    vec![
        Until(AppState::Game),
        Wait(5),
        touch(1, 340.0, 250.0),
        Wait(20),
        Shot("armed"),
        slide(1, 370.0, 280.0),
        Wait(2),
        slide(1, 440.0, 340.0),
        Wait(2),
        slide(1, 500.0, 400.0),
        Wait(2),
        Shot("box"),
        lift(1, 500.0, 400.0),
        Wait(4),
        Shot("boxed"),
    ]
}

/// Two fingers spreading zoom the view in around the point between them, and pinching zoom it out again to the
/// original's view.
fn touch_pinch() -> Vec<Cmd> {
    use Cmd::*;
    // Around the first peasant, which stays under the fingers as the view zooms.
    let (cx, cy) = (416.0, 318.0);
    let mut v = vec![
        Until(AppState::Game),
        Wait(5),
        Shot("game"),
        touch(1, cx - 20.0, cy),
        touch(2, cx + 20.0, cy),
        Wait(2),
    ];
    for step in 1..=6 {
        let d = 20.0 + 20.0 * step as f32;
        v.extend([slide(1, cx - d, cy), slide(2, cx + d, cy), Wait(2)]);
    }
    v.extend([
        Shot("zoomed-in"),
        lift(1, cx - 140.0, cy),
        lift(2, cx + 140.0, cy),
        Wait(3),
    ]);
    v.extend([touch(1, cx - 140.0, cy), touch(2, cx + 140.0, cy), Wait(2)]);
    for step in 1..=10 {
        let d = 140.0 - 12.0 * step as f32;
        v.extend([slide(1, cx - d, cy), slide(2, cx + d, cy), Wait(2)]);
    }
    v.extend([
        Shot("zoomed-out"),
        lift(1, cx - 20.0, cy),
        lift(2, cx + 20.0, cy),
        Wait(3),
        Shot("done"),
    ]);
    v
}

/// Two fingers dragging together pan the view a tile at a time.
fn touch_pan() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![
        Until(AppState::Game),
        Wait(5),
        Shot("game"),
        touch(1, 300.0, 300.0),
        touch(2, 360.0, 300.0),
        Wait(2),
    ];
    for step in 1..=8 {
        let dx = 14.0 * step as f32;
        v.extend([
            slide(1, 300.0 - dx, 300.0 - dx / 2.0),
            slide(2, 360.0 - dx, 300.0 - dx / 2.0),
            Wait(2),
        ]);
    }
    v.extend([
        Shot("panning"),
        lift(1, 188.0, 244.0),
        lift(2, 248.0, 244.0),
        Wait(3),
        Shot("panned"),
    ]);
    v
}

/// One finger dragging on the map pans the view without a click, and a flick glides on after the lift.
fn touch_swipe() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![
        Until(AppState::Game),
        Wait(5),
        Shot("game"),
        touch(1, 400.0, 300.0),
        Wait(2),
    ];
    for step in 1..=6 {
        let d = 12.0 * step as f32;
        v.extend([slide(1, 400.0 - d, 300.0 - d / 2.0), Wait(1)]);
    }
    v.extend([
        Shot("panning"),
        lift(1, 328.0, 264.0),
        Wait(2),
        Shot("gliding"),
        Wait(60),
        Shot("stopped"),
    ]);
    v
}

/// A phone's first game: the Touch Controls page shows by itself, Enter closes it, and the Help menu (F1) offers it
/// again.
fn touch_help() -> Vec<Cmd> {
    use Cmd::*;
    vec![
        TouchDevice,
        Until(AppState::Game),
        Wait(10),
        Shot("help"),
        Press(Key::Enter),
        Release(Key::Enter),
        Wait(5),
        Shot("help-2"),
        Press(Key::Enter),
        Release(Key::Enter),
        Wait(5),
        Shot("closed"),
        Press(Key::F(1)),
        Release(Key::F(1)),
        Wait(5),
        Shot("help-menu"),
        Press(Key::Escape),
        Release(Key::Escape),
        Wait(5),
    ]
}

/// The main menu on a phone: the Touch controls link above the Settings link, its pages over the menu, a tap on Next
/// and one on OK, which closes them.
fn touch_help_menu() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![TouchDevice, Until(AppState::MainMenu), Wait(25), Shot("menu")];
    v.extend(finger_tap(560.0, 406.0));
    v.extend([Wait(3), Shot("page")]);
    // Next, then OK in the same place on the second page (`touchhelp::buttons`).
    v.extend(finger_tap(521.0, 436.0));
    v.extend([Wait(3), Shot("page-2")]);
    v.extend(finger_tap(521.0, 436.0));
    v.extend([Wait(3), Shot("closed")]);
    v
}

/// Three fingers tapping open the game menu.
fn touch_menu() -> Vec<Cmd> {
    use Cmd::*;
    vec![
        Until(AppState::Game),
        Wait(5),
        touch(1, 250.0, 240.0),
        touch(2, 320.0, 240.0),
        touch(3, 390.0, 240.0),
        Wait(3),
        lift(1, 250.0, 240.0),
        lift(2, 320.0, 240.0),
        lift(3, 390.0, 240.0),
        Wait(4),
        Shot("menu"),
    ]
}

/// A line typed into the chat line and sent: Enter opens it, Enter sends it.
fn chat(line: &'static str) -> Vec<Cmd> {
    let mut v = tap(Key::Enter).to_vec();
    v.extend([Cmd::Wait(2), Cmd::Type(line), Cmd::Wait(2)]);
    v.extend(tap(Key::Enter));
    v
}

/// The chat line and the cheats, the victory dialog with its save dialog, the score screen, the next mission's
/// briefing, then the defeat dialog, its score screen and the replay's briefing.
fn outcome() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5), Shot("game")];
    v.extend(tap(K::Enter));
    v.extend([Wait(2), Type("unite the"), Wait(2), Shot("chat-typing")]);
    v.extend([Type(" clans"), Wait(1)]);
    v.extend(tap(K::Enter));
    v.extend([UntilOutcome, Wait(5), Shot("victory")]);
    v.extend(click(320.0, 229.0));
    v.extend([Wait(3), Shot("victory-save")]);
    v.extend(tap(K::Escape));
    v.extend([Wait(3), Shot("victory-again")]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::Score), Wait(20), Shot("score-counting")]);
    v.extend(tap(K::Space));
    v.extend([Wait(3), Shot("score-end")]);
    v.extend(tap(K::Space));
    v.extend([Until(AppState::Briefing), Wait(10), Shot("briefing-next")]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::Game), Wait(30), Shot("game-next")]);
    v.extend(chat("you pitiful worm"));
    v.extend([UntilOutcome, Wait(5), Shot("defeat")]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::Score), Wait(20), Shot("score-defeat")]);
    v.extend(tap(K::Space));
    v.extend([Wait(3)]);
    v.extend(tap(K::Space));
    v.extend([Until(AppState::Briefing), Wait(10), Shot("briefing-replay")]);
    v
}

/// The finale cheat: the ending's video (left at once), the finale, the credits, then the main menu and its load
/// dialog with no saved games; then the main menu's Show Credits (C): the credits screen's fade-in and its first page,
/// closed with Space.
fn ending() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5)];
    v.extend(chat("there can be only one"));
    v.extend([Until(AppState::Video), Wait(1)]);
    v.extend(tap(K::Escape));
    v.extend([
        Until(AppState::Finale),
        Wait(30),
        Shot("finale"),
        Wait(300),
        Shot("finale-later"),
    ]);
    v.extend(tap(K::Space));
    v.extend([
        Until(AppState::Credits),
        Wait(3),
        Shot("credits-fade"),
        Wait(300),
        Shot("credits"),
    ]);
    v.extend(tap(K::Space));
    v.extend([
        Wait(3),
        Shot("credits-closing"),
        Until(AppState::MainMenu),
        Wait(20),
        Shot("menu"),
    ]);
    v.extend(tap(K::Letter('S')));
    v.extend([Until(AppState::SinglePlayer), Wait(3)]);
    v.extend(tap(K::Letter('L')));
    v.extend([Wait(3), Shot("load-empty")]);
    v.extend(tap(K::Escape));
    v.extend([Wait(3), Shot("load-closed")]);
    // Previous Menu (Escape) goes back to the main menu.
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::MainMenu), Wait(3)]);
    v.extend(tap(K::Letter('C')));
    v.extend([
        Until(AppState::Credits),
        Wait(3),
        Shot("show-credits-fade"),
        Wait(60),
        Shot("show-credits"),
    ]);
    v.extend(tap(K::Space));
    v.extend([Until(AppState::MainMenu), Wait(20), Shot("menu-after-credits")]);
    v
}

/// A game saved and loaded again from the game menu's load dialog, then quit to the main menu and loaded from there;
/// then a screenshot (Print Screen) and its flash.
fn load() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![
        Move(400.0, 240.0),
        Until(AppState::Game),
        UntilCycle(100),
        Shot("before-save"),
    ];
    v.extend(tap(K::F(11)));
    v.extend([Wait(3), Type("one"), Wait(1), Shot("save-dialog")]);
    v.extend(tap(K::Enter));
    v.extend([Wait(3), Shot("saved"), UntilCycle(200), Shot("cycle-200")]);
    v.extend(tap(K::F(12)));
    v.extend([Wait(3), Shot("load-dialog")]);
    v.extend(tap(K::Enter));
    v.extend([
        Wait(5),
        Until(AppState::Game),
        Wait(5),
        Shot("loaded"),
        Wait(60),
        Shot("loaded-later"),
    ]);
    v.extend([Press(K::ControlLeft), Wait(1)]);
    v.extend(tap(K::Letter('Q')));
    v.extend([Release(K::ControlLeft), Wait(3), Shot("quit-confirm")]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::MainMenu), Wait(25)]);
    v.extend(tap(K::Letter('S')));
    v.extend([Until(AppState::SinglePlayer), Wait(3)]);
    v.extend(tap(K::Letter('L')));
    v.extend([Wait(3), Shot("menu-load")]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::Game), Wait(30), Shot("menu-loaded")]);
    v.extend(tap(K::PrintScreen));
    v.extend([Wait(1), Shot("printed"), Wait(5), Shot("flash-over")]);
    v
}

/// The Custom Scenario screen (`customsetup`): Single Player Game, then Custom Scenario opens it; the Resources box opened and High picked
/// with the pointer; the Opponents box opened and 3 Opponents picked with the keys; the Select scenario dialog
/// filtered to 5 to 8 players and its first line picked; the race and units chosen; Start Game; the game left by
/// Ctrl+Q.
fn custom() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(320.0, 240.0), Until(AppState::MainMenu), Wait(25)];
    v.extend(tap(K::Letter('S')));
    v.extend([Until(AppState::SinglePlayer), Wait(3), Shot("single-player")]);
    v.extend(tap(K::Letter('C')));
    v.extend([
        Wait(3),
        Shot("menu-leaving"),
        Until(AppState::CustomSetup),
        Wait(3),
        Shot("fading-in"),
    ]);
    v.extend([Wait(20), Shot("setup")]);
    // Resources: the box opens a list below it; High is its fourth line.
    v.extend(click(300.0, 285.0));
    v.extend([Wait(2), Move(300.0, 340.0), Wait(1), Shot("resources-list")]);
    v.extend(click(300.0, 364.0));
    v.extend([Wait(2), Shot("resources-high")]);
    // Opponents with the keys: Tab to the box, Enter opens its list, Down three times, Enter picks.
    v.extend(tap(K::Tab));
    v.extend(tap(K::Enter));
    v.extend([Wait(2), Shot("opponents-list")]);
    for _ in 0..3 {
        v.extend(tap(K::Down));
    }
    v.extend(tap(K::Enter));
    v.extend([Wait(2), Shot("opponents-3")]);
    // Your Race: Orc; Units: One Peasant Only.
    v.extend(click(100.0, 285.0));
    v.push(Wait(2));
    v.extend(click(100.0, 324.0));
    v.push(Wait(2));
    v.extend(click(450.0, 285.0));
    v.push(Wait(2));
    v.extend(click(450.0, 324.0));
    v.push(Wait(2));
    v.extend([Wait(2), Shot("options")]);
    // Select Scenario: the dialog, the Players filter to 5 to 8 players, then OK.
    v.extend(click(500.0, 382.0));
    v.extend([Wait(2), Shot("dialog")]);
    v.extend(click(300.0, 128.0));
    v.extend([Wait(2), Shot("players-list")]);
    v.extend(click(300.0, 186.0));
    v.extend([Wait(2), Shot("filtered")]);
    v.extend(tap(K::Down));
    v.extend([Wait(2), Shot("second-line")]);
    v.extend(click(470.0, 394.0));
    v.extend([Wait(2), Shot("picked")]);
    // Cancel Game fades back to the main menu; S then C opens the screen again with the same map.
    v.extend(tap(K::Letter('C')));
    v.extend([Until(AppState::MainMenu), Wait(25), Shot("menu-back")]);
    v.extend(tap(K::Letter('S')));
    v.extend([Until(AppState::SinglePlayer), Wait(3)]);
    v.extend(tap(K::Letter('C')));
    v.extend([Until(AppState::CustomSetup), Wait(25), Shot("setup-again")]);
    // Start Game: the music fades, the screen fades to black and the game starts.
    v.extend(click(500.0, 418.0));
    v.extend([
        Wait(5),
        Shot("starting"),
        Until(AppState::Game),
        Move(400.0, 240.0),
        Wait(60),
        Shot("game"),
    ]);
    v.extend([Press(K::ControlLeft), Wait(1)]);
    v.extend(tap(K::Letter('Q')));
    v.extend([Release(K::ControlLeft), Wait(3)]);
    v.extend(tap(K::Enter));
    v.extend([Until(AppState::MainMenu), Wait(25), Shot("menu-end")]);
    v
}

/// Tidewater's Settings screen (`settings`): F2 on the main menu opens it, each page is looked at, a drop-down is
/// opened and a value picked with the pointer, the keyboard moves the focus, Defaults resets, Esc leaves; the link
/// opens it again, a volume is chosen and OK applies it.
fn settings() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(320.0, 240.0), Until(AppState::MainMenu), Wait(25), Shot("menu")];
    v.extend([
        Move(560.0, 462.0),
        Wait(2),
        Shot("menu-link-hover"),
        Move(320.0, 200.0),
        Wait(1),
    ]);
    v.extend(tap(K::F(2)));
    v.extend([
        Wait(3),
        Shot("menu-leaving"),
        Until(AppState::Settings),
        Wait(3),
        Shot("fading-in"),
        Wait(20),
        Shot("general"),
    ]);
    // The Scaling row (the second): its list, then the second line.
    v.extend(click(460.0, 144.0));
    v.extend([Wait(2), Move(460.0, 190.0), Wait(1), Shot("scaling-list")]);
    v.extend(click(460.0, 187.0));
    v.extend([Wait(2), Shot("scaling-sharp")]);
    // The tabs.
    v.extend(click(212.0, 76.0));
    v.extend([Wait(2), Shot("interface")]);
    v.extend(click(320.0, 76.0));
    v.extend([Wait(2), Shot("sound")]);
    v.extend(click(460.0, 174.0));
    v.extend([Wait(2), Shot("music-list")]);
    v.extend(tap(K::Escape));
    v.extend(click(428.0, 76.0));
    v.extend([Wait(2), Shot("gameplay")]);
    // The keyboard: Tab to the first row, Enter opens its list, Down and Enter pick.
    v.extend(tap(K::Tab));
    v.extend(tap(K::Tab));
    v.extend([Wait(2), Shot("focus")]);
    v.extend(tap(K::Enter));
    v.extend(tap(K::Down));
    v.extend(tap(K::Enter));
    v.extend([Wait(2), Shot("rally-on")]);
    // The Keys page: the list, a click on Pause asks for a key, Ctrl+U binds it, Ctrl+S is refused (it is Sound effects
    // on or off), the wheel scrolls, Backspace puts a key back and Reset keys every key.
    v.extend(click(536.0, 76.0));
    v.extend([Wait(2), Shot("keys")]);
    v.extend(click(300.0, 160.0));
    v.extend([Wait(2), Shot("keys-press")]);
    v.extend([Press(K::ControlLeft), Wait(1)]);
    v.extend(tap(K::Letter('U')));
    v.extend([Release(K::ControlLeft), Wait(2), Shot("keys-bound")]);
    v.extend(click(300.0, 184.0));
    v.extend([Press(K::ControlLeft), Wait(1)]);
    v.extend(tap(K::Letter('S')));
    v.extend([Release(K::ControlLeft), Wait(2), Shot("keys-conflict")]);
    v.extend([Wheel(-5.0), Wait(2), Shot("keys-scrolled")]);
    v.extend(tap(K::Tab));
    v.extend(tap(K::Enter));
    v.extend([Wait(2), Shot("keys-reset")]);
    // Defaults resets every row; Esc leaves without applying anything.
    v.extend(click(428.0, 76.0));
    v.extend([Wait(2)]);
    v.extend(click(117.0, 424.0));
    v.extend([Wait(2), Shot("defaults")]);
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::MainMenu), Wait(25), Shot("menu-back")]);
    // The link opens it again; the music volume is set to 30% and OK applies it.
    v.extend(click(560.0, 462.0));
    v.extend([Until(AppState::Settings), Wait(25), Shot("again"), Wait(2)]);
    v.extend(click(320.0, 76.0));
    v.extend([Wait(2), Shot("again-sound")]);
    v.extend(click(460.0, 174.0));
    v.extend([Wait(2), Move(460.0, 250.0), Wait(1)]);
    v.extend(click(460.0, 247.0));
    v.extend([Wait(2), Shot("music-30")]);
    v.extend(click(523.0, 424.0));
    v.extend([Until(AppState::MainMenu), Wait(25), Shot("menu-end")]);
    v
}

/// Magnifying zoom (`map-zoom`): wheel steps over the map view zoom in around the pointer (2 times, 4 times, and no
/// further), the wheel over the minimap does nothing, and steps the other way come back to the original's view and stop
/// there.
fn extras_zoom() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(500.0, 250.0), Until(AppState::Game), Wait(30), Shot("normal")];
    v.extend([Wheel(1.0), Wait(3), Shot("in-2x"), Wheel(1.0), Wait(3), Shot("in-4x")]);
    v.extend([Wheel(1.0), Wait(3), Shot("in-4x-again")]);
    v.extend([Move(90.0, 90.0), Wait(1), Wheel(1.0), Wait(3), Shot("over-the-minimap")]);
    v.extend([Move(500.0, 250.0), Wait(1), Wheel(-1.0), Wait(3), Shot("out-2x")]);
    v.extend([
        Wheel(-1.0),
        Wait(3),
        Shot("out-1x"),
        Wheel(-1.0),
        Wait(3),
        Shot("out-1x-again"),
    ]);
    v
}

/// The Settings screen over a game (`gamemenu::run_settings`): Ctrl+G opens it over the running game, the pages are
/// looked at, a value is changed and OK applies it at once, and the link in the Options dialog opens it again.
fn settings_ingame() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(30), Shot("game")];
    v.extend(ctrl_tap(K::Letter('G')));
    v.extend([Wait(3), Shot("general")]);
    // The game stands still while it is open.
    v.extend([Wait(20), Shot("general-later")]);
    // Interface: Health bars (the first row) to Always.
    v.extend(click(212.0, 76.0));
    v.extend([Wait(2)]);
    v.extend(click(460.0, 114.0));
    v.extend([Wait(2), Move(460.0, 195.0), Wait(1), Shot("health-list")]);
    v.extend(click(460.0, 192.0));
    v.extend([Wait(2), Shot("health-always")]);
    // Gameplay: the game's rules, greyed, and a click on one does nothing.
    v.extend(click(428.0, 76.0));
    v.extend([Wait(2), Shot("gameplay")]);
    v.extend(click(460.0, 114.0));
    v.extend([Wait(2), Shot("gameplay-click")]);
    // Keys: Pause is given Ctrl+U, which OK applies at once (the game's key table reads the stored keys every frame).
    v.extend(click(536.0, 76.0));
    v.extend(click(300.0, 160.0));
    v.extend([Press(K::ControlLeft), Wait(1)]);
    v.extend(tap(K::Letter('U')));
    v.extend([Release(K::ControlLeft), Wait(2), Shot("keys")]);
    // OK applies and returns to the game, which runs again with health bars on.
    v.extend(click(523.0, 424.0));
    v.extend([Wait(40), Shot("applied")]);
    v.extend([Press(K::ControlLeft), Wait(1)]);
    v.extend(tap(K::Letter('U')));
    v.extend([Release(K::ControlLeft), Wait(10), Shot("new-pause-key")]);
    // F2 is still the first remembered view.
    v.extend(tap(K::F(2)));
    v.extend([Wait(3), Shot("f2-view")]);
    // The Options dialog's link, then Esc back to the game.
    v.extend(tap(K::F(5)));
    v.extend([Wait(3), Shot("options")]);
    v.extend([Move(410.0, 322.0), Wait(1), Shot("options-link-hover")]);
    v.extend(click(410.0, 322.0));
    v.extend([Wait(3), Shot("from-link")]);
    v.extend(tap(K::Escape));
    v.extend([Wait(3), Shot("cancelled")]);
    v
}

/// The multiplayer menus (`mpmenu`): the choice, the connection list with Battle.net disabled, the Direct IP setup
/// with its Mode list, name entry, the game list that Local Network opens, and Classic's name entry first.
fn multi() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(320.0, 240.0), Until(AppState::MainMenu), Wait(25), Shot("menu")];
    v.extend(tap(K::Letter('M')));
    v.extend([Until(AppState::Multiplayer), Wait(3), Shot("choice")]);
    v.extend(tap(K::Letter('E')));
    v.extend([Wait(3), Shot("methods")]);
    // Battle.net is the first row, drawn disabled; choosing it shows why.
    v.extend(click(100.0, 206.0));
    v.extend([Wait(2), Shot("battle")]);
    // Direct IP: the setup screen, its Mode list opened, Answer picked and an address typed.
    v.extend(click(100.0, 224.0));
    v.extend(tap(K::Enter));
    v.extend([Wait(3), Shot("direct")]);
    v.extend(click(130.0, 376.0));
    v.extend([Wait(2), Shot("mode-list")]);
    v.extend(click(130.0, 410.0));
    v.extend(click(200.0, 342.0));
    v.push(Type("192.168.0.7"));
    v.extend([Wait(2), Shot("answer")]);
    // Previous Menu back to the list; Local Network and Connect ask for the name.
    v.extend(tap(K::Escape));
    v.extend(click(100.0, 242.0));
    v.extend([Wait(2), Shot("local")]);
    v.extend(tap(K::Letter('C')));
    v.extend([Wait(3), Shot("name")]);
    for _ in 0..9 {
        v.extend(tap(K::Backspace));
    }
    v.push(Type("Fen"));
    v.extend([Wait(2), Shot("name-typed")]);
    v.extend(tap(K::Enter));
    v.extend([Wait(3), Shot("games")]);
    // Disconnect leaves for the main menu.
    v.extend(tap(K::Letter('D')));
    v.extend([Until(AppState::MainMenu), Wait(25)]);
    // Classic mode asks for the name before the list, which has no Battle.net row.
    v.extend(tap(K::Letter('M')));
    v.extend([Until(AppState::Multiplayer), Wait(3)]);
    v.extend(tap(K::Letter('C')));
    v.extend([Wait(3), Shot("classic-name")]);
    v.extend(tap(K::Enter));
    v.extend([Wait(3), Shot("classic-methods")]);
    v.extend(tap(K::Escape));
    v.extend([Until(AppState::MainMenu), Wait(25), Shot("menu-end")]);
    v
}

/// The lobby as its creator sees it, then a row's list opened, a chat line sent and Start refused (nobody has joined).
fn lobby() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(320.0, 240.0), Until(AppState::MainMenu), Wait(25)];
    v.extend(tap(K::Letter('M')));
    v.extend([Until(AppState::Multiplayer), Wait(3)]);
    v.extend(tap(K::Letter('E')));
    v.extend([Wait(3)]);
    v.extend(click(100.0, 224.0));
    v.extend(tap(K::Enter));
    v.extend([Wait(3)]);
    v.extend(tap(K::Letter('C')));
    v.extend([Wait(3)]);
    v.extend(tap(K::Enter));
    v.extend([Wait(6), Shot("lobby")]);
    v.extend(click(100.0, 92.0));
    v.extend([Wait(3), Shot("row-list")]);
    v.extend(tap(K::Escape));
    v.extend(click(300.0, 56.0));
    v.extend([Wait(2), Type("hello"), Wait(2), Shot("chat-typed")]);
    v.extend(tap(K::Enter));
    v.extend([Wait(3), Shot("chat-sent")]);
    v.extend(tap(K::Escape));
    v.extend(tap(K::Letter('S')));
    v.extend([Wait(3), Shot("alone")]);
    v
}

/// The creator's lobby with the Extras box showing: its list opened, a C+L row cycled, then the Observers list.
fn lobby_extras() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(320.0, 240.0), Until(AppState::MainMenu), Wait(25)];
    v.extend(tap(K::Letter('M')));
    v.extend([Until(AppState::Multiplayer), Wait(3)]);
    v.extend(tap(K::Letter('E')));
    v.extend([Wait(3)]);
    v.extend(click(100.0, 224.0));
    v.extend(tap(K::Enter));
    v.extend([Wait(3)]);
    v.extend(tap(K::Letter('C')));
    v.extend([Wait(3)]);
    v.extend(tap(K::Enter));
    v.extend([Wait(6), Shot("box")]);
    v.extend(click(480.0, 336.0));
    v.extend([Wait(3), Shot("list")]);
    // The list opens above its box (it has eight rows): the seventh is the second C+L option. One pick changes it and
    // the list stays open.
    v.extend(click(480.0, 293.0));
    v.extend([Wait(3), Shot("picked")]);
    v.extend(tap(K::Escape));
    v.extend(click(490.0, 246.0));
    v.extend([Wait(3), Shot("observers")]);
    v.extend(click(490.0, 246.0 + 20.0 + 2.0 * 20.0 + 6.0));
    v.extend([Wait(3), Shot("two-places")]);
    v
}

/// A farm placed: a peasant selected, the build commands by their keys, the placement image over good and bad ground,
/// then placed; the chat line edited and cancelled; the load dialog's list moved with the wheel.
fn build() -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5)];
    v.extend(click(416.0, 318.0));
    v.extend([Wait(3), Shot("peasant")]);
    v.extend(tap(K::Letter('B')));
    v.extend([Wait(3), Shot("build-basic")]);
    v.extend(tap(K::Letter('F')));
    v.extend([Wait(2), Move(330.0, 300.0), Wait(3), Shot("placing")]);
    v.extend([Move(250.0, 120.0), Wait(3), Shot("placing-bad")]);
    v.extend(click(330.0, 300.0));
    v.extend([Wait(3), Shot("placed"), Wait(120), Shot("building")]);
    v.extend(tap(K::Enter));
    v.extend([Wait(2), Type("abc"), Wait(1)]);
    v.extend(tap(K::Backspace));
    v.extend([Wait(2), Shot("chat-edited")]);
    v.extend(tap(K::Escape));
    v.extend([Wait(2), Shot("chat-closed")]);
    v.extend([Wheel(1.0), Wait(1), Wheel(-1.0), Wait(2), Shot("wheel-on-map")]);
    v.extend(tap(K::F(11)));
    v.extend([Wait(3), Type("two"), Wait(1)]);
    v.extend(tap(K::Enter));
    v.extend([Wait(3)]);
    v.extend(tap(K::F(11)));
    v.extend([Wait(3), Type("three"), Wait(1)]);
    v.extend(tap(K::Enter));
    v.extend([Wait(3)]);
    v.extend(tap(K::F(12)));
    v.extend([
        Wait(3),
        Shot("load-list"),
        Wheel(-1.0),
        Wait(2),
        Shot("load-wheel-down"),
    ]);
    v.extend([Wheel(1.0), Wait(2), Shot("load-wheel-up")]);
    v.extend(tap(K::Escape));
    v.extend([Wait(3), Shot("load-closed")]);
    v
}

/// The first-run screen over the demo backend (`firstrun::DemoBackend`): the list, the keyboard moving the focus, the
/// pointer hovering a button, a dropped folder the import refuses, the way back, and a click on an install that imports.
fn first_run() -> Vec<Cmd> {
    use Cmd::*;
    let demo = crate::firstrun::FirstRun::with_candidates(
        std::sync::Arc::new(crate::firstrun::DemoBackend),
        crate::firstrun::DemoBackend::candidates(),
        false,
    );
    let at = |item| demo.centre_of(item).expect("the demo screen lists it");
    let (installer, first) = (
        at(crate::firstrun::Item::Installer),
        at(crate::firstrun::Item::Candidate(0)),
    );
    let mut v = vec![Move(0.0, 0.0), Wait(2), Shot("list")];
    v.extend(tap(Key::Down));
    v.extend([Wait(1), Shot("second-selected")]);
    v.extend([Move(installer.0, installer.1), Wait(1), Shot("hover-installer")]);
    v.extend([Drop("/downloads/bad-folder"), Wait(2), Shot("error")]);
    v.extend(tap(Key::Enter));
    v.extend([Wait(2), Shot("back-to-list")]);
    v.extend(click(first.0, first.1));
    v.extend([Wait(2), Shot("imported")]);
    v
}

/// Carried goods and the ground marker: a peasant sent to a tree fells it and walks home carrying lumber, then is given
/// a move on open ground with a right-click, whose marker grows, holds and shrinks.
fn carry() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5)];
    // A peasant sent to a tree fells it and walks home with lumber.
    v.extend(click(416.0, 318.0));
    v.extend([
        Wait(3),
        Move(565.0, 85.0),
        Down(Button::Right),
        Wait(1),
        Up(Button::Right),
    ]);
    v.extend([UntilCycle(1400), Shot("carrying-lumber")]);
    // A move click on open ground: the marker grows, holds and shrinks while the load is carried.
    v.extend([Move(470.0, 330.0), Down(Button::Right), Wait(1), Up(Button::Right)]);
    v.extend([
        Wait(3),
        Shot("marker-grows"),
        Wait(10),
        Shot("marker-holds"),
        Wait(14),
        Shot("marker-shrinks"),
    ]);
    v.extend([Wait(20), Shot("carrying-walk")]);
    // The move command's target mode, then a left click on the minimap inside the view's rectangle: the marker goes at
    // the clicked tile's top-left pixel.
    v.extend(tap(Key::Letter('M')));
    v.extend([Wait(2), Move(90.0, 60.0), Wait(2), Shot("minimap-target")]);
    v.extend(click(90.0, 60.0));
    v.extend([Wait(8), Shot("minimap-target-marker")]);
    v
}

/// The optional improvements on: bars over every unit, the clock and actions per minute, the idle-worker button and
/// its key (each press selects a worker and sends a normal select, which the actions count).
fn extras() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5), Shot("bars-always")];
    v.extend([UntilCycle(200), Shot("clock")]);
    v.extend(click(564.0, 8.0));
    v.extend([Wait(3), Shot("idle-button")]);
    v.extend(tap(Key::F(6)));
    v.extend([Wait(3), Shot("idle-key")]);
    v.extend(click(416.0, 318.0));
    v.extend([
        Wait(3),
        Move(565.0, 85.0),
        Down(Button::Right),
        Wait(1),
        Up(Button::Right),
    ]);
    v.extend([UntilCycle(500), Shot("apm")]);
    v
}

/// A control group set from the selection with Ctrl and the number key.
fn set_group(d: u8) -> Vec<Cmd> {
    let mut v = vec![Cmd::Press(Key::ControlLeft), Cmd::Wait(1)];
    v.extend(tap(Key::Digit(d)));
    v.extend([Cmd::Release(Key::ControlLeft), Cmd::Wait(2)]);
    v
}

/// The control-group bar: three groups made from clicks and a box, the strip, a click on its second button.
fn extras_groups() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5), Shot("none")];
    v.extend(click(511.0, 248.0));
    v.extend([Wait(2)]);
    v.extend(set_group(1));
    v.extend(click(416.0, 318.0));
    v.extend([Wait(2)]);
    v.extend(set_group(2));
    v.extend([
        Move(260.0, 225.0),
        Down(Button::Left),
        Wait(1),
        Move(540.0, 340.0),
        Wait(1),
        Up(Button::Left),
    ]);
    v.extend([Wait(2)]);
    v.extend(set_group(5));
    v.extend(click(300.0, 400.0));
    v.extend([Wait(3), Shot("bar")]);
    // The second button is group 2.
    v.extend(click(250.0, 27.0));
    v.extend([Wait(3), Shot("recalled")]);
    v
}

/// Smart attack-move: a unit selected, a right-click on the ground, and the same with a worker.
fn extras_attack() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5)];
    v.extend(click(511.0, 248.0));
    v.extend([Wait(2), Shot("selected")]);
    v.extend([Move(330.0, 300.0), Down(Button::Right), Wait(1), Up(Button::Right)]);
    v.extend([Wait(3), Shot("attack-move")]);
    v.extend(click(416.0, 318.0));
    v.extend([
        Wait(2),
        Move(330.0, 250.0),
        Down(Button::Right),
        Wait(1),
        Up(Button::Right),
    ]);
    v.extend([Wait(3), Shot("worker-moves")]);
    v
}

/// Rally points and the production queue: look around, then select the starting building.
fn extras_sim() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5)];
    v.extend(click(395.0, 435.0));
    v.extend([Wait(2), Shot("selected")]);
    v.extend([Move(330.0, 330.0), Down(Button::Right), Wait(1), Up(Button::Right)]);
    v.extend([Wait(3), Shot("rally")]);
    // Peasants from the Town Hall's first button: one trains, the others wait in the queue, paid for already.
    for _ in 0..4 {
        v.extend(click(33.0, 358.0));
        v.push(Wait(2));
    }
    v.extend([Wait(3), Shot("queued")]);
    v
}

/// A key tapped with Ctrl held.
fn ctrl_tap(k: Key) -> Vec<Cmd> {
    let mut v = vec![Cmd::Press(Key::ControlLeft), Cmd::Wait(1)];
    v.extend(tap(k));
    v.extend([Cmd::Release(Key::ControlLeft), Cmd::Wait(2)]);
    v
}

/// Rebound keys: J gives the Move command (then a click on the ground), the old pause key does nothing and the new one
/// pauses, and the idle-worker key selects a worker.
fn extras_keys() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5)];
    v.extend(click(511.0, 248.0));
    v.extend([Wait(2)]);
    v.extend(tap(Key::Letter('J')));
    v.extend([Wait(2), Shot("move-mode")]);
    v.extend(click(330.0, 300.0));
    v.extend([Wait(3), Shot("moved")]);
    v.extend(ctrl_tap(Key::Letter('P')));
    v.extend([Wait(3), Shot("old-pause-key")]);
    v.extend(ctrl_tap(Key::Letter('U')));
    v.extend([Wait(3), Shot("paused")]);
    v.extend(ctrl_tap(Key::Letter('U')));
    v.extend(ctrl_tap(Key::Letter('I')));
    v.extend([Wait(3), Shot("idle-key")]);
    v
}

/// Health bars in the `hovered` mode: none until the pointer is over a unit, then that unit's, then none again.
fn extras_bars() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5), Shot("none")];
    v.extend([
        Move(416.0, 318.0),
        Wait(3),
        Shot("hovered"),
        Move(300.0, 200.0),
        Wait(3),
        Shot("away"),
    ]);
    v
}

/// A game on the 864 x 480 screen: the start, a selection box dragged past where the 640 screen ended, the view scrolled
/// by the pointer at the new right edge, and the game menu.
fn widescreen() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![Move(400.0, 240.0), Until(AppState::Game), Wait(5), Shot("start")];
    v.extend([
        Move(260.0, 225.0),
        Down(Button::Left),
        Wait(1),
        Move(780.0, 360.0),
        Wait(1),
        Shot("box"),
        Up(Button::Left),
        Wait(3),
        Shot("selected"),
    ]);
    // The pointer on the far right edge of the 864 pixels scrolls the view.
    v.extend([Move(863.0, 240.0), Wait(40), Shot("scrolled")]);
    v.extend([Move(500.0, 240.0), Wait(2)]);
    v.extend(tap(Key::F(10)));
    v.extend([Wait(4), Move(432.0, 200.0), Wait(2), Shot("menu")]);
    v
}

/// The 768 x 480 screen: the start and a box over the view's whole width.
fn widescreen_16x10() -> Vec<Cmd> {
    use Cmd::*;
    vec![
        Move(400.0, 240.0),
        Until(AppState::Game),
        Wait(5),
        Shot("start"),
        Move(200.0, 30.0),
        Down(Button::Left),
        Wait(1),
        Move(740.0, 440.0),
        Wait(1),
        Shot("box"),
        Up(Button::Left),
        Wait(2),
    ]
}

pub fn scene(name: &str) -> Option<Scene> {
    scenes().into_iter().find(|s| s.name == name)
}

/// The run of a scene.
pub struct Script {
    pub scene: Scene,
    next: usize,
    waiting: Option<u32>,
    /// Frames run so far.
    pub frame: u32,
    pointer: Vec2,
    /// The shots asked for this frame.
    pub shots: Vec<&'static str>,
    /// All commands have run.
    pub done: bool,
}

impl Script {
    pub fn new(scene: Scene) -> Self {
        Self {
            scene,
            next: 0,
            waiting: None,
            frame: 0,
            pointer: Vec2::new(320.0, 240.0),
            shots: Vec::new(),
            done: false,
        }
    }
}

/// A scripted run: the scene, what it saw, where it writes, and the scene's pointer.
pub struct Run {
    pub script: Script,
    pub record: Record,
    pub out: Out,
    /// The scene's pointer in the window's logical points, where the system's would be (set by [`drive`]).
    pointer: Option<(f32, f32)>,
    /// The fingerprints are written.
    finished: bool,
    /// What the run saw happen (`<scene>-events.txt`).
    pub events: Events,
}

/// What a scripted run saw happen, written as it happens to `<scene>-events.txt` in the output folder, one line each:
/// `state NAME` when the screen changes, `msg TEXT` when a message is shown (chat, notices), and `game ...` as each game
/// starts (the local slot, the rules, the options in force). Network scenes are checked by these lines (`tests/mp_e2e.rs`), as two
/// programs' frames never come out the same twice.
#[derive(Default)]
pub struct Events {
    file: Option<std::path::PathBuf>,
    /// Every line so far.
    pub lines: Vec<String>,
    state: Option<AppState>,
    /// The game's line is written.
    game: bool,
}

impl Events {
    fn note(&mut self, line: String) {
        if let Some(path) = &self.file {
            use std::io::Write;
            let written = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| writeln!(f, "{line}"));
            if let Err(e) = written {
                eprintln!("can't write {}: {e}", path.display());
            }
        }
        self.lines.push(line);
    }

    /// Whether a message containing `text` has shown.
    fn saw_message(&self, text: &str) -> bool {
        self.lines
            .iter()
            .any(|l| l.strip_prefix("msg ").is_some_and(|m| m.contains(text)))
    }
}

/// Notes what changed since the last frame in the run's events.
fn note_events(c: &mut Core, state: AppState) {
    let noted = c.script.as_ref().is_none_or(|r| r.events.game);
    let game = match (&c.sim, state) {
        (Some(sim), AppState::Game) if !noted => {
            let w = &sim.world;
            let cl = crate::extras::describe_lobby(&crate::game::extras(c).lobby_values());
            let names: Vec<String> = (0..8u8).map(|p| sim.session.player_name(p)).collect();
            Some(format!(
                "game slot={} rules={} multiplayer={} observing={} network={} sim-extras=[{}] cl-extras=[{}] names=[{}]",
                sim.human,
                if w.rules.enhanced { "enhanced" } else { "classic" },
                w.rules.multiplayer,
                w.observing,
                sim.session.is_network(),
                w.rules.extras.describe().join(", "),
                cl.join(", "),
                names.join(", "),
            ))
        }
        _ => None,
    };
    let Some(run) = &mut c.script else { return };
    let ev = &mut run.events;
    if ev.state != Some(state) {
        ev.state = Some(state);
        ev.note(format!("state {state:?}"));
        // Each game gets its line.
        ev.game = false;
    }
    if let Some(line) = game
        && !ev.game
    {
        ev.game = true;
        ev.note(line);
    }
}

/// A message is added to the message list or the notice line (`game::update_messages`): a scripted run notes it, even
/// one the next replaces in the same frame.
pub(crate) fn note_message(run: &mut Option<Run>, text: &str) {
    if let Some(run) = run {
        run.events.note(format!("msg {text}"));
    }
}

impl Run {
    /// A run of `scene` that writes to `out`.
    pub fn new(scene: Scene, out: Out) -> Run {
        if let Err(e) = std::fs::create_dir_all(&out.dir) {
            eprintln!("can't make {}: {e}", out.dir.display());
        }
        let events = out.dir.join(format!("{}-events.txt", scene.name));
        let _ = std::fs::remove_file(&events);
        Run {
            script: Script::new(scene),
            record: Record::default(),
            out,
            pointer: None,
            finished: false,
            events: Events {
                file: Some(events),
                ..Events::default()
            },
        }
    }

    /// The scene's pointer in the window's logical points.
    pub fn pointer(&self) -> Option<(f32, f32)> {
        self.pointer
    }
}

/// One frame of the scene, at the frame's start: runs its commands until one waits, and replaces the system's input
/// with the scene's. `window`: the window, without which the scene waits.
pub(crate) fn drive(c: &mut Core, window: Option<WindowGeom>) {
    c.input.clear_events();
    let Some(w) = window else { return };
    let state = c.states.get();
    note_events(c, state);
    let tick = c.sim.as_ref().map(|s| s.world.tick);
    let outcome = c.dialog.is_some();
    let mp = c.screens.mp_progress();
    let Core {
        script: Some(run),
        input,
        touch,
        ..
    } = c
    else {
        return;
    };
    let events = &run.events;
    let s = &mut run.script;
    s.frame += 1;
    s.shots.clear();
    let key = |key: Key, pressed: bool, text: Option<&str>| KeyEvent {
        key,
        pressed,
        text: text.map(Into::into),
    };
    while let Some(cmd) = s.scene.cmds.get(s.next).cloned() {
        match cmd {
            Cmd::Wait(n) => {
                let left = s.waiting.unwrap_or(n);
                if left > 0 {
                    s.waiting = Some(left - 1);
                    break;
                }
                s.waiting = None;
            }
            Cmd::Until(want) => {
                if state != want {
                    break;
                }
            }
            Cmd::UntilCycle(n) => {
                if tick.is_none_or(|t| t < n) {
                    break;
                }
            }
            Cmd::UntilOutcome => {
                if !outcome {
                    break;
                }
            }
            Cmd::UntilGames(n) => {
                if mp.games < n {
                    break;
                }
            }
            Cmd::UntilLobby(humans, watchers) => {
                if mp.lobby.is_none_or(|l| l.humans < humans || l.watchers < watchers) {
                    break;
                }
            }
            Cmd::UntilWatchable => {
                if !mp.watchable {
                    break;
                }
            }
            Cmd::UntilStartable => {
                if !mp.lobby.is_some_and(|l| l.startable) {
                    break;
                }
            }
            Cmd::UntilMessage(text) => {
                if !events.saw_message(text) {
                    break;
                }
            }
            Cmd::Move(x, y) => s.pointer = Vec2::new(x, y),
            Cmd::Down(b) => input.button(b, true),
            Cmd::Up(b) => input.button(b, false),
            Cmd::Press(k) => input.key(key(k, true, None)),
            Cmd::Release(k) => input.key(key(k, false, None)),
            Cmd::Type(text) => {
                // A key the system doesn't name, as a typed character comes.
                for c in text.chars() {
                    let t = c.to_string();
                    input.key(key(Key::Other(0), true, Some(&t)));
                    input.key(key(Key::Other(0), false, None));
                }
            }
            Cmd::Wheel(y) => input.wheel(Wheel { y, lines: true }),
            Cmd::Drop(path) => input.dropped(path.into()),
            Cmd::TouchDevice => touch.touch_only = true,
            Cmd::Finger(id, phase, x, y) => {
                let lb = w.letterbox();
                let at = |p: f32, offset: u32| (offset as f32 + p * lb.scale) / w.factor;
                input.touch(Touch {
                    id,
                    phase,
                    pos: (at(x, lb.offset.0), at(y, lb.offset.1)),
                });
            }
            // A shot is this frame as the input so far left it: the commands after it wait for the next frame.
            Cmd::Shot(name) => {
                s.shots.push(name);
                s.next += 1;
                break;
            }
        }
        s.next += 1;
    }
    s.done = s.next >= s.scene.cmds.len();
    // The pointer in the window's logical points, where the system's would be (`display::Letterbox`).
    let p = s.pointer;
    let lb = w.letterbox();
    let f = w.factor;
    let at = |p: f32, offset: u32| (offset as f32 + p * lb.scale) / f;
    run.pointer = Some((at(p.x, lb.offset.0), at(p.y, lb.offset.1)));
}

/// FNV-1a, 64 bits: the frame and sound fingerprints.
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// What a scripted run saw: the frame fingerprints (one line per run of equal frames), the shots' and the sounds'.
#[derive(Default)]
pub struct Record {
    lines: Vec<String>,
    /// The current run of equal frames: its first frame and fingerprint.
    run: Option<(u32, u64)>,
    last: u32,
    /// The scene has run all its commands.
    ended: bool,
    /// The shots' composed frames, until the window's screenshot of the same frame arrives (with a window).
    pending: Vec<(&'static str, Frame)>,
    /// Shots compared with the window's screenshot: name, differing pixels.
    pub compared: Vec<(&'static str, usize)>,
    /// The mixed sound's fingerprints (the audio snapshot).
    sound: crate::audio::Fingerprints,
    /// The kinds of sound in the order the run first started one of each (`start_lines`).
    kinds: Vec<u8>,
}

impl Record {
    /// The sound lines of the sounds started in frame `n`: in the order of their kinds' first starts in the run, then
    /// in the order they started (the order Bevy's query listed its sound entities in: by archetype, then by spawn).
    fn start_lines(&mut self, n: u32, starts: Vec<crate::audio::Start>) -> Vec<String> {
        use crate::audio::Group;
        let kind = |g: Group| match g {
            Group::Music => 0,
            Group::Effect => 1,
            Group::Screen(_) => 2,
            Group::Act => 3,
        };
        for s in &starts {
            if !self.kinds.contains(&kind(s.group)) {
                self.kinds.push(kind(s.group));
            }
        }
        let mut starts = starts;
        starts.sort_by_key(|s| self.kinds.iter().position(|&k| k == kind(s.group)));
        starts
            .iter()
            .map(|s| format!("sound\t{n}\t{:016x}\t{:.4}\t{:?}", s.hash, s.volume, s.mode))
            .collect()
    }

    fn frame(&mut self, n: u32, hash: u64) {
        match self.run {
            Some((_, h)) if h == hash => {}
            Some((from, h)) => {
                self.lines.push(format!("frames\t{from}-{}\t{h:016x}", self.last));
                self.run = Some((n, hash));
            }
            None => self.run = Some((n, hash)),
        }
        self.last = n;
    }

    /// The fingerprint file: frames, shots and sounds in the order they happened.
    pub fn text(&self, scene: &str) -> String {
        let mut out = format!("# frame and sound fingerprints of the scripted scene `{scene}` (hashes only)\n");
        for l in &self.lines {
            out.push_str(l);
            out.push('\n');
        }
        if let Some((from, h)) = self.run {
            out.push_str(&format!("frames\t{from}-{}\t{h:016x}\n", self.last));
        }
        out
    }
}

/// Where a scripted run writes, and whether it has a window.
#[derive(Clone)]
pub struct Out {
    pub dir: std::path::PathBuf,
    pub windowed: bool,
    /// Without a window: also save the shots' composed frames as pictures.
    pub pictures: bool,
    /// With `pictures`: also every this many frames (0: none).
    pub every: u32,
}

/// Notes the sounds started this frame in the core's mixer: the sound's bytes' fingerprint, its loudness and how it
/// plays.
pub(crate) fn note_sounds(c: &mut Core) {
    let Some(run) = &mut c.script else { return };
    let starts = c.audio.take_starts();
    let lines = run.record.start_lines(run.script.frame, starts);
    run.record.lines.extend(lines);
}

/// What [`record`] asks of the platform.
#[derive(Default)]
pub struct Recorded {
    /// The shots to take the window's own screenshot of (with a window), for [`compare_shot`].
    pub screenshots: Vec<&'static str>,
    /// The scene is done and every screenshot is in: the program ends.
    pub finished: bool,
}

/// Notes the fingerprint of the frame the window shows and keeps the scene's shots; with a window, asks for the
/// window's screenshot of the same frame. Ends the run when the scene is done and every screenshot is in, or when the
/// program is ending on its own (`exiting`: a quit key; the fingerprints so far are kept, marked).
///
/// `frame`: the composed screen as the window shows it at one window pixel per screen pixel, `None` without a window
/// or when the window shows it larger (the run's frames are then the platform's own). `output`: the core's mixer.
pub fn record(run: &mut Run, frame: Option<&Frame>, output: &crate::audio::Output, exiting: bool) -> Recorded {
    let mut asked = Recorded::default();
    let Some(frame) = frame else { return asked };
    let (n, shots, done) = (run.script.frame, run.script.shots.clone(), run.script.done);
    let hash = frame.hash();
    let out = run.out.clone();
    let rec = &mut run.record;
    // Frames after the scene's end (while screenshots come in) aren't part of it.
    if !rec.ended {
        rec.frame(n, hash);
        // The frame's share of the mixed sound: the sounds started in this frame start at its first sample.
        let mut samples = vec![0i16; 2 * crate::audio::frames_for(n as u64 - 1, (1.0 / FRAME_SECS).round() as u64)];
        output.mix(&mut samples);
        rec.sound.add(&samples);
    }
    rec.ended |= done;
    for &name in &shots {
        rec.lines.push(format!("shot\t{n}\t{name}\t{hash:016x}"));
    }
    if !out.windowed && out.pictures {
        for &name in &shots {
            save_png(&out.dir.join(format!("{name}-composed.png")), frame);
        }
        if out.every > 0 && n % out.every == 0 {
            save_png(&out.dir.join(format!("frame{n:05}.png")), frame);
        }
    }
    if out.windowed {
        for &name in &shots {
            rec.pending.push((name, frame.clone()));
            asked.screenshots.push(name);
        }
    }
    let finished = done && rec.pending.is_empty();
    if (finished || exiting) && !run.finished {
        let scene = run.script.scene.name;
        let mut text = rec.text(scene);
        text.push_str(&written_files(&out.dir.join("save")));
        if !finished {
            text.push_str("# the program ended before the scene did\n");
        }
        let file = out.dir.join(format!("{scene}.txt"));
        if let Err(e) = std::fs::create_dir_all(&out.dir).and_then(|_| std::fs::write(&file, text)) {
            eprintln!("can't write {}: {e}", file.display());
        }
        let file = out.dir.join(format!("{scene}-audio.txt"));
        if let Err(e) = std::fs::write(&file, rec.sound.text(scene)) {
            eprintln!("can't write {}: {e}", file.display());
        }
        if out.windowed {
            let report: String = rec.compared.iter().map(|(n, d)| format!("{n}\t{d}\n")).collect();
            let _ = std::fs::write(out.dir.join(format!("{scene}-compare.txt")), report);
        }
        run.finished = true;
        asked.finished = finished;
    }
    asked
}

/// A fingerprint line for each file the run wrote to its own folder (saved games, screenshots), by name.
fn written_files(dir: &std::path::Path) -> String {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    files
        .iter()
        .map(|name| {
            let bytes = std::fs::read(dir.join(name)).unwrap_or_default();
            format!("file\t{name}\t{:016x}\n", fnv64(&bytes))
        })
        .collect()
}

/// Compares the window's own screenshot of the shot `name` with the composed frame, and saves both (and the
/// differing pixels in white) as pictures in the output folder. `shot`: the screenshot's width, height and RGBA bytes,
/// or `None` if it couldn't be read.
pub fn compare_shot(run: &mut Run, name: &'static str, shot: Option<(usize, usize, &[u8])>) {
    let rec = &mut run.record;
    let Some(i) = rec.pending.iter().position(|(n, _)| *n == name) else {
        return;
    };
    let (_, cpu) = rec.pending.remove(i);
    let Some((w, h, gpu)) = shot else { return };
    // The screen is 640 x 480, or wider in a game with `widescreen`: the composed frame says.
    let (fw, fh) = (cpu.width, cpu.height);
    let lb = crate::display::Letterbox::fit_screen((w as u32, h as u32), (fw as u32, fh as u32));
    let (ox, oy) = (lb.offset.0 as usize, lb.offset.1 as usize);
    let mut diff = vec![0u8; fw * fh * 4];
    let mut gpu_screen = vec![0u8; fw * fh * 4];
    let mut differing = 0;
    for y in 0..fh {
        for x in 0..fw {
            let (gx, gy) = (ox + x, oy + y);
            let g = if gx < w && gy < h {
                let at = (gy * w + gx) * 4;
                [gpu[at], gpu[at + 1], gpu[at + 2], 255]
            } else {
                [0, 0, 0, 255]
            };
            let i = (y * fw + x) * 4;
            gpu_screen[i..i + 4].copy_from_slice(&g);
            let c = &cpu.rgba[i..i + 3];
            let same = c == &g[..3];
            if !same {
                differing += 1;
            }
            let v = if same { 0 } else { 255 };
            diff[i..i + 4].copy_from_slice(&[v, v, v, 255]);
        }
    }
    rec.compared.push((name, differing));
    // A window of another size (`TIDEWATER_SCRIPT_WINDOW`) also keeps the whole window: the chrome, the black bars,
    // the sharp fit.
    if (w, h) != (fw, fh) {
        save_png_sized(&run.out.dir.join(format!("{name}-full.png")), (w, h), gpu);
    }
    for (suffix, rgba) in [("window", gpu_screen), ("composed", cpu.rgba), ("diff", diff)] {
        save_png_sized(&run.out.dir.join(format!("{name}-{suffix}.png")), (fw, fh), &rgba);
    }
}

/// Saves a frame as a PNG file (the `png` crate's encoder).
fn save_png(path: &std::path::Path, frame: &Frame) {
    save_png_sized(path, (frame.width, frame.height), &frame.rgba);
}

/// Saves an RGBA picture of any size as a PNG file.
fn save_png_sized(path: &std::path::Path, (w, h): (usize, usize), rgba: &[u8]) {
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(file, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(rgba)?;
        Ok(())
    };
    if let Err(e) = write() {
        eprintln!("can't write {}: {e}", path.display());
    }
}

// --- Network scenes ------------------------------------------------------------------------------------------------
//
// Two or three programs, each running one of these, play one game through the menus (`tests/mp_e2e.rs`). They wait on
// each other only through what the screens show (`UntilGames`, `UntilLobby`, `UntilStartable`, `UntilMessage`), never
// on a count of frames, as the other program's timing is its own. Not in the frame snapshot: two programs' frames
// never come out the same twice.

/// How a network scene meets its game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    /// Enhanced rules, Local Network: the game list's beacon.
    Lan,
    /// Enhanced rules, Direct IP: the joiner types the address.
    Ip,
    /// Classic rules (opt-in), Local Network: the joiner ticks Ready.
    Classic,
    /// Enhanced rules, Local Network, the creator letting one observer in.
    Watched,
    /// Enhanced rules, Local Network, three players: the creator leaves first and the joiners play on with the first
    /// joiner as the new host (`docs/design/network.md`, "When the host leaves").
    Three,
    /// Enhanced rules, Local Network: the creator alone with one Computer row (Start without another human).
    Solo,
    /// Enhanced rules, Local Network: the creator, one joiner and one Computer row.
    Mixed,
}

/// The names the players type, so each can wait for the other's chat line.
const MP_HOST: &str = "Hosty";
const MP_JOIN: &str = "Joiner";
const MP_SECOND: &str = "Second";
const MP_WATCH: &str = "Watcher";

fn mp_scenes() -> Vec<Scene> {
    let scene = |name, cmds, extras| Scene {
        name,
        start: vec![Step::MainMenu],
        cmds,
        extras,
        keys: "",
    };
    // The creator turns a C+L option on (health bars), so the lobby's Extras box shows from the start.
    let host_extras: &[(&str, &str)] = &[("health-bars", "always")];
    vec![
        scene("mp-lan-host", mp_host(Way::Lan), host_extras),
        scene("mp-lan-join", mp_join(Way::Lan, Role::Joiner), &[]),
        scene("mp-ip-host", mp_host(Way::Ip), host_extras),
        scene("mp-ip-join", mp_join(Way::Ip, Role::Joiner), &[]),
        scene("mp-classic-host", mp_host(Way::Classic), host_extras),
        scene("mp-classic-join", mp_join(Way::Classic, Role::Joiner), &[]),
        scene("mp-watched-host", mp_host(Way::Watched), host_extras),
        scene("mp-watched-join", mp_join(Way::Watched, Role::Joiner), &[]),
        scene("mp-watch", mp_join(Way::Watched, Role::Watcher), &[]),
        scene("mp-three-host", mp_host(Way::Three), host_extras),
        scene("mp-three-join", mp_join(Way::Three, Role::Joiner), &[]),
        scene("mp-three-second", mp_join(Way::Three, Role::Second), &[]),
        scene("mp-solo-host", mp_host(Way::Solo), host_extras),
        scene("mp-mixed-host", mp_host(Way::Mixed), host_extras),
        scene("mp-mixed-join", mp_join(Way::Mixed, Role::Joiner), &[]),
    ]
}

/// From the main menu to the connection: Multi Player Game, the rules, the method, the name. Local Network ends on
/// the game list; Direct IP ends on its setup screen with the name still to give.
fn mp_menus(way: Way, name: &'static str) -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = vec![Move(320.0, 240.0), Until(AppState::MainMenu), Wait(25)];
    v.extend(tap(K::Letter('M')));
    v.extend([Until(AppState::Multiplayer), Wait(3)]);
    let give_name = |v: &mut Vec<Cmd>| {
        v.push(Wait(3));
        for _ in 0..12 {
            v.extend(tap(K::Backspace));
        }
        v.extend([Type(name), Wait(2)]);
        v.extend(tap(K::Enter));
    };
    match way {
        Way::Classic => {
            // Classic asks for the name first; its list has Direct IP and Local Network.
            v.extend(tap(K::Letter('C')));
            give_name(&mut v);
            v.extend([Wait(3)]);
            v.extend(click(100.0, 224.0));
            v.extend(tap(K::Letter('C')));
            v.extend([Wait(3)]);
        }
        Way::Lan | Way::Watched | Way::Three | Way::Solo | Way::Mixed => {
            v.extend(tap(K::Letter('E')));
            v.extend([Wait(3)]);
            v.extend(click(100.0, 242.0));
            v.extend(tap(K::Letter('C')));
            give_name(&mut v);
            v.extend([Wait(3)]);
        }
        Way::Ip => {
            v.extend(tap(K::Letter('E')));
            v.extend([Wait(3)]);
            v.extend(click(100.0, 224.0));
            v.extend(tap(K::Letter('C')));
            v.extend([Wait(3)]);
        }
    }
    v
}

/// Selects the units at the start (a box over the map view) and sends them somewhere, then back: commands that travel.
fn mp_orders() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = vec![
        Move(180.0, 20.0),
        Down(Button::Left),
        Wait(2),
        Move(630.0, 470.0),
        Wait(2),
        Up(Button::Left),
        Wait(2),
        Shot("selected"),
        Move(300.0, 120.0),
        Down(Button::Right),
        Wait(1),
        Up(Button::Right),
        Wait(30),
        Move(520.0, 380.0),
        Down(Button::Right),
        Wait(1),
        Up(Button::Right),
        Wait(3),
    ];
    // Ctrl+number keeps the selection as a group (a synced command too).
    v.extend([Press(Key::ControlLeft), Wait(1)]);
    v.extend(tap(Key::Digit(1)));
    v.extend([Release(Key::ControlLeft), Wait(2)]);
    v
}

/// Ctrl+Q and its confirmation: in a network game it quits to the main menu (`net/leave-and-drop.md`, LV17), and the
/// others see this player leave.
fn mp_leave() -> Vec<Cmd> {
    use Cmd::*;
    let mut v = ctrl(Key::Letter('Q'));
    v.extend([Wait(2), Shot("quit-confirm")]);
    v.extend(tap(Key::Enter));
    v.extend([Until(AppState::MainMenu), Wait(25), Shot("left")]);
    v
}

/// The creator's side: the lobby, a map picked, options set (Extras: a simulation option and a C+L one), the joiners
/// waited for, Start; in the game, orders, a chat line each way, then leaving (in Classic mode the joiner leaves).
fn mp_host(way: Way) -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let mut v = mp_menus(way, MP_HOST);
    match way {
        // Direct IP's Call creates the game: Connect, then the name.
        Way::Ip => {
            v.extend(tap(K::Letter('C')));
            v.push(Wait(3));
            for _ in 0..12 {
                v.extend(tap(K::Backspace));
            }
            v.extend([Type(MP_HOST), Wait(2)]);
            v.extend(tap(K::Enter));
        }
        // The game list's Create Game.
        _ => v.extend(tap(K::Letter('C'))),
    }
    v.extend([UntilLobby(1, 0), Wait(6), Shot("lobby-alone")]);
    // Select Scenario: the dialog, the next map down, OK.
    v.extend(tap(K::Letter('E')));
    v.extend([Wait(3), Shot("scenarios")]);
    v.extend(tap(K::Down));
    v.extend([Wait(2)]);
    v.extend(tap(K::Enter));
    v.extend([Wait(3), Shot("map-picked")]);
    // Extras: rally points (a simulation option) and smart attack-move (C+L) on, for everyone.
    v.extend(click(480.0, 336.0));
    v.extend([Wait(3)]);
    v.extend(click(480.0, 173.0));
    v.extend([Wait(2)]);
    v.extend(click(480.0, 293.0));
    v.extend([Wait(2), Shot("extras-list")]);
    v.extend(tap(K::Escape));
    v.push(Wait(2));
    if way == Way::Watched {
        // Observers: 1.
        v.extend(click(490.0, 246.0));
        v.push(Wait(3));
        v.extend(click(490.0, 246.0 + 20.0 + 20.0 + 6.0));
        v.push(Wait(3));
    }
    v.push(Shot("options"));
    if matches!(way, Way::Solo | Way::Mixed) {
        // Row 2 (the third) becomes a Computer: its name box, then the drop-down's second line.
        v.extend(click(100.0, 97.0));
        v.extend([Wait(3), Shot("row-list")]);
        v.extend(click(100.0, 141.0));
        v.extend([Wait(3), Shot("computer")]);
    }
    let watchers = usize::from(way == Way::Watched);
    let humans = match way {
        Way::Three => 3,
        Way::Solo => 1,
        _ => 2,
    };
    v.extend([
        UntilLobby(humans, watchers),
        UntilStartable,
        Wait(10),
        Shot("lobby-full"),
    ]);
    v.extend(tap(K::Letter('S')));
    v.extend([Until(AppState::Game), Move(400.0, 240.0), Wait(30), Shot("game")]);
    v.extend(mp_orders());
    v.extend([UntilCycle(200), Shot("cycle-200")]);
    if way == Way::Solo {
        // Nobody to talk to, nobody to wait for: the Computer plays and the cycles run.
        v.extend([UntilCycle(500), Shot("last-cycle")]);
        v.extend(mp_leave());
        return v;
    }
    v.extend(chat("hello from the host"));
    v.push(UntilMessage("Joiner: hello from the joiner"));
    if way == Way::Three {
        v.push(UntilMessage("Second: hello from the second"));
    }
    v.extend([Wait(2), Shot("chat")]);
    if way == Way::Classic {
        // Here the joiner leaves first; the creator, the only human left in, wins (LV1).
        v.extend([UntilMessage("Joiner left the game"), Wait(5), Shot("joiner-left")]);
        v.extend([UntilOutcome, Wait(5), Shot("outcome")]);
        return v;
    }
    if way == Way::Mixed {
        v.extend([UntilMessage("Joiner left the game"), Wait(5), Shot("joiner-left")]);
    }
    // Three players: the creator leaves first, and the joiners hand the game on and play on.
    let end = if way == Way::Three { 300 } else { 400 };
    v.extend([UntilCycle(end), Shot("last-cycle")]);
    v.extend(mp_leave());
    v
}

/// Who a joining scene is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// The first joiner.
    Joiner,
    /// The second joiner of a three-player game.
    Second,
    /// An observer (Watch Game).
    Watcher,
}

/// A joiner's side (or an observer's): the game found (the list, or the address typed), joined, Ready in Classic mode;
/// in the game, orders and a chat line answered, then a leave seen and the game ended or left.
fn mp_join(way: Way, role: Role) -> Vec<Cmd> {
    use Cmd::*;
    use Key as K;
    let name = match role {
        Role::Joiner => MP_JOIN,
        Role::Second => MP_SECOND,
        Role::Watcher => MP_WATCH,
    };
    let mut v = mp_menus(way, name);
    if way == Way::Ip {
        // Answer, the address, Connect, the name: the game at the address is the list's.
        v.extend(click(130.0, 376.0));
        v.push(Wait(2));
        v.extend(click(130.0, 410.0));
        v.push(Wait(2));
        v.extend(click(200.0, 342.0));
        v.extend([Wait(2), Type("127.0.0.1"), Wait(2), Shot("address")]);
        v.extend(tap(K::Enter));
        v.push(Wait(3));
        for _ in 0..12 {
            v.extend(tap(K::Backspace));
        }
        v.extend([Type(name), Wait(2)]);
        v.extend(tap(K::Enter));
    }
    v.extend([UntilGames(1), Wait(4), Shot("games")]);
    if role == Role::Watcher {
        // Watch Game shows once the creator has made a place for an observer.
        v.extend([UntilWatchable, Wait(2)]);
        v.extend(tap(K::Letter('W')));
        v.extend([UntilLobby(2, 1), Wait(6), Shot("lobby")]);
    } else {
        v.extend(tap(K::Letter('J')));
        v.extend([UntilLobby(2, 0), Wait(6), Shot("lobby")]);
    }
    if way == Way::Classic {
        // The Ready box of the joiner's row (the second).
        v.extend(click(19.0, 82.0));
        v.extend([Wait(3), Shot("ready")]);
    }
    v.extend([Until(AppState::Game), Move(400.0, 240.0), Wait(30), Shot("game")]);
    match role {
        Role::Watcher => {
            v.extend([UntilMessage("Joiner: hello from the joiner"), Wait(2), Shot("chat")]);
            v.extend([UntilMessage("Hosty left the game"), Wait(5), Shot("host-left")]);
            v.extend(mp_leave());
            return v;
        }
        Role::Joiner => {
            v.extend(mp_orders());
            v.extend([UntilMessage("Hosty: hello from the host"), Wait(2)]);
            v.extend(chat("hello from the joiner"));
        }
        Role::Second => {
            v.extend(mp_orders());
            v.extend([UntilMessage("Hosty: hello from the host"), Wait(2)]);
            v.extend(chat("hello from the second"));
        }
    }
    v.extend([Wait(4), Shot("chat")]);
    if way == Way::Lan {
        // The game menu open for a while: a network game goes on underneath (`ui/game-menu.md`, "Multiplayer"), so
        // these cycles pass here and the creator never waits.
        v.extend(tap(K::F(10)));
        v.extend([Wait(3), Shot("menu-open"), UntilCycle(330), Shot("menu-later")]);
        v.extend(tap(K::Escape));
        v.push(Wait(3));
    }
    // In Classic mode this joiner leaves first.
    if way == Way::Classic {
        v.extend([UntilCycle(300), Shot("last-cycle")]);
        v.extend(mp_leave());
        return v;
    }
    if way == Way::Three {
        // The creator leaves first: the first joiner takes over as host, and both joiners give orders and talk again.
        v.extend([
            UntilMessage("Hosty left the game"),
            UntilMessage("Joiner is now the host."),
        ]);
        v.extend([Wait(5), Shot("host-left")]);
        v.extend(mp_orders());
        if role == Role::Joiner {
            v.extend(chat("still here"));
            v.extend([UntilMessage("Second: still here too"), Wait(2), Shot("chat-after")]);
            // Then the new host leaves, and the second, the only human left in, wins.
            v.extend([UntilCycle(650), Shot("last-cycle")]);
            v.extend(mp_leave());
            return v;
        }
        v.extend([UntilMessage("Joiner: still here"), Wait(2)]);
        v.extend(chat("still here too"));
        v.extend([Wait(4), Shot("chat-after")]);
        v.extend([UntilMessage("Joiner left the game"), Wait(5), Shot("joiner-left")]);
        v.extend([UntilOutcome, Wait(5), Shot("outcome")]);
        return v;
    }
    if way == Way::Mixed {
        // The Computer is still in the game, so no victory: the joiner leaves, and the creator sees it and leaves later.
        v.extend([UntilCycle(330), Shot("last-cycle")]);
        v.extend(mp_leave());
        return v;
    }
    // The creator leaves; the joiner, the only human left in, wins at the next victory check (LV1).
    v.extend([UntilMessage("Hosty left the game"), Wait(5), Shot("host-left")]);
    v.extend([UntilOutcome, Wait(5), Shot("outcome")]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_has_shots_and_a_unique_name() {
        let all = scenes();
        for s in &all {
            assert!(s.cmds.iter().any(|c| matches!(c, Cmd::Shot(_))), "{}", s.name);
            assert_eq!(all.iter().filter(|o| o.name == s.name).count(), 1);
        }
        for name in ["campaign", "outcome", "ending", "load", "custom"] {
            assert!(scene(name).is_some(), "{name}");
        }
    }

    #[test]
    fn fnv_is_the_published_function() {
        // FNV-1a 64 test vectors (the FNV authors' published table).
        assert_eq!(fnv64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
