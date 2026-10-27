//! What every platform shell shares (`docs/design/client-core.md`, "Platform trait"), with no engine types: the
//! start of the program from its command line ([`launch`]), a scripted run's frame time ([`scripted_elapsed`]), the
//! run without a window ([`run_headless`]), the mouse lock's pointer ([`LockedPointer`]), frame times for measuring
//! ([`FrameStats`]) and a name for a key the core doesn't know ([`other_key`]).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::args::Args;
use crate::core::{Core, Platform};
use crate::input::{Buttons, Key, WindowGeom};
use crate::storage::Store;
use crate::{display, firstrun, frontend, gamemenu, prefs, script, storage};

/// The program, started from its command line, and what its window asks for.
pub struct Launch {
    pub core: Core,
    /// The window's first title.
    pub title: String,
    /// `--ui-scale`: the window's size in logical points per screen pixel, if given.
    pub ui_scale: Option<u32>,
    /// A scripted run without a window (`--script` with `--headless`).
    pub headless: bool,
}

/// `--import PATH`: checks the game files at `path`, copies them into the data directory, prints what was done and
/// returns the process exit code (0 on success; `ImportError::exit_code` otherwise).
pub fn import_command(path: &std::path::Path, data_dir: Option<&std::path::Path>) -> i32 {
    let data = storage::data_dir(data_dir);
    let run = || -> Result<(), tidewater_import::ImportError> {
        // A progress line when a step starts and about every 10% of it.
        let mut shown: Option<(tidewater_import::Step, u64)> = None;
        let mut show = |p: &tidewater_import::Progress| {
            let tenth = p.done * 10 / p.total.max(1);
            if shown.is_none_or(|(step, at)| step != p.step || (p.total > 0 && tenth > at)) {
                shown = Some((p.step, tenth));
                let what = match p.step {
                    tidewater_import::Step::ReadingDisc => "reading the CD image",
                    tidewater_import::Step::Unpacking => "unpacking the installer",
                    tidewater_import::Step::Checking => "checking",
                    tidewater_import::Step::Copying => "copying",
                };
                if p.total == 0 {
                    eprintln!("  {what}...");
                } else {
                    eprintln!("  {what}... {}%", tenth * 10);
                }
            }
        };
        let source = tidewater_import::classify_with(path, &mut show)?;
        let report = tidewater_import::validate(&source);
        print!("{}", tidewater_import::render_report(&source, &report));
        let report = report.into_result()?;
        let manifest = tidewater_import::import(&source, &report, &data, &mut show)?;
        print!(
            "{}",
            tidewater_import::render_summary(&manifest, &report, &tidewater_import::game_dir(&data))
        );
        Ok(())
    };
    match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            e.exit_code()
        }
    }
}

/// What the program says when it has no game files and can't show the first-run screen (a scripted or headless run).
const NO_GAME_FILES: &str = "No game files yet. Import your Warcraft II: Battle.net Edition files first:\n\n    \
     tidewater --import <path>\n\nwhere <path> is the folder the game is installed in (for example the GOG folder), \
     or a mounted CD.";

/// The flow's first steps for a run that isn't scripted: the map given on the command line starts at once; otherwise
/// the startup sequence ("Startup") or a screen named by `--goto`.
pub(crate) fn start_steps(
    args: &Args,
    prefs: &prefs::Prefs,
    flow: &mut frontend::Flow,
    front: &mut frontend::Front,
) -> Result<Vec<frontend::Step>> {
    Ok(match (&args.map, &args.goto) {
        (Some(map), _) => vec![frontend::Step::Play {
            map: map.clone(),
            setup: args.setup,
        }],
        (None, Some(g)) => {
            let steps = frontend::goto_steps(g).map_err(anyhow::Error::msg)?;
            if let Some((what, e)) = g.split_once(':')
                && what.starts_with("score-")
            {
                let e: u8 = e.parse()?;
                frontend::synthetic_score(flow, front, e, what == "score-victory");
            }
            steps
        }
        // The introduction video plays unless `--no-intro` or the Settings screen's Intro video row (the stored
        // "opening film" bit) says not to.
        (None, None) => frontend::startup_steps(
            false,
            flow.skip_videos,
            !args.no_intro && prefs.has(prefs::bit::OPENING_FILM),
        ),
    })
}

/// The settings file's place: `--settings`, else `settings.txt` in the user folder (`--save-dir`, else Tidewater's own
/// per-user data directory). The Settings screen opens its folder.
pub fn settings_path(args: &Args) -> std::path::PathBuf {
    args.settings
        .clone()
        .unwrap_or_else(|| storage::user_dir(args.save_dir.as_deref(), args.data_dir.as_deref()).join(prefs::FILE_NAME))
}

/// The core on the first-run screen (`firstrun`), over `store` (whose game folder doesn't exist yet): `backend` does
/// the searching and the copying. With `candidates` the list is given and the work runs inside the frames (a scripted
/// scene's, whose frames must come out the same every time); without, the usual places are searched on a thread.
pub fn first_run_core(
    args: Args,
    store: Store,
    backend: Arc<dyn firstrun::Backend>,
    candidates: Option<Vec<firstrun::Candidate>>,
    run: Option<script::Run>,
) -> Core {
    let prefs = if run.is_some() {
        prefs::Prefs::default()
    } else {
        prefs::Prefs::load(&*store.0)
    };
    let stored =
        gamemenu::Stored(prefs.with_args(args.mouse_scroll_speed, args.key_scroll_speed, args.second_mouse_style));
    let screen = match candidates {
        Some(list) => firstrun::FirstRun::with_candidates(backend, list, false),
        None => firstrun::FirstRun::searching(backend),
    }
    .with_intake(firstrun::Intake::native());
    let mut core = Core::new(
        args,
        store,
        frontend::Front::none(),
        frontend::Flow::default(),
        stored,
        run,
    );
    core.first_run = Some(screen);
    core.states.set(frontend::AppState::FirstRun);
    core
}

/// The mods to enable: the names the settings give that exist (one that is gone is reported and skipped, so a deleted
/// folder cannot keep the game from starting), then the names `--mod` gave (one that is wrong is an error).
fn enabled_mods(dir: &std::path::Path, stored: &[String], flags: &[String]) -> Result<Vec<String>> {
    use tidewater_formats::mods::ModSet;
    let mut names = Vec::new();
    for n in stored {
        match ModSet::resolve(dir, std::slice::from_ref(n)) {
            Ok(_) => names.push(n.clone()),
            Err(e) => eprintln!("settings: [mods] {e}; skipped"),
        }
    }
    for n in flags {
        ModSet::resolve(dir, std::slice::from_ref(n)).with_context(|| format!("--mod {n}"))?;
        names.push(n.clone());
    }
    Ok(names)
}

/// Starts the program from its command line: the desktop's storage (`storage::Desktop`), the installation, the
/// flow's first steps (a map, a `--goto` screen, a scripted scene or the startup sequence) and the stored settings.
/// A scripted run gets its own save folder and settings, a wall clock that stands still and no mouse lock.
pub fn launch(mut args: Args) -> Result<Launch> {
    let scripted = match (&args.script, &args.script_out) {
        (Some(name), Some(out)) => {
            let scene = script::scene(name).ok_or_else(|| anyhow::anyhow!("no scripted scene {name}"))?;
            // The run's own saved games and settings, so nothing of the user's changes what it shows.
            let saves = out.join("save");
            if saves.exists() {
                std::fs::remove_dir_all(&saves)?;
            }
            args.save_dir = Some(saves);
            args.settings = Some(out.join("settings.txt"));
            args.no_mouse_lock = true;
            args.ui_scale = Some(1);
            Some((
                scene,
                script::Out {
                    dir: out.clone(),
                    windowed: !args.headless,
                    pictures: std::env::var_os("TIDEWATER_SCRIPT_PICTURES").is_some(),
                    every: std::env::var("TIDEWATER_SCRIPT_PICTURES")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                },
            ))
        }
        _ => None,
    };
    // The desktop's storage (`storage`): a scripted run uses its own folder and a wall clock that stands still.
    let user = storage::user_dir(args.save_dir.as_deref(), args.data_dir.as_deref());
    let settings = args.settings.clone().unwrap_or_else(|| user.join(prefs::FILE_NAME));
    let saves = args.save_dir.clone().unwrap_or_else(|| match &args.data_dir {
        Some(d) => d.join("Save"),
        None => storage::default_save_dir(),
    });
    let data = storage::data_dir(args.data_dir.as_deref());
    // The first-run scene shows the screen over a demo backend, with no game files.
    let first_run_scene = scripted.as_ref().is_some_and(|(s, _)| s.name == script::FIRST_RUN);
    let mut desktop = match &args.install {
        // The developer override reads the files in place.
        Some(install) => storage::Desktop::new(install, saves, settings, user),
        None if (tidewater_import::is_imported(&data) && !args.reimport) || first_run_scene => {
            storage::Desktop::from_data_dir(&data, saves, settings, user)
        }
        // A script or a run without a window can't use the first-run screen.
        None if scripted.is_some() || args.headless => anyhow::bail!(NO_GAME_FILES),
        None => {
            let store = Store(Arc::new(storage::Desktop::from_data_dir(&data, saves, settings, user)));
            let (ui_scale, headless) = (args.ui_scale, false);
            let backend = Arc::new(firstrun::DesktopBackend { data_dir: data });
            let core = first_run_core(args, store, backend, None, None);
            return Ok(Launch {
                core,
                title: "Tidewater".to_string(),
                ui_scale,
                headless,
            });
        }
    };
    if scripted.is_some() {
        desktop = desktop.with_fixed_clock(script::WALL_SECS);
    }
    // The mods: those the settings enable, then the `--mod` flags. A scripted run (the frame snapshot) has none, so what
    // a developer has in the mods folder cannot change its pictures.
    if scripted.is_none() {
        let mods_dir = tidewater_formats::mods::dir(&data);
        let mod_names = enabled_mods(&mods_dir, &prefs::Prefs::load(&desktop).mods, &args.mods)?;
        desktop = desktop.with_mods(mods_dir, &mod_names)?;
        for m in storage::Storage::mods(&desktop).roots() {
            println!("mod: {}", m.info.tag());
        }
    }
    let store = Store(Arc::new(desktop));
    if first_run_scene {
        let (scene, out) = scripted.expect("the first-run scene is scripted");
        let (ui_scale, headless) = (args.ui_scale, args.headless);
        let backend = Arc::new(firstrun::DemoBackend);
        let candidates = Some(firstrun::DemoBackend::candidates());
        let core = first_run_core(args, store, backend, candidates, Some(script::Run::new(scene, out)));
        return Ok(Launch {
            core,
            title: "Tidewater".to_string(),
            ui_scale,
            headless,
        });
    }
    let install = storage::open_install(&store.0)?;
    let mut front = frontend::Front::load(install)?;
    let mut flow = frontend::Flow::default();
    // The map given on the command line starts at once; otherwise the startup sequence ("Startup") or a screen
    // named by `--goto`.
    // `--replay FILE`: the recorded game starts at once (`session::ReplaySession`); a damaged file is refused here.
    let replay = match &args.replay {
        Some(path) => {
            let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            let replay = tidewater_replay::Replay::decode(&bytes).with_context(|| format!("{}", path.display()))?;
            if replay.header.sim_build != tidewater_replay::format::sim_build() {
                eprintln!(
                    "note: the recording is from {}, this is {}; the checkpoints will say whether it still replays",
                    replay.header.sim_build,
                    tidewater_replay::format::sim_build()
                );
            }
            // A recording made on other game files or mods is refused with the reason, rather than going out of sync at
            // the first checkpoint; `--force-replay` plays it anyway.
            if let Some(recorded) = &replay.header.data {
                let now = tidewater_formats::identity::DataIdentity::of(&mut front.install)?;
                if let Some(m) = recorded.mismatch(&now) {
                    if args.force_replay {
                        eprintln!(
                            "warning: this replay was recorded with different game files: {m}; playing it anyway"
                        );
                    } else {
                        anyhow::bail!(
                            "this replay was recorded with different game files: {m} (--force-replay plays it anyway, but expect it to go out of sync)"
                        );
                    }
                }
            }
            Some(replay)
        }
        None => None,
    };
    // `--host` / `--join`: the network game is set up before the window opens, on the command line (`net::lobby`).
    let net = network_start(&args, &mut front.install, &store)?;
    // A scripted run starts from the default settings.
    let prefs = if scripted.is_some() {
        prefs::Prefs::default()
    } else {
        prefs::Prefs::load(&*store.0)
    };
    let steps = match (&replay, &scripted, &net) {
        (_, _, Some(net)) => vec![frontend::Step::Play {
            map: net.config.map_name.clone(),
            setup: Some(net.config.setup),
        }],
        (Some(replay), _, _) => vec![frontend::Step::Play {
            map: replay.header.map_name.clone(),
            setup: Some(replay.header.setup),
        }],
        (None, Some((scene, _)), None) => scene.start.clone(),
        (None, None, None) => start_steps(&args, &prefs, &mut flow, &mut front)?,
    };
    flow.steps = steps.into();
    flow.replay = replay.map(Box::new);
    flow.net = net.map(Box::new);
    let stored =
        gamemenu::Stored(prefs.with_args(args.mouse_scroll_speed, args.key_scroll_speed, args.second_mouse_style));
    let title = match (&flow.replay, args.map.as_deref()) {
        (Some(r), _) => format!("Tidewater — replay of {}", r.header.map_name),
        (None, _) if flow.net.is_some() => "Tidewater — network game".to_string(),
        (None, m) => m.map_or("Tidewater".to_string(), |m| format!("Tidewater — {m}")),
    };
    let (ui_scale, headless) = (args.ui_scale, args.headless && scripted.is_some());
    let run = scripted.map(|(scene, out)| script::Run::new(scene, out));
    let core = Core::new(args, store, front, flow, stored, run);
    Ok(Launch {
        core,
        title,
        ui_scale,
        headless,
    })
}

/// `--host` or `--join`: runs the handshake and returns the game it started. Blocks, saying on the terminal what
/// happens, until every player of the lobby is in. Nothing for a local game.
fn network_start(
    args: &Args,
    install: &mut tidewater_formats::install::Install,
    store: &Store,
) -> Result<Option<crate::net::NetStart>> {
    use crate::net::lobby;
    let say = |line: &str| eprintln!("{line}");
    let name = args.name.clone().unwrap_or_default();
    if let Some(port) = args.host {
        let map = args.map.as_deref().context("--host needs a map")?;
        let mut spec = args.lobby.clone().unwrap_or_default();
        // The options that change the simulation are the host's, from its settings and its command line, and travel to
        // every joiner (`docs/design/extras.md`).
        let mut extras = prefs::Prefs::load(&*store.0).extras;
        args.extras.apply(&mut extras);
        spec.extras = crate::extras::local_sim_extras(&extras);
        // So are the client-only options that give an edge (C+L): the host's own, unless `--lobby extras=...` chooses.
        spec.client_extras = spec.lobby_values(&extras);
        let setup = args.setup.unwrap_or_default();
        let wall = store.wall_clock_secs();
        return lobby::host(Some(port), &spec, map, setup, install, store, &name, wall, say).map(Some);
    }
    if let Some(addr) = &args.join {
        return lobby::join(addr, install, store, &name, args.observe, say).map(Some);
    }
    Ok(None)
}

/// A scripted run's frame time: none for the first frame (the clock's first frame reports none, as the Bevy shell's
/// did), then a fixed [`script::FRAME_SECS`], so a run comes out the same every time.
pub fn scripted_elapsed(first: bool) -> Duration {
    if first {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(script::FRAME_SECS)
    }
}

/// The desktop window's icon (`assets/icon/window.png`, drawn by `packaging/icon/render.sh` from our own
/// `packaging/icon/tidewater.svg`): width, height and RGBA bytes. `None` if it doesn't decode.
pub fn window_icon() -> Option<(u32, u32, Vec<u8>)> {
    const PNG: &[u8] = include_bytes!("../assets/icon/window.png");
    let mut reader = png::Decoder::new(std::io::Cursor::new(PNG)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some((info.width, info.height, buf))
}

/// The window a run without one reports: 640 × 480 physical pixels at one per point, the scripted window's.
struct NoWindow;

impl Platform for NoWindow {
    fn window(&self) -> Option<WindowGeom> {
        Some(WindowGeom::new(display::SCREEN, 1.0))
    }

    fn pointer(&mut self, _keys: &Buttons<Key>, _game_pointer: bool) -> (Option<(f32, f32)>, bool) {
        (None, false)
    }
}

/// Runs a scripted scene without a window, sound device or GPU until it ends: each frame is composed and recorded
/// (`script::record`); the settings are written if the program ends on its own.
pub fn run_headless(core: Core) {
    run_headless_watched(core, |_| {});
}

/// [`run_headless`], with `watch` called after each frame has run and before its sound is mixed (tests check the core's
/// state at every frame with it).
pub(crate) fn run_headless_watched(mut core: Core, mut watch: impl FnMut(&Core)) {
    let mut first = true;
    loop {
        core.advance(scripted_elapsed(first), &mut NoWindow);
        watch(&core);
        first = false;
        let exiting = std::mem::take(&mut core.exit);
        if exiting {
            core.store_settings();
        }
        let output = core.audio.output();
        let finished = match core.script.as_mut() {
            Some(run) => script::record(run, Some(&core.frame), &output, exiting).finished,
            None => false,
        };
        if finished || exiting {
            return;
        }
    }
}

/// The pointer while the mouse lock holds the system's still (`display`, "Mouse lock"), in the window's logical
/// points: the game's own pointer, moved by the mouse's motion and kept on the screen.
#[derive(Default)]
pub struct LockedPointer(pub Option<(f32, f32)>);

impl LockedPointer {
    /// One frame of a lock that holds the system's pointer still: `want` the lock this frame, `system` the system's
    /// pointer (`None` outside the window), `motion` the mouse's motion since the last frame in logical points.
    /// Returns where to put the system's pointer when the lock ends (where ours was).
    pub fn update(
        &mut self,
        want: bool,
        system: Option<(f32, f32)>,
        motion: (f32, f32),
        lb: &display::Letterbox,
        factor: f32,
    ) -> Option<(f32, f32)> {
        let (next, warp) = match (want, self.0) {
            (true, None) => (
                Some(system.map_or_else(|| lb.centre(factor), |p| lb.keep_on_screen(p, factor))),
                None,
            ),
            (true, Some(p)) => (Some(lb.keep_on_screen((p.0 + motion.0, p.1 + motion.1), factor)), None),
            (false, Some(p)) => (None, Some(p)),
            (false, None) => (None, None),
        };
        self.0 = next;
        warp
    }
}

/// Frame times for measuring a shell (`TIDEWATER_FRAME_STATS=1`): every [`FrameStats::EVERY`] it prints the
/// frames' intervals and the shell's own work per frame (the core's frame and showing it), in milliseconds.
pub struct FrameStats {
    on: bool,
    intervals: Vec<f32>,
    work: Vec<f32>,
    since: std::time::Instant,
}

impl Default for FrameStats {
    fn default() -> Self {
        FrameStats {
            on: std::env::var_os("TIDEWATER_FRAME_STATS").is_some_and(|v| !v.is_empty() && v != "0"),
            intervals: Vec::new(),
            work: Vec::new(),
            since: std::time::Instant::now(),
        }
    }
}

impl FrameStats {
    pub const EVERY: Duration = Duration::from_secs(10);

    /// Notes a frame `interval` after the last one, of which the shell worked `work`.
    pub fn frame(&mut self, interval: Duration, work: Duration) {
        if !self.on {
            return;
        }
        self.intervals.push(interval.as_secs_f32() * 1000.0);
        self.work.push(work.as_secs_f32() * 1000.0);
        if self.since.elapsed() >= Self::EVERY {
            eprintln!(
                "frame stats: {} frames, interval {}, work {}",
                self.intervals.len(),
                summary(&mut self.intervals),
                summary(&mut self.work)
            );
            self.intervals.clear();
            self.work.clear();
            self.since = std::time::Instant::now();
        }
    }
}

/// Mean, median, 99th percentile and largest of `v`, in milliseconds.
fn summary(v: &mut [f32]) -> String {
    if v.is_empty() {
        return "-".into();
    }
    v.sort_by(f32::total_cmp);
    let at = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
    let mean = v.iter().sum::<f32>() / v.len() as f32;
    format!(
        "mean {mean:.2} p50 {:.2} p99 {:.2} max {:.2} ms",
        at(0.5),
        at(0.99),
        v[v.len() - 1]
    )
}

/// A key the core has no name for, by the shell's own name for it: the same name gives the same key.
pub fn other_key(name: &str) -> Key {
    Key::Other(1 + (script::fnv64(name.as_bytes()) % u32::MAX as u64) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_icon_decodes() {
        let (w, h, rgba) = window_icon().expect("the embedded icon decodes");
        assert_eq!((w, h), (256, 256));
        assert_eq!(rgba.len(), 256 * 256 * 4);
        assert_eq!(rgba[3], 0, "the corner is clear");
    }

    #[test]
    fn the_locked_pointer_moves_by_the_motion_and_stays_on_the_screen() {
        let lb = display::Letterbox::fit((1280, 960));
        let mut p = LockedPointer::default();
        assert_eq!(p.update(true, Some((100.0, 100.0)), (0.0, 0.0), &lb, 2.0), None);
        assert_eq!(p.0, Some((100.0, 100.0)));
        p.update(true, None, (-500.0, 20.0), &lb, 2.0);
        assert_eq!(p.0, Some((0.0, 120.0)));
        assert_eq!(p.update(false, None, (0.0, 0.0), &lb, 2.0), Some((0.0, 120.0)));
        assert_eq!(p.0, None);
    }

    #[test]
    fn a_locked_pointer_starts_in_the_middle_without_the_system_s() {
        let lb = display::Letterbox::fit((640, 480));
        let mut p = LockedPointer::default();
        p.update(true, None, (0.0, 0.0), &lb, 1.0);
        assert_eq!(p.0, Some((320.0, 240.0)));
    }

    #[test]
    fn a_scripted_run_s_first_frame_has_no_time() {
        assert_eq!(scripted_elapsed(true), Duration::ZERO);
        assert!(scripted_elapsed(false) > Duration::ZERO);
    }
}
