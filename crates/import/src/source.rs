//! Works out what a path the user gave is: a game folder, a single archive, a folder of loose archives, a GOG
//! installer (read through [`crate::inno`]), or a DOS-edition copy we refuse.
//!
//! Folders are searched a few levels deep (breadth first, so the shallowest match wins), in any letter case, so the
//! user can point at a GOG folder, a Wine prefix's `drive_c`, a macOS `.app` bundle or a mounted CD and still land on
//! the folder that holds `War2Dat.mpq`. Symbolic links to folders are not followed.

use std::fs;
use std::path::{Path, PathBuf};

use std::sync::Arc;

use crate::ImportError;
use crate::copy::Progress;
use crate::inno::{self, InstallerReader, Staging};

/// How deep below the given folder to look for the game files.
const MAX_DEPTH: usize = 6;
/// A cap on the folders looked at, so pointing at a huge tree (a home folder) ends quickly.
const MAX_DIRS: usize = 20_000;

/// What kind of thing the user's path was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// A game folder with the archives and usually other files (an installed game, a CD, an app bundle's contents).
    InstallFolder,
    /// One `.mpq` file.
    SingleMpq,
    /// A folder holding nothing but archives (and perhaps a `Maps` folder).
    MpqFolder,
    /// A GOG (Inno Setup) installer; its game files are staged in a temporary folder.
    GogInstaller,
    /// A CD image (ISO 9660); its game files are staged in a temporary folder.
    CdImage,
}

impl SourceKind {
    /// A stable name for the manifest.
    pub fn name(self) -> &'static str {
        match self {
            Self::InstallFolder => "install-folder",
            Self::SingleMpq => "single-mpq",
            Self::MpqFolder => "mpq-folder",
            Self::GogInstaller => "gog-installer",
            Self::CdImage => "cd-image",
        }
    }

    /// A phrase for messages.
    pub fn describe(self) -> &'static str {
        match self {
            Self::InstallFolder => "a game folder",
            Self::SingleMpq => "a single archive file",
            Self::MpqFolder => "a folder of archive files",
            Self::GogInstaller => "a GOG installer",
            Self::CdImage => "a CD image",
        }
    }
}

/// The game files found at a path.
#[derive(Clone, Debug)]
pub struct Source {
    pub kind: SourceKind,
    /// The path the user gave.
    pub given: PathBuf,
    /// The folder the archives are in (the archive's folder for a single file).
    pub root: PathBuf,
    /// The archives (`*.mpq`) at the top of `root`, sorted by name.
    pub archives: Vec<PathBuf>,
    /// The `Maps` folder next to the archives, if there is one.
    pub maps: Option<PathBuf>,
    /// The temporary folder an installer's files were extracted to; removed when the last copy of the source is dropped.
    pub staged: Option<Arc<Staging>>,
}

/// Classifies `path`, or says why the game files can't be taken from it.
pub fn classify(path: &Path) -> Result<Source, ImportError> {
    classify_with(path, &mut crate::copy::no_progress)
}

/// [`classify`], telling `progress` how a CD image is read or an installer unpacked into a temporary folder
/// ([`Step::ReadingDisc`], [`Step::Unpacking`]); a game folder or an archive needs neither and reports nothing.
///
/// [`Step::ReadingDisc`]: crate::Step::ReadingDisc
/// [`Step::Unpacking`]: crate::Step::Unpacking
pub fn classify_with(path: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Source, ImportError> {
    let meta = fs::metadata(path).map_err(|_| ImportError::NotFound(path.to_path_buf()))?;
    if meta.is_file() {
        return classify_file(path, progress);
    }
    let found = search(path);
    if let Some(root) = found.game {
        return Ok(source_at(path, &root));
    }
    if found.dos {
        return Err(ImportError::DosEdition);
    }
    // A folder with the installer in it (the download folder) gets the installer message.
    if let Some(exe) = installer_in(path) {
        return open_installer(&exe, progress);
    }
    Err(ImportError::NoGameFiles(path.to_path_buf()))
}

fn classify_file(path: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Source, ImportError> {
    let name = file_name(path);
    if has_ext(&name, "mpq") {
        let root = path.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        return Ok(Source {
            kind: SourceKind::SingleMpq,
            given: path.to_path_buf(),
            root,
            archives: vec![path.to_path_buf()],
            maps: None,
            staged: None,
        });
    }
    if has_ext(&name, "exe") && inno::is_inno_installer(path) {
        return open_installer(path, progress);
    }
    if is_installer_name(&name) {
        if has_ext(&name, "bin")
            && let Some(exe) = inno::exe_for_part(path)
        {
            return open_installer(&exe, progress);
        }
        return Err(ImportError::InstallerUnsupported(path.to_path_buf()));
    }
    if is_disc_image(path, &name) {
        return crate::disc::open_source(path, progress);
    }
    if has_ext(&name, "war") {
        return Err(ImportError::DosEdition);
    }
    Err(ImportError::NoGameFiles(path.to_path_buf()))
}

fn open_installer(exe: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Source, ImportError> {
    InstallerReader::open_with_progress(exe, progress).map(InstallerReader::into_source)
}

/// A CD image: a `.cue` sheet, `.img`, `.mdf`, `.mds` or `.nrg` by extension, or any other file (an `.iso`, a `.bin`)
/// by the ISO 9660 signature `CD001` at sector 16 (ECMA-119), whatever the sector size ([`crate::disc`]).
fn is_disc_image(path: &Path, name: &str) -> bool {
    if ["cue", "img", "mdf", "mds", "nrg"].iter().any(|e| has_ext(name, e)) {
        return true;
    }
    (has_ext(name, "iso") || has_ext(name, "bin")) && crate::disc::looks_like_disc(path)
}

/// The files a source can supply, whatever holds them. A plain folder is the first implementation ([`FolderFiles`]);
/// a disc image or an installer reader can implement it later and plug into [`crate::copy`] unchanged.
pub trait SourceFiles {
    /// Every file, with its path relative to the source's root (components separated by `/`) and its size.
    fn list(&self) -> std::io::Result<Vec<SourceFile>>;
    /// Opens the file at a relative path from [`list`](Self::list).
    fn open(&self, rel: &str) -> std::io::Result<Box<dyn std::io::Read>>;
}

/// One file a source can supply.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub rel: String,
    pub size: u64,
}

/// A folder on disk as a [`SourceFiles`]: the archives at the top and everything under the `Maps` folder.
pub struct FolderFiles {
    root: PathBuf,
    archives: Vec<PathBuf>,
    maps: Option<PathBuf>,
}

impl FolderFiles {
    pub fn new(source: &Source) -> Self {
        Self {
            root: source.root.clone(),
            archives: source.archives.clone(),
            maps: source.maps.clone(),
        }
    }
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<SourceFile>) -> std::io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let t = e.file_type()?;
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        let rel = format!("{rel}/{name}");
        if t.is_dir() {
            walk(&e.path(), &rel, out)?;
        } else if t.is_file() {
            out.push(SourceFile {
                rel,
                size: e.metadata()?.len(),
            });
        }
    }
    Ok(())
}

impl SourceFiles for FolderFiles {
    fn list(&self) -> std::io::Result<Vec<SourceFile>> {
        let mut out = Vec::new();
        for a in &self.archives {
            if let Some(name) = a.file_name().and_then(|n| n.to_str()) {
                out.push(SourceFile {
                    rel: name.to_string(),
                    size: fs::metadata(a)?.len(),
                });
            }
        }
        if let Some(m) = &self.maps {
            walk(m, "Maps", &mut out)?;
        }
        Ok(out)
    }

    fn open(&self, rel: &str) -> std::io::Result<Box<dyn std::io::Read>> {
        let path = match rel.strip_prefix("Maps/") {
            Some(sub) => self
                .maps
                .as_ref()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?
                .join(sub),
            None => self.root.join(rel),
        };
        Ok(Box::new(fs::File::open(path)?))
    }
}

/// The installer GOG ships: `setup_*.exe`, with `.bin` parts next to it.
fn is_installer_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("setup") && (lower.ends_with(".exe") || lower.ends_with(".bin"))
}

fn installer_in(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            is_installer_name(&e.file_name().to_string_lossy()) && has_ext(&e.file_name().to_string_lossy(), "exe")
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found.into_iter().next()
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn has_ext(name: &str, ext: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(ext))
}

pub(crate) fn source_at(given: &Path, root: &Path) -> Source {
    let mut archives = Vec::new();
    let mut other_files = false;
    let mut maps = None;
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.filter_map(Result::ok) {
            let Ok(t) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            if t.is_file() {
                if has_ext(&name, "mpq") {
                    archives.push(e.path());
                } else {
                    other_files = true;
                }
            } else if t.is_dir() && name.eq_ignore_ascii_case("maps") {
                maps = Some(e.path());
            }
        }
    }
    archives.sort();
    let kind = if other_files {
        SourceKind::InstallFolder
    } else {
        SourceKind::MpqFolder
    };
    Source {
        kind,
        given: given.to_path_buf(),
        root: root.to_path_buf(),
        archives,
        maps,
        staged: None,
    }
}

#[derive(Default)]
struct Found {
    /// The shallowest folder holding game archives.
    game: Option<PathBuf>,
    /// Whether a DOS-edition layout (`DATA/*.WAR`) turned up.
    dos: bool,
}

/// Breadth-first search for the folder with the archives; `War2Dat.mpq` beats `Install.mpq` beats any other `.mpq`
/// at the same depth.
fn search(start: &Path) -> Found {
    let mut found = Found::default();
    let mut level = vec![start.to_path_buf()];
    let mut seen = 0usize;
    for _depth in 0..=MAX_DEPTH {
        let mut best: Option<(u8, PathBuf)> = None;
        let mut next = Vec::new();
        for dir in &level {
            seen += 1;
            if seen > MAX_DIRS {
                return found;
            }
            let Ok(rd) = fs::read_dir(dir) else { continue };
            let mut score = 0u8;
            for e in rd.filter_map(Result::ok) {
                let Ok(t) = e.file_type() else { continue };
                let name = e.file_name().to_string_lossy().into_owned();
                if t.is_file() && has_ext(&name, "mpq") {
                    let s = if name.eq_ignore_ascii_case("war2dat.mpq") {
                        3
                    } else if name.eq_ignore_ascii_case("install.mpq") {
                        2
                    } else {
                        1
                    };
                    score = score.max(s);
                } else if t.is_dir() {
                    if name.eq_ignore_ascii_case("data") && has_war_files(&e.path()) {
                        found.dos = true;
                    }
                    next.push(e.path());
                }
            }
            if score > 0 && best.as_ref().is_none_or(|(b, _)| score > *b) {
                best = Some((score, dir.clone()));
            }
        }
        if let Some((_, dir)) = best {
            found.game = Some(dir);
            return found;
        }
        next.sort();
        level = next;
        if level.is_empty() {
            break;
        }
    }
    found
}

fn has_war_files(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .any(|e| has_ext(&e.file_name().to_string_lossy(), "war"))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn a_game_folder_is_found_in_any_letter_case() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("WAR2DAT.MPQ"));
        touch(&t.path().join("Warcraft II BNE.exe"));
        touch(&t.path().join("MAPS/one.pud"));
        let s = classify(t.path()).unwrap();
        assert_eq!(s.kind, SourceKind::InstallFolder);
        assert_eq!(s.archives.len(), 1);
        assert!(s.maps.is_some());
    }

    #[test]
    fn a_wine_prefix_or_app_bundle_is_searched_deeply() {
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("drive_c/Program Files/GOG Galaxy/Games/Warcraft II");
        touch(&game.join("War2Dat.mpq"));
        touch(&game.join("game.exe"));
        let s = classify(t.path()).unwrap();
        assert_eq!(s.root, game);
        assert_eq!(s.kind, SourceKind::InstallFolder);

        let t = tempfile::tempdir().unwrap();
        let res = t.path().join("Warcraft II.app/Contents/Resources");
        touch(&res.join("War2Dat.mpq"));
        assert_eq!(classify(&t.path().join("Warcraft II.app")).unwrap().root, res);
    }

    #[test]
    fn the_shallowest_and_best_folder_wins() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("a/Install.mpq"));
        touch(&t.path().join("b/c/War2Dat.mpq"));
        assert_eq!(classify(t.path()).unwrap().root, t.path().join("a"));
    }

    #[test]
    fn a_single_archive_and_a_folder_of_archives() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("War2Dat.mpq"));
        touch(&t.path().join("Install.mpq"));
        let s = classify(t.path()).unwrap();
        assert_eq!(s.kind, SourceKind::MpqFolder);
        assert_eq!(s.archives.len(), 2);
        let s = classify(&t.path().join("War2Dat.mpq")).unwrap();
        assert_eq!(s.kind, SourceKind::SingleMpq);
        assert_eq!(s.root, t.path());
    }

    #[test]
    fn a_dos_edition_copy_is_refused() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("DATA/MAIN.WAR"));
        touch(&t.path().join("WAR2.EXE"));
        assert!(matches!(classify(t.path()), Err(ImportError::DosEdition)));
        assert!(matches!(
            classify(&t.path().join("DATA/MAIN.WAR")),
            Err(ImportError::DosEdition)
        ));
    }

    #[test]
    fn an_installer_that_is_not_inno_setup_is_refused() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("setup_warcraft_ii_2.02_v5_(78104).exe"));
        let exe = t.path().join("setup_warcraft_ii_2.02_v5_(78104).exe");
        assert!(matches!(classify(&exe), Err(ImportError::InstallerUnsupported(_))));
        assert!(matches!(classify(t.path()), Err(ImportError::InstallerUnsupported(_))));
        // A part file with no installer beside it is refused the same way.
        touch(&t.path().join("setup_lonely-1.bin"));
        assert!(matches!(
            classify(&t.path().join("setup_lonely-1.bin")),
            Err(ImportError::InstallerUnsupported(_))
        ));
    }

    #[test]
    fn disc_images_that_are_not_discs_are_refused() {
        let t = tempfile::tempdir().unwrap();
        // Recognised by extension, but junk inside.
        fs::write(t.path().join("disc.cue"), b"nothing useful").unwrap();
        assert!(matches!(
            classify(&t.path().join("disc.cue")),
            Err(ImportError::DiscImageUnreadable { .. })
        ));
        // A .iso or .bin without the ISO 9660 signature is not a disc image.
        fs::write(t.path().join("notes.iso"), vec![0u8; 20 * 2048]).unwrap();
        assert!(matches!(
            classify(&t.path().join("notes.iso")),
            Err(ImportError::NoGameFiles(_))
        ));
    }

    #[test]
    fn missing_paths_and_unrelated_folders_are_errors() {
        let t = tempfile::tempdir().unwrap();
        assert!(matches!(
            classify(&t.path().join("nope")),
            Err(ImportError::NotFound(_))
        ));
        touch(&t.path().join("readme.txt"));
        assert!(matches!(classify(t.path()), Err(ImportError::NoGameFiles(_))));
    }
}
