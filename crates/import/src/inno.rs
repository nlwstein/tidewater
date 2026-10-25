//! Reads the GOG installer (`setup_*.exe` plus its `.bin` part or parts), which is an Inno Setup installer.
//!
//! [`InstallerReader`] is the seam: it supplies the allowlisted game files (the archives at the top and `Maps/`) of
//! an installer as a [`SourceFiles`]. Today it is backed by the `innoextract` program (Daniel Scharrer, zlib licence;
//! found through `TIDEWATER_INNOEXTRACT`, the `PATH` or the usual install folders): it lists the installer, extracts
//! only the allowlisted files into a staging folder, and the rest of the import treats that folder like an installed
//! game. A pure-Rust backend can replace the extraction step later without touching `source`, `validate` or `copy`;
//! see `docs/design/import.md` for why it isn't there yet.
//!
//! Format facts used here, from public sources: an Inno Setup installer is a Windows program whose loader carries the
//! text `Inno Setup Setup Data (<version>)` (the setup-data identifier of the Inno Setup source, `Shared.Struct.pas`
//! in the jrsoftware/issrc repository, which the installer's data begins with); the file data can sit in external
//! slice files next to it, named `<name>-1.bin`, `<name>-2.bin`, ... (Inno Setup's disk spanning), which is what GOG
//! does. `innoextract --list` prints one ` - "path" [language] (size)` line per entry (an entry installed under several names
//! lists them all, comma separated); a path ending in `/` is a folder.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::ImportError;
use crate::copy::{Progress, Step, no_progress};
use crate::source::{FolderFiles, Source, SourceFile, SourceFiles};

/// The marker the Inno Setup loader carries.
const MARKER: &[u8] = b"Inno Setup Setup Data (";
/// How much of the start of the file is searched for the marker (the loader stub is well inside this).
const SCAN_LIMIT: u64 = 8 * 1024 * 1024;
/// How often the staging folder is measured while the tool unpacks.
const POLL: Duration = Duration::from_millis(200);
/// The environment variable that names the `innoextract` program, for when it isn't on the `PATH`.
pub const TOOL_ENV: &str = "TIDEWATER_INNOEXTRACT";

/// A staging folder that is removed when the last reference to it is dropped.
#[derive(Debug)]
pub struct Staging {
    dir: PathBuf,
}

impl Staging {
    pub(crate) fn create() -> std::io::Result<Self> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("tidewater-installer-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// The folder holding the extracted files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Whether the file at `path` is an Inno Setup installer: a Windows program (`MZ`) with the setup-data marker near
/// its start. This is a cheap test; the extraction step finds out whether the version is one it can read.
pub fn is_inno_installer(path: &Path) -> bool {
    let Ok(f) = fs::File::open(path) else { return false };
    let mut data = Vec::new();
    if f.take(SCAN_LIMIT).read_to_end(&mut data).is_err() {
        return false;
    }
    data.starts_with(b"MZ") && data.windows(MARKER.len()).any(|w| w == MARKER)
}

/// The `.exe` that goes with a `.bin` part (`setup_x-1.bin` belongs to `setup_x.exe`), if it is next to it.
pub fn exe_for_part(bin: &Path) -> Option<PathBuf> {
    let stem = bin.file_stem()?.to_str()?;
    let base = stem.rsplit_once('-').map_or(
        stem,
        |(b, n)| {
            if n.chars().all(|c| c.is_ascii_digit()) { b } else { stem }
        },
    );
    let dir = bin.parent()?;
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
                && p.file_stem().and_then(|s| s.to_str()) == Some(base)
        })
}

/// Finds the `innoextract` program: `TIDEWATER_INNOEXTRACT`, then the `PATH`, then the usual install folders (a
/// program started from a desktop shortcut often has a short `PATH`).
pub fn find_tool() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "innoextract.exe"
    } else {
        "innoextract"
    };
    if let Some(p) = std::env::var_os(TOOL_ENV) {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let from_path = std::env::var_os("PATH")
        .map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
        .unwrap_or_default();
    let usual = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/snap/bin"].map(PathBuf::from);
    from_path
        .into_iter()
        .chain(usual)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The allowlisted game files of an Inno Setup installer, staged on disk.
pub struct InstallerReader {
    source: Source,
    files: FolderFiles,
}

impl InstallerReader {
    /// Opens the installer at `exe` with the `innoextract` found by [`find_tool`].
    pub fn open(exe: &Path) -> Result<Self, ImportError> {
        // Check the file before the tool, so a non-installer gets the same error whether or not innoextract exists.
        if !is_inno_installer(exe) {
            return Err(ImportError::InstallerUnsupported(exe.to_path_buf()));
        }
        let tool = find_tool().ok_or_else(|| ImportError::InnoextractMissing(exe.to_path_buf()))?;
        Self::open_with_tool(exe, &tool, &mut no_progress)
    }

    /// [`InstallerReader::open`], telling `progress` how the unpacking goes ([`Step::Unpacking`]).
    pub fn open_with_progress(exe: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Self, ImportError> {
        if !is_inno_installer(exe) {
            return Err(ImportError::InstallerUnsupported(exe.to_path_buf()));
        }
        let tool = find_tool().ok_or_else(|| ImportError::InnoextractMissing(exe.to_path_buf()))?;
        Self::open_with_tool(exe, &tool, progress)
    }

    /// Opens the installer at `exe` with a given `innoextract` program.
    pub fn open_with(exe: &Path, tool: &Path) -> Result<Self, ImportError> {
        Self::open_with_tool(exe, tool, &mut no_progress)
    }

    /// [`InstallerReader::open_with`], telling `progress` how the unpacking goes. The tool reports nothing while it
    /// works, so the bytes done are the staging folder's size, measured every [`POLL`], against the sizes the listing
    /// gives (rounded by the tool, so the last report is exact only at the end).
    pub fn open_with_tool(exe: &Path, tool: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Self, ImportError> {
        if !is_inno_installer(exe) {
            return Err(ImportError::InstallerUnsupported(exe.to_path_buf()));
        }
        let fail = |detail: String| ImportError::InstallerFailed {
            path: exe.to_path_buf(),
            detail: with_parts_hint(exe, detail),
        };

        // 1. List the installer to learn which archives are at the top.
        let out = Command::new(tool)
            .arg("--list")
            .arg(exe)
            .output()
            .map_err(|e| spawn_error(e, exe))?;
        if !out.status.success() {
            return Err(fail(tool_message(&out.stderr)));
        }
        let listing = String::from_utf8_lossy(&out.stdout);
        let mut wanted: Vec<String> = parse_list(&listing)
            .into_iter()
            .filter(|n| !n.contains('/') && n.to_ascii_lowercase().ends_with(".mpq"))
            .collect();
        wanted.sort();
        wanted.dedup();
        if wanted.is_empty() {
            return Err(ImportError::NoGameFiles(exe.to_path_buf()));
        }

        let total: u64 = parse_sizes(&listing)
            .into_iter()
            .filter(|(n, _)| wanted.contains(n) || n.to_ascii_lowercase().starts_with("maps/"))
            .map(|(_, size)| size)
            .sum();

        // 2. Extract only those and the maps folder, measuring the staging folder as it fills.
        let staging = Staging::create().map_err(|e| ImportError::io("creating the staging folder", e))?;
        let mut cmd = Command::new(tool);
        cmd.arg("--silent");
        for n in wanted.iter().map(String::as_str).chain(["Maps"]) {
            cmd.args(["--include", n]);
        }
        let mut child = cmd
            .arg("--output-dir")
            .arg(staging.dir())
            .arg(exe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| spawn_error(e, exe))?;
        // The error output is read on its own thread, so a full pipe can never stall the tool.
        let mut pipe = child.stderr.take();
        let errors = std::thread::spawn(move || {
            let mut v = Vec::new();
            if let Some(p) = pipe.as_mut() {
                let _ = p.read_to_end(&mut v);
            }
            v
        });
        let report = |progress: &mut dyn FnMut(&Progress), done: u64| {
            progress(&Progress {
                step: Step::Unpacking,
                file: "",
                done: if total == 0 { 0 } else { done.min(total) },
                total,
            });
        };
        report(progress, 0);
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|e| ImportError::io("running innoextract", e))?
            {
                break status;
            }
            report(progress, dir_size(staging.dir()));
            std::thread::sleep(POLL);
        };
        let stderr = errors.join().unwrap_or_default();
        if !status.success() {
            let text = tool_message(&stderr);
            if text.to_ascii_lowercase().contains("space") {
                return Err(ImportError::NotEnoughSpace { needed: 0 });
            }
            return Err(fail(text));
        }

        report(progress, total);

        // 3. The staged folder is a game folder like any other.
        let mut source = crate::source::source_at(exe, staging.dir());
        source.kind = crate::source::SourceKind::GogInstaller;
        source.staged = Some(Arc::new(staging));
        if source.archives.is_empty() {
            return Err(ImportError::NoGameFiles(exe.to_path_buf()));
        }
        let files = FolderFiles::new(&source);
        Ok(Self { source, files })
    }

    /// The staged files as a [`Source`] for `validate` and `import`. It keeps the staging folder alive.
    pub fn into_source(self) -> Source {
        self.source
    }

    /// The staged files as a [`Source`] (a clone that shares the staging folder).
    pub fn source(&self) -> &Source {
        &self.source
    }
}

impl SourceFiles for InstallerReader {
    fn list(&self) -> std::io::Result<Vec<SourceFile>> {
        self.files.list()
    }

    fn open(&self, rel: &str) -> std::io::Result<Box<dyn Read>> {
        self.files.open(rel)
    }
}

/// The entry names (folders excluded) of an `innoextract --list` output.
fn parse_list(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim_start().strip_prefix("- \"")?;
            let end = rest.find('"')?;
            let name = &rest[..end];
            (!name.ends_with('/')).then(|| name.to_string())
        })
        .collect()
}

/// Each entry's first name and size from an `innoextract --list` output (folders excluded). The size is the last
/// bracketed group of the line, such as `(4 B)`, `(13.8 KiB)` or `(467 MiB)` (binary units); an entry whose size can't be
/// read counts as 0.
fn parse_sizes(text: &str) -> Vec<(String, u64)> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim_start().strip_prefix("- \"")?;
            let end = rest.find('"')?;
            let name = &rest[..end];
            if name.ends_with('/') {
                return None;
            }
            let size = l
                .trim_end()
                .strip_suffix(')')
                .and_then(|t| t.rfind('(').map(|i| &t[i + 1..]))
                .and_then(parse_size)
                .unwrap_or(0);
            Some((name.to_string(), size))
        })
        .collect()
}

/// A size as `innoextract` prints it: a number, a space and `B`, `bytes`, `KiB`, `MiB` or `GiB`.
fn parse_size(text: &str) -> Option<u64> {
    let (number, unit) = text.trim().split_once(' ')?;
    let scale: f64 = match unit {
        "B" | "bytes" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let n: f64 = number.parse().ok()?;
    (n.is_finite() && n >= 0.0).then(|| (n * scale).round() as u64)
}

/// The bytes of the files under `dir` (what can be read of it; a file being written counts as far as it has got).
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map_or(0, |m| m.len()),
            Err(_) => 0,
        })
        .sum()
}

fn spawn_error(e: std::io::Error, exe: &Path) -> ImportError {
    if e.kind() == std::io::ErrorKind::NotFound {
        ImportError::InnoextractMissing(exe.to_path_buf())
    } else {
        ImportError::io("running innoextract", e)
    }
}

/// The useful end of the tool's error output, on one line.
fn tool_message(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(2)..].join(" ");
    if tail.is_empty() {
        "it reported an error".to_string()
    } else {
        tail.chars().take(300).collect()
    }
}

/// Adds the hint about the `.bin` part when none is next to the installer.
fn with_parts_hint(exe: &Path, detail: String) -> String {
    let has_part = exe
        .parent()
        .zip(exe.file_stem().and_then(|s| s.to_str()))
        .is_some_and(|(dir, stem)| {
            fs::read_dir(dir).is_ok_and(|rd| {
                rd.filter_map(Result::ok).any(|e| {
                    let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                    n.starts_with(&stem.to_ascii_lowercase()) && n.ends_with(".bin")
                })
            })
        });
    if has_part {
        detail
    } else {
        format!("{detail}. The installer's .bin file or files must be in the same folder as the .exe")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_installer(dir: &Path, name: &str) -> PathBuf {
        let mut b = b"MZ".to_vec();
        b.extend(vec![0u8; 100]);
        b.extend_from_slice(MARKER);
        b.extend_from_slice(b"5.6.2) (u)");
        let p = dir.join(name);
        fs::write(&p, b).unwrap();
        p
    }

    #[test]
    fn inno_installers_are_recognised_by_their_marker() {
        let t = tempfile::tempdir().unwrap();
        assert!(is_inno_installer(&fake_installer(t.path(), "setup_x.exe")));
        let plain = t.path().join("other.exe");
        fs::write(&plain, b"MZ plain program").unwrap();
        assert!(!is_inno_installer(&plain));
        let no_mz = t.path().join("nomz.exe");
        fs::write(&no_mz, [MARKER, b"1.0)"].concat()).unwrap();
        assert!(!is_inno_installer(&no_mz));
        assert!(!is_inno_installer(&t.path().join("missing.exe")));
    }

    #[test]
    fn a_part_file_finds_its_installer() {
        let t = tempfile::tempdir().unwrap();
        let exe = fake_installer(t.path(), "setup_game_1.0_(123).exe");
        let bin = t.path().join("setup_game_1.0_(123)-1.bin");
        fs::write(&bin, b"x").unwrap();
        assert_eq!(exe_for_part(&bin), Some(exe));
        assert_eq!(exe_for_part(&t.path().join("lonely-1.bin")), None);
    }

    #[test]
    fn the_listing_is_parsed_without_folders() {
        let text = "Listing \"Game\" - setup data version 5.6.2 (unicode)\n - \"app/Save/\"\n - \"War2Dat.mpq\" [en-US] (1 MiB)\n - \"Maps/a b.pud\" (2 KiB)\n - \"tmp/x.jpg\" [temp], \"tmp/y.jpg\" [temp] (3 KiB)\n";
        assert_eq!(parse_list(text), ["War2Dat.mpq", "Maps/a b.pud", "tmp/x.jpg"]);
    }

    #[test]
    fn the_listing_gives_each_entry_its_size() {
        let text = "Listing \"Game\" - setup data version 5.6.2 (unicode)\n - \"app/Save/\"\n - \"War2Dat.mpq\" [en-US] (25.2 MiB)\n - \"Maps/a (1).pud\" (13.8 KiB)\n - \"tmp/x.jpg\" [temp], \"tmp/y.jpg\" [temp] (3 B)\n - \"big.mpq\" (1.5 GiB)\n - \"odd.mpq\" (lots)\n";
        assert_eq!(
            parse_sizes(text),
            [
                ("War2Dat.mpq".to_string(), 26_424_115),
                ("Maps/a (1).pud".to_string(), 14_131),
                ("tmp/x.jpg".to_string(), 3),
                ("big.mpq".to_string(), 1_610_612_736),
                ("odd.mpq".to_string(), 0),
            ]
        );
        assert_eq!(parse_size("12 bytes"), Some(12));
        assert_eq!(parse_size("-1 KiB"), None);
        assert_eq!(parse_size("1 TiB"), None);
    }

    #[test]
    fn a_missing_tool_and_a_non_inno_exe_are_clear_errors() {
        let t = tempfile::tempdir().unwrap();
        let exe = fake_installer(t.path(), "setup_x.exe");
        let err = InstallerReader::open_with(&exe, &t.path().join("no-such-tool"))
            .err()
            .unwrap();
        assert!(matches!(err, ImportError::InnoextractMissing(_)), "{err}");
        assert!(err.to_string().contains("innoextract"));
        let plain = t.path().join("setup_plain.exe");
        fs::write(&plain, b"MZ nothing").unwrap();
        let err = InstallerReader::open_with(&plain, &t.path().join("no-such-tool"))
            .err()
            .unwrap();
        assert!(matches!(err, ImportError::InstallerUnsupported(_)), "{err}");
    }

    /// A shell script standing in for `innoextract`: lists two archives and a map, extracts what is asked for.
    #[cfg(unix)]
    fn fake_tool(dir: &Path, fail: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let body = if fail {
            "echo 'Error: unexpected setup data version' >&2\nexit 1\n"
        } else {
            r#"
case "$1" in
  --list)
    echo 'Listing "Game" - setup data version 5.6.2 (unicode)'
    echo ' - "War2Dat.mpq" (4 B)'
    echo ' - "Install.mpq" (4 B)'
    echo ' - "Maps/one.pud" (3 B)'
    echo ' - "game.exe" (3 B)'
    exit 0;;
esac
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--output-dir" ]; then out="$2"; fi
  shift
done
mkdir -p "$out/Maps"
printf 'dat1' > "$out/War2Dat.mpq"
sleep 1
printf 'ins1' > "$out/Install.mpq"
printf 'pud' > "$out/Maps/one.pud"
"#
        };
        let p = dir.join("fake-innoextract");
        fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[cfg(unix)]
    #[test]
    fn a_staged_installer_imports_like_a_folder() {
        let t = tempfile::tempdir().unwrap();
        let exe = fake_installer(t.path(), "setup_x.exe");
        let tool = fake_tool(t.path(), false);
        let reader = InstallerReader::open_with(&exe, &tool).unwrap();
        let mut names: Vec<_> = reader.list().unwrap().into_iter().map(|f| f.rel).collect();
        names.sort();
        assert_eq!(names, ["Install.mpq", "Maps/one.pud", "War2Dat.mpq"]);
        let mut s = String::new();
        reader.open("Maps/one.pud").unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "pud");

        let source = reader.into_source();
        assert_eq!(source.kind, crate::source::SourceKind::GogInstaller);
        let staged = source.staged.as_ref().unwrap().dir().to_path_buf();
        assert!(staged.is_dir());
        drop(source);
        assert!(!staged.exists(), "the staging folder goes with the last reference");
    }

    #[cfg(unix)]
    #[test]
    fn unpacking_reports_the_staging_folder_filling_against_the_listed_sizes() {
        let t = tempfile::tempdir().unwrap();
        let exe = fake_installer(t.path(), "setup_x.exe");
        let tool = fake_tool(t.path(), false);
        let mut heard: Vec<(Step, u64, u64)> = Vec::new();
        InstallerReader::open_with_tool(&exe, &tool, &mut |p| heard.push((p.step, p.done, p.total))).unwrap();
        // The two archives and the map (4 + 4 + 3 bytes), not the program the listing also names.
        assert!(heard.iter().all(|h| h.0 == Step::Unpacking && h.2 == 11), "{heard:?}");
        assert_eq!(heard.first().unwrap().1, 0);
        assert_eq!(heard.last().unwrap().1, 11);
        assert!(heard.windows(2).all(|w| w[0].1 <= w[1].1), "{heard:?}");
        // The tool pauses after the first archive: the folder was measured part-full on the way.
        assert!(heard.iter().any(|h| h.1 > 0 && h.1 < 11), "{heard:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_failure_names_the_problem_and_the_missing_part() {
        let t = tempfile::tempdir().unwrap();
        let exe = fake_installer(t.path(), "setup_x.exe");
        let tool = fake_tool(t.path(), true);
        let err = InstallerReader::open_with(&exe, &tool).err().unwrap();
        let text = err.to_string();
        assert!(matches!(err, ImportError::InstallerFailed { .. }), "{text}");
        assert!(text.contains("unexpected setup data version"), "{text}");
        assert!(text.contains(".bin"), "{text}");
        assert_eq!(err.exit_code(), 12);
    }
}
