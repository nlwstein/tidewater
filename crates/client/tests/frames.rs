//! The frame snapshot: runs the client's scripted scenes (`src/script.rs`) without a window on a real installation and
//! checks the fingerprints of every frame and of every sound started against `tests/snapshots/`, so moving the
//! client onto an engine-neutral core (`docs/design/client-core.md`) can show it changes nothing on the screen or in
//! the sound. Set `TIDEWATER_INSTALL` to the game's installation; without it the tests do nothing.
//!
//! **What is hashed.** Each frame is the whole screen (640 × 480, or wider in a `widescreen` scene) as the window would show it, composed on the CPU from
//! the client's layers (`src/frame.rs`); a line covers a run of frames with the same fingerprint. A sound line gives
//! the frame it starts on, a fingerprint of the sound's bytes, its loudness and its playback mode. A file line gives
//! the fingerprint of a file the run wrote (its saved games and screenshots), so saves stay byte for byte the same.
//! The files hold fingerprints only.
//!
//! **The mixed sound.** The core mixes every frame's share of the sound (`src/audio.rs`: 22,050 Hz stereo 16-bit,
//! 367 or 368 sample frames a frame); `tests/snapshots/{scene}-audio.txt` holds a fingerprint per second of it, equal
//! seconds in a row on one line.
//!
//! **Blessing.** When a change to what is shown is intended, regenerate with `TIDEWATER_SNAPSHOT_BLESS=1 cargo test
//! --release -p tidewater-client --test frames` and commit the new files with the change that explains it.
//!
//! **In a window.** `the_window_shows_the_composed_frames` (ignored: it opens a window) runs the scenes in the SDL 3
//! shell's window and compares the window's own pixels, read back before each shot is shown, with the composed frame
//! at each of the scene's shots, and the windowed run's fingerprints with the snapshot. Run it with
//! `cargo test --release -p tidewater-client --test frames -- --ignored`; set `TIDEWATER_SCRIPT_OUT` to a folder to
//! keep the pictures (both frames and their difference) for a look.

use std::path::{Path, PathBuf};
use std::process::Command;

const SCENES: &[&str] = &[
    "campaign",
    "outcome",
    "ending",
    "load",
    "build",
    "carry",
    "flow",
    "custom",
    "settings-screen",
    "settings-ingame",
    "multi",
    "lobby",
    "lobby-extras",
    "extras",
    "extras-bars",
    "extras-groups",
    "extras-attack",
    "extras-sim",
    "extras-keys",
    "extras-zoom",
    "widescreen",
    "widescreen-16x10",
    "touch-select",
    "touch-order",
    "touch-drag",
    "touch-pinch",
    "touch-pan",
    "touch-swipe",
    "touch-help",
    "touch-help-menu",
    "touch-menu",
];

fn on(var: &str) -> bool {
    std::env::var_os(var).is_some_and(|v| !v.is_empty() && v != "0")
}

fn snapshot(scene: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/snapshots/{scene}.txt"))
}

/// The audio snapshot's file for a scene.
fn audio_snapshot(scene: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/snapshots/{scene}-audio.txt"))
}

/// Runs a scene; returns its fingerprint file's text.
fn run(install: &std::ffi::OsStr, scene: &str, out: &Path, headless: bool) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tidewater"));
    // No LAN beacons: a scripted run must not hear (or be heard by) games on the machine's network.
    cmd.env("TIDEWATER_NO_BEACON", "1");
    cmd.arg("--install")
        .arg(install)
        .args(["--script", scene, "--script-out"])
        .arg(out);
    if headless {
        cmd.arg("--headless");
    }
    let status = cmd.status().expect("the client runs");
    assert!(status.success(), "the scripted run of {scene} failed");
    std::fs::read_to_string(out.join(format!("{scene}.txt"))).expect("the run's fingerprints")
}

/// The first differing line, with its neighbours' context.
fn first_difference(want: &str, got: &str) -> Option<String> {
    let (w, g): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    let i = (0..w.len().max(g.len())).find(|&i| w.get(i) != g.get(i))?;
    Some(format!(
        "line {}:\n  expected {}\n  got      {}",
        i + 1,
        w.get(i).unwrap_or(&"(end)"),
        g.get(i).unwrap_or(&"(end)")
    ))
}

#[test]
fn every_scene_shows_and_sounds_as_snapshotted() {
    let Some(install) = std::env::var_os("TIDEWATER_INSTALL") else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let bless = on("TIDEWATER_SNAPSHOT_BLESS");
    let mut failures = Vec::new();
    for scene in SCENES {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let got = run(&install, scene, dir.path(), true);
        let got_audio =
            std::fs::read_to_string(dir.path().join(format!("{scene}-audio.txt"))).expect("the run's mixed sound");
        for (file, got) in [(snapshot(scene), got), (audio_snapshot(scene), got_audio)] {
            if bless {
                std::fs::write(&file, &got).expect("writing the snapshot");
                eprintln!("blessed {}", file.display());
                continue;
            }
            let want = std::fs::read_to_string(&file).expect("the snapshot file");
            if let Some(d) = first_difference(&want, &got) {
                failures.push(format!(
                    "{}: {d}",
                    file.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "the frames, sounds or mixed sound differ from tests/snapshots/:\n{}\nIf the change is intended, bless the files (see this \
         test's module doc).",
        failures.join("\n")
    );
}

/// The first-run screen (`src/firstrun.rs`) needs no game files: the scene `firstrun` runs it over a demo backend with a
/// fake list of installs, so this test never skips. Bless it with the others (see the module doc).
#[test]
fn the_first_run_screen_shows_as_snapshotted() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let out = dir.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_tidewater"))
        // An install in the environment must not matter: the scene never opens one.
        .env_remove("TIDEWATER_INSTALL")
        .arg("--data-dir")
        .arg(dir.path().join("data"))
        .args(["--script", "firstrun", "--script-out"])
        .arg(&out)
        .arg("--headless")
        .status()
        .expect("the client runs");
    assert!(status.success(), "the scripted run of firstrun failed");
    let got = std::fs::read_to_string(out.join("firstrun.txt")).expect("the run's fingerprints");
    let file = snapshot("firstrun");
    if on("TIDEWATER_SNAPSHOT_BLESS") {
        std::fs::write(&file, &got).expect("writing the snapshot");
        eprintln!("blessed {}", file.display());
        return;
    }
    let want = std::fs::read_to_string(&file).expect("the snapshot file");
    if let Some(d) = first_difference(&want, &got) {
        panic!(
            "firstrun.txt differs from tests/snapshots/: {d}\nIf the change is intended, bless the file (see this module's doc)."
        );
    }
}

#[test]
#[ignore = "opens a window"]
fn the_window_shows_the_composed_frames() {
    let Some(install) = std::env::var_os("TIDEWATER_INSTALL") else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    let mut report = Vec::new();
    for scene in SCENES {
        let tmp = tempfile::tempdir().expect("a temporary folder");
        let out = std::env::var_os("TIDEWATER_SCRIPT_OUT").map_or_else(|| tmp.path().to_path_buf(), PathBuf::from);
        let windowed = run(&install, scene, &out, false);
        let compare = std::fs::read_to_string(out.join(format!("{scene}-compare.txt"))).expect("the comparison");
        for line in compare.lines() {
            let (shot, differing) = line.split_once('\t').expect("shot and count");
            if differing != "0" {
                report.push(format!("{scene} {shot}: {differing} pixels differ"));
            }
        }
        // The window shows what the run without one composes.
        let want = std::fs::read_to_string(snapshot(scene)).expect("the snapshot file");
        if let Some(d) = first_difference(&want, &windowed) {
            report.push(format!(
                "{scene}: the windowed run's fingerprints differ from the snapshot at {d}"
            ));
        }
    }
    assert!(report.is_empty(), "{}", report.join("\n"));
}

/// Runs a scene headless with extra arguments; returns whether it succeeded, its frame and sound fingerprints and its
/// error output.
fn run_with(install: &std::ffi::OsStr, scene: &str, out: &Path, extra: &[&std::ffi::OsStr]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_tidewater"))
        .env("TIDEWATER_NO_BEACON", "1")
        .arg("--install")
        .arg(install)
        .args(["--script", scene, "--script-out"])
        .arg(out)
        .arg("--headless")
        .args(extra)
        .output()
        .expect("the client runs");
    let text = |name: String| std::fs::read_to_string(out.join(name)).unwrap_or_default();
    (
        output.status.success(),
        text(format!("{scene}.txt")) + &text(format!("{scene}-audio.txt")),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The `.twrep` file a recording run wrote.
fn recorded(out: &Path) -> PathBuf {
    let dir = out.join("save").join("replays");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the replays folder")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.sort();
    files.pop().expect("a recorded replay")
}

#[test]
fn a_scene_recorded_and_replayed_gives_the_same_frames_and_sound() {
    let Some(install) = std::env::var_os("TIDEWATER_INSTALL") else {
        eprintln!("skipped: set TIDEWATER_INSTALL to run");
        return;
    };
    // The `carry` scene sends a peasant to fell a tree and walks it home; the replay drives the same game from the
    // file (the scene's clicks only select and look; its orders are ignored) and must show and sound the same.
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (ok, want, err) = run_with(&install, "carry", a.path(), &["--record-replay".as_ref()]);
    assert!(ok, "recording failed: {err}");
    let file = recorded(a.path());
    let (ok, got, err) = run_with(&install, "carry", b.path(), &["--replay".as_ref(), file.as_os_str()]);
    assert!(ok, "replaying failed: {err}");
    assert!(!want.is_empty());
    assert_eq!(first_difference(&want, &got), None, "the replay shows something else");
    // Recording does not change the game: the frames are the snapshotted ones.
    let snap = std::fs::read_to_string(snapshot("carry")).expect("the snapshot");
    assert!(want.starts_with(&snap), "recording changed the frames");

    // A damaged file is refused with a message and a failing exit, not a crash.
    let mut bytes = std::fs::read(&file).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    let bad = b.path().join("bad.twrep");
    std::fs::write(&bad, &bytes).unwrap();
    let c = tempfile::tempdir().unwrap();
    let (ok, _, err) = run_with(&install, "carry", c.path(), &["--replay".as_ref(), bad.as_os_str()]);
    assert!(!ok, "a damaged replay must not run");
    assert!(err.contains("bad.twrep") && !err.contains("panicked"), "{err}");
}
