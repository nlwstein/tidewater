//! `tidewater-editor`: the map editor's command line and window. The editor is in the library (`app`).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use tidewater_editor::app::{Editor, Options};

/// Tidewater's map editor: open, paint and save `.pud` maps with the game's own art.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// The Warcraft II: Battle.net Edition installation (or a folder with its `.mpq` archives) to read the art from.
    /// The editor bundles no game art.
    #[arg(long, env = "TIDEWATER_INSTALL")]
    install: PathBuf,
    /// The map to open. Without one, a new 64 × 64 forest map.
    map: Option<PathBuf>,
    /// The game program "Play in Tidewater" starts. Without it, `tidewater` next to this program.
    #[arg(long, env = "TIDEWATER_GAME")]
    game: Option<PathBuf>,
    /// The data directory (settings, saves) the play test runs the game with, so it never touches your own. Without
    /// it, a `tidewater-editor-play` folder in the system's temporary folder.
    #[arg(long)]
    play_data_dir: Option<PathBuf>,
    /// Development aid: a script of editor actions run one per frame, for demonstrations and screenshots (see
    /// `docs/editor.md`, "Scripted actions"), e.g. `"tool terrain 7 3; click 10 10; shot out.png; quit"`.
    #[arg(long)]
    actions: Option<String>,
    /// The window size in points, `WIDTHxHEIGHT`.
    #[arg(long, default_value = "1400x900")]
    size: String,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let actions = match args.actions.as_deref().map(tidewater_editor::script::parse).transpose() {
        Ok(a) => a.unwrap_or_default(),
        Err(e) => {
            eprintln!("error: --actions: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (w, h) = args
        .size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))
        .unwrap_or((1400.0, 900.0));
    let opts = Options {
        install: args.install,
        map: args.map,
        game: args.game,
        play_data_dir: args
            .play_data_dir
            .unwrap_or_else(|| std::env::temp_dir().join("tidewater-editor-play")),
        actions,
    };
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([w, h])
            .with_title("Tidewater map editor"),
        ..Default::default()
    };
    let r = eframe::run_native(
        "Tidewater map editor",
        native,
        Box::new(|cc| Ok(Box::new(Editor::new(&cc.egui_ctx, opts)?))),
    );
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
