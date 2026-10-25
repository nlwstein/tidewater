//! Copies the allowlisted files (`*.mpq` at the top, everything under `Maps/`) into a temporary folder inside the data
//! directory, hashing as it copies, then swaps that folder in as `<data>/game/`. An existing import is moved aside
//! first and put back if the swap fails, so a failed import never leaves the user without a working copy. Only the
//! allowlist is copied because the installation reader gives loose files priority over the archives.
//!
//! Free space: there is no portable way to ask for it without `unsafe` or a new dependency, so the copy does not check
//! beforehand. A full disk shows up as a write error, which is reported as "not enough free space" (with the bytes
//! the copy needed), and the temporary folder is removed.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::manifest::{FileEntry, Manifest};
use crate::source::{FolderFiles, Source, SourceFile, SourceFiles};
use crate::validate::Report;
use crate::{ImportError, game_dir};

/// What an import is doing when it reports progress (`docs/design/import.md`, "Progress").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Reading the game files out of a CD image into a temporary folder.
    ReadingDisc,
    /// Unpacking a GOG installer into a temporary folder (with `innoextract`).
    Unpacking,
    /// Opening the archives and checking the key files. It has no size to measure: `total` is 0.
    Checking,
    /// Copying the files into Tidewater's data folder, hashing them as it goes.
    Copying,
}

/// Progress for the callback: the step, the file it is on (empty when it doesn't know), and the bytes done of that
/// step's total. A `total` of 0 means the step has no size to measure.
pub struct Progress<'a> {
    pub step: Step,
    pub file: &'a str,
    pub done: u64,
    pub total: u64,
}

/// A callback that ignores progress, for the calls that don't show it.
pub fn no_progress(_: &Progress) {}

/// Whether a source file is one we keep.
fn allowed(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.starts_with("maps/") || (!rel.contains('/') && lower.ends_with(".mpq"))
}

/// Removes a folder when dropped, unless disarmed.
struct Cleanup(Option<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = fs::remove_dir_all(p);
        }
    }
}

/// Copies the source's game files into `<data_dir>/game`, replacing an earlier import, and returns the manifest
/// written there. `report` (from [`crate::validate`]) supplies the edition guess.
pub fn import(
    source: &Source,
    report: &Report,
    data_dir: &Path,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Manifest, ImportError> {
    import_from(
        &FolderFiles::new(source),
        source.kind.name(),
        report,
        data_dir,
        progress,
    )
}

/// [`import`] from any [`SourceFiles`].
pub fn import_from(
    files: &dyn SourceFiles,
    kind: &str,
    report: &Report,
    data_dir: &Path,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Manifest, ImportError> {
    let mut plan: Vec<SourceFile> = files
        .list()
        .map_err(|e| ImportError::io("listing the game files", e))?
        .into_iter()
        .filter(|f| allowed(&f.rel))
        .collect();
    plan.sort_by(|a, b| a.rel.cmp(&b.rel));
    let total: u64 = plan.iter().map(|f| f.size).sum();

    fs::create_dir_all(data_dir).map_err(|e| ImportError::io("creating the data folder", e))?;
    let pid = std::process::id();
    let temp = data_dir.join(format!(".import-{pid}"));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).map_err(|e| ImportError::io("creating the temporary folder", e))?;
    let mut guard = Cleanup(Some(temp.clone()));

    let mut done = 0u64;
    let mut entries = Vec::new();
    for f in &plan {
        let dest = temp.join(&f.rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| space(e, total, "creating a folder"))?;
        }
        let mut input = files
            .open(&f.rel)
            .map_err(|e| ImportError::io(format!("reading {}", f.rel), e))?;
        let mut output = fs::File::create(&dest).map_err(|e| space(e, total, &format!("writing {}", f.rel)))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut size = 0u64;
        loop {
            let n = input
                .read(&mut buf)
                .map_err(|e| ImportError::io(format!("reading {}", f.rel), e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            output
                .write_all(&buf[..n])
                .map_err(|e| space(e, total, &format!("writing {}", f.rel)))?;
            size += n as u64;
            done += n as u64;
            progress(&Progress {
                step: Step::Copying,
                file: &f.rel,
                done,
                total,
            });
        }
        output
            .sync_all()
            .map_err(|e| space(e, total, &format!("writing {}", f.rel)))?;
        entries.push(FileEntry {
            path: f.rel.clone(),
            size,
            sha256: hex(&hasher.finalize()),
        });
    }
    let manifest = Manifest::new(kind, &report.edition, entries);
    manifest.write(&temp)?;

    // Swap in: move the old import aside, move the new one in, drop the old one; undo the first step on failure.
    let game = game_dir(data_dir);
    let old = data_dir.join(format!(".game-old-{pid}"));
    let _ = fs::remove_dir_all(&old);
    let had_old = game.exists();
    if had_old {
        fs::rename(&game, &old).map_err(|e| ImportError::io("replacing the earlier import", e))?;
    }
    if let Err(e) = fs::rename(&temp, &game) {
        if had_old {
            let _ = fs::rename(&old, &game);
        }
        return Err(ImportError::io("moving the import into place", e));
    }
    guard.0 = None;
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(manifest)
}

fn space(e: std::io::Error, needed: u64, context: &str) -> ImportError {
    if e.kind() == std::io::ErrorKind::StorageFull {
        ImportError::NotEnoughSpace { needed }
    } else {
        ImportError::io(context.to_string(), e)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::classify;
    use crate::testutil::write_mpq;
    use crate::validate::{KeyCheck, validate_with};

    fn marker(i: &mut tidewater_formats::install::Install) -> Result<(), String> {
        i.read("rez\\marker.txt").map(|_| ()).map_err(|e| e.to_string())
    }
    const MARKER: &[KeyCheck] = &[KeyCheck {
        label: "the marker",
        check: marker,
    }];

    fn make_source(root: &Path, tag: &[u8]) -> Source {
        write_mpq(&root.join("War2Dat.mpq"), &[("rez\\marker.txt", tag)]);
        fs::write(root.join("game.exe"), b"stray, must not be copied").unwrap();
        fs::create_dir_all(root.join("Maps/Sub")).unwrap();
        fs::write(root.join("Maps/a.pud"), b"pud").unwrap();
        fs::write(root.join("Maps/Sub/b.pud"), b"pud2").unwrap();
        classify(root).unwrap()
    }

    #[test]
    fn copies_only_the_allowlist_and_writes_a_manifest() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        let data = t.path().join("data");
        let source = make_source(&src, b"one");
        let report = validate_with(&source, MARKER);
        let mut last = 0;
        let m = import(&source, &report, &data, &mut |p| {
            assert_eq!(p.step, Step::Copying);
            last = p.done;
        })
        .unwrap();
        let game = data.join("game");
        let paths: Vec<_> = m.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["Maps/Sub/b.pud", "Maps/a.pud", "War2Dat.mpq"]);
        assert!(!game.join("game.exe").exists());
        assert_eq!(fs::read(game.join("Maps/a.pud")).unwrap(), b"pud");
        assert_eq!(last, m.files.iter().map(|f| f.size).sum::<u64>());
        // "pud" hashed with SHA-256.
        let a = m.files.iter().find(|f| f.path == "Maps/a.pud").unwrap();
        assert_eq!(a.sha256, hex(&Sha256::digest(b"pud")));
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(Manifest::read(&game).unwrap(), m);
        assert!(crate::is_imported(&data));
        // No temporary folders are left behind.
        let names: Vec<_> = fs::read_dir(&data).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["game"]);
    }

    #[test]
    fn a_second_import_replaces_the_first() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let s1 = make_source(&t.path().join("a"), b"one");
        import(&s1, &validate_with(&s1, MARKER), &data, &mut |_| {}).unwrap();
        fs::write(data.join("game/leftover.txt"), b"x").unwrap();
        let s2 = make_source(&t.path().join("b"), b"two!");
        let m = import(&s2, &validate_with(&s2, MARKER), &data, &mut |_| {}).unwrap();
        assert!(!data.join("game/leftover.txt").exists());
        let mpq = m.files.iter().find(|f| f.path == "War2Dat.mpq").unwrap();
        assert_eq!(mpq.size, fs::metadata(data.join("game/War2Dat.mpq")).unwrap().len());
        let names: Vec<_> = fs::read_dir(&data).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["game"]);
    }

    #[test]
    fn a_failed_import_keeps_the_earlier_one_and_cleans_up() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let s1 = make_source(&t.path().join("a"), b"one");
        import(&s1, &validate_with(&s1, MARKER), &data, &mut |_| {}).unwrap();
        let s2 = make_source(&t.path().join("b"), b"two");
        let report = validate_with(&s2, MARKER);
        /// Lists a file it can't open, as a disc that stops reading would.
        struct Broken(FolderFiles);
        impl SourceFiles for Broken {
            fn list(&self) -> std::io::Result<Vec<SourceFile>> {
                self.0.list()
            }
            fn open(&self, rel: &str) -> std::io::Result<Box<dyn Read>> {
                if rel == "War2Dat.mpq" {
                    return Err(std::io::Error::other("read error"));
                }
                self.0.open(rel)
            }
        }
        let broken = Broken(FolderFiles::new(&s2));
        let err = import_from(&broken, "x", &report, &data, &mut |_| {}).unwrap_err();
        assert!(matches!(err, ImportError::Io { .. }));
        assert!(crate::is_imported(&data));
        assert_eq!(fs::read_dir(&data).unwrap().count(), 1);
    }
}
