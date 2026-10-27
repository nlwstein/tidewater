//! The command line (`clap`).

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "tidewater", about = "Tidewater, the Warcraft II: Battle.net Edition game client", version = crate::update::VERSION)]
pub struct Args {
    /// Development override: read the Warcraft II: Battle.net Edition files in place from this folder instead of the
    /// copy made by `--import`.
    #[arg(long, env = "TIDEWATER_INSTALL")]
    pub install: Option<PathBuf>,
    /// Import the game files from a game folder (or a folder of `.mpq` archives, or one archive): check them, copy
    /// them into Tidewater's data directory, print a summary and exit. The exit code tells what went wrong (10 path
    /// not found, 11 DOS edition, 12 installer, 13 no game files, 14 no `War2Dat.mpq`, 15 damaged archive, 16
    /// incomplete files, 17 no space, 18 i/o error, 19 CD image).
    #[arg(long, value_name = "PATH")]
    pub import: Option<PathBuf>,
    /// Open the import screen even though game files are already imported, to import again (a second import replaces
    /// the first). Esc leaves without changing anything.
    #[arg(long, conflicts_with_all = ["import", "install"])]
    pub reimport: bool,
    /// Tidewater's data directory (the imported game files in `game`, saved games in `Save`, the settings file).
    /// Without it, the per-user data directory.
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
    /// A .pud file path, or a name inside the archives; it starts at once. Without one the program starts with its
    /// startup videos and title (`docs/specs/game/campaign-flow.md`).
    pub map: Option<String>,
    /// Record the game as played: a `.twrep` file in the `replays` folder of Tidewater's user data directory (the
    /// `--save-dir` folder if one is given), written as the game goes and when it ends. Off by default. Not for a
    /// loaded game or a run with a `--demo-*` option; those can't be replayed from their start.
    #[arg(long)]
    pub record_replay: bool,
    /// Play back a recorded game (a `.twrep` file) as the recording player saw it. Input is ignored except the game
    /// menu, pause (holds the playback) and speed (plays faster or slower). Stops with a message at the first
    /// checkpoint that doesn't match the recording. The map is found as the recording named it, in the archives or
    /// by that name in the current folder.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["map", "record_replay", "setup"])]
    pub replay: Option<PathBuf>,
    /// Development: enable the mod in this folder name of `<data>/mods` for this run, after the mods the settings enable
    /// (repeat for several; later ones override earlier ones). `docs/mods.md`.
    #[arg(long = "mod", value_name = "NAME")]
    pub mods: Vec<String>,
    /// Play a `--replay` even when it was recorded with other game files or mods than these (it will most likely go out
    /// of sync; without this the game stops and says what differs).
    #[arg(long, requires = "replay")]
    pub force_replay: bool,
    /// Host a network game of MAP on PORT (6112 without one) and wait for the players of `--lobby` to `--join` it
    /// (`docs/design/network.md`). Write the map before it (`tidewater MAP --host`): a bare `--host` followed by a
    /// word would read the word as a port. The lobby's other choices are `--setup` and `--lobby`.
    #[arg(long, value_name = "PORT", num_args = 0..=1, default_missing_value = "6112", requires = "map", conflicts_with_all = ["join", "replay", "goto"])]
    pub host: Option<u16>,
    /// Join the network game hosted at ADDR (`HOST` or `HOST:PORT`). The map and the lobby come from the host.
    #[arg(long, value_name = "ADDR", conflicts_with_all = ["map", "setup", "lobby", "replay", "goto"])]
    pub join: Option<String>,
    /// With `--host`, the lobby's choices, comma separated: `humans=2` (2 to 8), `races=human/orc` (one per human, or
    /// `map`), `computers=N`, `fixed` (Fixed Order), `observers=0..4` (places for watchers, none by default),
    /// `speed=0..8` (8, the fastest, by default), `row=0..15` (the network delay: 3, a turn of 8 cycles at speed 8, by
    /// default; 4 gives 12), `seed=N` and `extras=NAME=VALUE/NAME=VALUE` (the client-only options that give an edge,
    /// such as `extras=health-bars=always/widescreen=16:9`, which every player then plays with; the host's own settings
    /// stand for those not named). Classic rules, which are opt-in, and fog of war are
    /// `--setup custom:classic,fog=off`.
    #[arg(long, value_name = "SPEC", value_parser = crate::net::lobby::LobbySpec::parse, requires = "host")]
    pub lobby: Option<crate::net::lobby::LobbySpec>,
    /// With `--join`, watch the game instead of playing it: the host must have a place for an observer
    /// (`--lobby observers=1`). An observer runs the game, sees the whole map and gives no commands.
    #[arg(long, requires = "join")]
    pub observe: bool,
    /// With `--host` or `--join`, the name this player goes by in the game.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// Development aid: open a screen of the campaign flow: `title`, `menu`, `campaign`, `custom` (the Custom Scenario screen), `multi` (the multiplayer menus), `mission:E`, `act:E`,
    /// `briefing:E`, `score-victory:E` or `score-defeat:E` (synthetic statistics), `ending:E`, `credits:tod`,
    /// `credits:bdp` (the credits page after a finale), `show-credits` (the main menu's credits screen) or `video:N`,
    /// with E a mission entry (0 to 51).
    #[arg(long)]
    pub goto: Option<String>,
    /// Don't play the intro video at startup (the "play the intro" preference bit cleared).
    #[arg(long)]
    pub no_intro: bool,
    /// Development aid: the game ends at once with `victory` or `defeat` (to see the dialog and the score screen).
    #[arg(long, value_parser = ["victory", "defeat"])]
    pub demo_outcome: Option<String>,
    /// Game setup options: `campaign:ENTRY`, or `custom` with choices such as
    /// `custom:race=orc,opponents=2,resources=high,terrain=swamp,units=one-worker,fixed-starts,fog=off`.
    /// By default a campaign map starts as its campaign mission and any other map as a custom game.
    #[arg(long, value_parser = tidewater_sim::setup::Setup::parse)]
    pub setup: Option<tidewater_sim::setup::Setup>,
    /// The folder for saved games. Without it, a `Save` folder in Tidewater's own user data directory; the program
    /// never writes into the installation.
    #[arg(long)]
    pub save_dir: Option<PathBuf>,
    /// Development aid, not part of the original: the mouse wheel zooms the map view, in powers of two from 1/4 to 8
    /// map pixels per screen pixel. Without it the map view shows one map pixel per screen pixel, as the original.
    #[arg(long)]
    pub free_zoom: bool,
    /// Development aid, not part of the original: the starting zoom in map pixels per screen pixel (2 shows twice as
    /// much map), taken to the nearest power of two from 1/4 to 8. Implies `--free-zoom`.
    #[arg(long)]
    pub zoom: Option<f32>,
    /// The window size in logical points (the system's "looks like" size) per screen pixel: N opens a 640N × 480N
    /// window. Without it, the window is the largest whole number of physical pixels per screen pixel that fits the
    /// monitor (on a display with 2 pixels per point, 3 gives a 960 × 720-point window). Either way the 640 × 480
    /// screen is drawn at the largest whole number of physical pixels that fits the window, with black around it.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=8))]
    pub ui_scale: Option<u32>,
    /// Development aid: order all of your units to tile `X,Y` at the start.
    #[arg(long, value_parser = parse_tile)]
    pub demo_move: Option<(i32, i32)>,
    /// Development aid: send every player's units against the nearest enemy within 16 tiles at the start.
    #[arg(long)]
    pub demo_fight: bool,
    /// Development aid: select your first N units at the start and centre the view on them.
    #[arg(long)]
    pub demo_select: Option<usize>,
    /// Development aid: with `--demo-select`, select only units of this type number (any owner).
    #[arg(long)]
    pub demo_type: Option<u8>,
    /// Development aid: with `--demo-select`, start in placement mode for this building type number.
    #[arg(long)]
    pub demo_place: Option<u8>,
    /// Development aid: with `--demo-place`, the pointer's tile `X,Y` while the pointer is off the map view, and one
    /// placement click there at the start.
    #[arg(long, value_parser = parse_tile)]
    pub demo_site: Option<(i32, i32)>,
    /// Don't lock the pointer to the window during a game (not part of the original, which runs full screen; see
    /// `display`, "Mouse lock"). Cmd (the Windows key elsewhere) frees it while held, and switching away frees it.
    #[arg(long)]
    pub no_mouse_lock: bool,
    /// Touch emulation for a desktop (not part of the original; `docs/design/touch.md`): the mouse is one finger, and
    /// the touch controls show beside the picture. Hold Alt as the button goes down for a second finger mirrored
    /// through the picture's centre (pinch, pan), Alt+Shift for one that moves beside the first (two-finger pan),
    /// Alt+Ctrl for a third (a three-finger tap opens the game menu).
    #[arg(long)]
    pub touch_emulation: bool,
    /// The screen's density in physical pixels per inch, for the touch sizes (taps, slop, controls). Without it the
    /// shell's best guess: 140 points to the inch (160 on a phone or tablet).
    #[arg(long)]
    pub touch_dpi: Option<f32>,
    /// The Preferences dialog's second mouse style: right click centres the view instead of giving a command.
    #[arg(long)]
    pub second_mouse_style: bool,
    /// The speed dialog's mouse scroll speed, 0 to 6 (fastest). Without it, the stored setting.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=6))]
    pub mouse_scroll_speed: Option<u8>,
    /// The speed dialog's key scroll speed, 0 to 6 (fastest). Without it, the stored setting.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=6))]
    pub key_scroll_speed: Option<u8>,
    /// The settings file, read at start and written when the program exits normally. Without it, `settings.txt` in
    /// the `--save-dir` folder if one is given, else in Tidewater's own user data directory.
    #[arg(long)]
    pub settings: Option<PathBuf>,
    /// Development aid: run a scripted scene (`script`, `docs/design/client-core.md`): a fixed frame time and wall
    /// clock, the scene's input instead of the system's, no sound, a 640 × 480 window at one pixel per screen pixel.
    /// It writes the frame and sound fingerprints to `--script-out`, and with a window also the window's own pixels
    /// and the composed frame at each of the scene's shots, compared.
    #[arg(long, requires = "script_out")]
    pub script: Option<String>,
    /// The folder a scripted run writes to (its own saved games and settings go there too, cleared first).
    #[arg(long, requires = "script")]
    pub script_out: Option<PathBuf>,
    /// With `--script`: no window and no GPU; the frames are only composed.
    #[arg(long, requires = "script")]
    pub headless: bool,
    /// Don't check for a newer release at start-up (the `update_check=0` line of the settings file does the same
    /// for good). Development builds never check, and neither do scripted or headless runs.
    #[arg(long)]
    pub no_update_check: bool,
    /// The optional improvements (`extras`): `--extra-NAME` and `--no-extra-NAME`, generated from its table.
    #[command(flatten)]
    pub extras: crate::extras::ExtraFlags,
}

pub fn parse_tile(s: &str) -> Result<(i32, i32), String> {
    let (x, y) = s.split_once(',').ok_or("expected X,Y")?;
    Ok((
        x.trim().parse().map_err(|_| "bad X")?,
        y.trim().parse().map_err(|_| "bad Y")?,
    ))
}
