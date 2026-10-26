//! Storage (`docs/design/client-core.md`, migration step 8): everything the core reads or writes outside itself goes
//! through [`Storage`], which the shell provides. The installation's files, the saved games, the settings file,
//! Print Screen pictures and the wall clock. SDL-free.
//!
//! [`Desktop`] is the desktop's: the installation folder given on the command line, the save folder and the user
//! folder (`saves`: `--save-dir`, else Tidewater's per-user data directory) and the system clock.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tidewater_formats::install::{Files, Folder, Install, InstallError};
pub use tidewater_formats::mpq::ReadSeek;

/// Why a saved game couldn't be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    /// The folder or the file couldn't be made.
    CantCreate,
    /// The bytes couldn't all be written; nothing of the file is left.
    WriteFailed,
}

/// What the core reads and writes outside itself. Every method takes `&self`: a platform that needs to change state
/// to read or write keeps it inside.
pub trait Storage: Send + Sync {
    /// The installation's file `name` (archive-style, `\` or `/` between components, any letter case), or `None`.
    fn install(&self, name: &str) -> io::Result<Option<Box<dyn ReadSeek>>>;
    /// A map the user named by its own path (`--map`), or `None` when `path` is no such file; the installation's maps
    /// are read through [`install`](Self::install).
    fn user_map(&self, path: &str) -> io::Result<Option<Vec<u8>>>;
    /// The installation's scenarios: every `.pud` file in its `Maps` folder and the folders below it, as archive-style
    /// names (`Maps\Sub\Name.pud`), in no particular order. Nothing by default (a platform that can't list files).
    fn maps(&self) -> Vec<String> {
        Vec::new()
    }
    /// The names of the files in the save folder, in the folder's own order.
    fn saves(&self) -> Vec<String>;
    fn read_save(&self, file: &str) -> io::Result<Vec<u8>>;
    /// Writes the saved game `file` whole, or leaves no file.
    fn write_save(&self, file: &str, bytes: &[u8]) -> Result<(), WriteError>;
    fn delete_save(&self, file: &str) -> io::Result<()>;
    /// The settings file's bytes, or `None` when there is none.
    fn read_settings(&self) -> Option<Vec<u8>>;
    fn write_settings(&self, bytes: &[u8]) -> io::Result<()>;
    /// Whether a Print Screen picture named `name` exists.
    fn has_screenshot(&self, name: &str) -> bool;
    fn write_screenshot(&self, name: &str, bytes: &[u8]) -> io::Result<()>;
    /// Writes the replay `name` (a `.twrep` file, `session`) whole into the replay folder, making the folder if need
    /// be. Replays go into the user's own data and never into the installation.
    fn write_replay(&self, name: &str, bytes: &[u8]) -> io::Result<()>;
    /// Keeps a map a game's creator sent (`net::room`), in the user's own data and never in the installation, and says
    /// the path to name it by afterwards (what [`user_map`](Self::user_map) reads). `name` is the creator's name for
    /// the map and `hash` its hash; neither is trusted as a file name. A platform that can't keep files says so.
    fn write_received_map(&self, _name: &str, _hash: u64, _bytes: &[u8]) -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this platform can't keep a received map",
        ))
    }
    /// Whole seconds since 1970-01-01 UTC: a new game's seed, a loaded game's reseed, a save's time
    /// (`core/timing-and-rng.md`, "Seeding").
    fn wall_clock_secs(&self) -> u64;
}

/// The storage the platform gives the core.
#[derive(Clone)]
pub struct Store(pub Arc<dyn Storage>);

impl std::ops::Deref for Store {
    type Target = dyn Storage;

    fn deref(&self) -> &(dyn Storage + 'static) {
        &*self.0
    }
}

impl Store {
    /// The save folder.
    pub fn folder(&self) -> crate::saves::SaveDir {
        crate::saves::SaveDir::new(self.0.clone())
    }
}

/// The installation through a [`Storage`].
struct InstallFiles(Arc<dyn Storage>);

impl Files for InstallFiles {
    fn open(&self, name: &str) -> io::Result<Option<Box<dyn ReadSeek>>> {
        self.0.install(name)
    }
}

/// Opens the installation `storage` gives.
pub fn open_install(storage: &Arc<dyn Storage>) -> Result<Install, InstallError> {
    Install::from_files(Box::new(InstallFiles(storage.clone())))
}

/// The PS Vita's folder for everything: saved games, settings and the game files, and where the user copies the game
/// in (`docs/design/vita.md`). The Vita has no home folder for the `directories` crate to find.
pub const VITA_DATA_DIR: &str = "ux0:data/Tidewater";

/// The default save folder: `Save` in the user's data directory for Tidewater.
pub fn default_save_dir() -> PathBuf {
    if cfg!(target_os = "vita") {
        return Path::new(VITA_DATA_DIR).join("Save");
    }
    directories::ProjectDirs::from("", "", "Tidewater")
        .map_or_else(|| PathBuf::from("Save"), |d| d.data_dir().join("Save"))
}

/// The folder for the program's other files (the settings file, Print Screen pictures): the folder given with
/// `--save-dir`, so a run with its own save folder writes nothing else; else `--data-dir`; otherwise the folder that
/// holds the default save folder (the original keeps them in the game's folder, which holds `Save`; we never write
/// into the install).
pub fn user_dir(save_dir: Option<&Path>, data_dir: Option<&Path>) -> PathBuf {
    match (save_dir, data_dir) {
        (Some(d), _) | (None, Some(d)) => d.to_path_buf(),
        (None, None) => {
            let d = default_save_dir();
            d.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        }
    }
}

/// The data directory that holds the imported game files (`game`): `--data-dir`, else the per-user local data
/// directory (on Windows this is the non-roaming one, as the files are hundreds of megabytes).
pub fn data_dir(data_dir: Option<&Path>) -> PathBuf {
    match data_dir {
        Some(d) => d.to_path_buf(),
        None if cfg!(target_os = "vita") => PathBuf::from(VITA_DATA_DIR),
        None => directories::ProjectDirs::from("", "", "Tidewater")
            .map_or_else(|| PathBuf::from("."), |d| d.data_local_dir().to_path_buf()),
    }
}

/// Adds the `.pud` files in `dir` and below it to `out`, named `prefix\...`.
fn list_puds(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if path.is_dir() {
            list_puds(&path, &format!("{prefix}\\{name}"), out);
        } else if path.is_file() && name.to_ascii_lowercase().ends_with(".pud") {
            out.push(format!("{prefix}\\{name}"));
        }
    }
}

/// The desktop's storage: folders on disk and the system clock.
pub struct Desktop {
    install: Folder,
    saves: PathBuf,
    settings: PathBuf,
    shots: PathBuf,
    /// Recorded games: `replays` in the user folder.
    replays: PathBuf,
    /// A wall clock that stands still (a scripted run's, so its games and saves come out the same every time).
    fixed_clock: Option<u64>,
}

impl Desktop {
    /// The installation in `install`, saved games in `saves`, the settings file `settings` and Print Screen pictures in
    /// `shots`.
    pub fn new(install: impl Into<PathBuf>, saves: PathBuf, settings: PathBuf, shots: PathBuf) -> Self {
        Self {
            install: Folder::new(install),
            saves,
            settings,
            replays: shots.join("replays"),
            shots,
            fixed_clock: None,
        }
    }

    /// The imported game in `<data>/game` (`tidewater_import`), saved games in `saves`, the settings file `settings` and
    /// Print Screen pictures in `shots`.
    pub fn from_data_dir(data: &Path, saves: PathBuf, settings: PathBuf, shots: PathBuf) -> Self {
        Self::new(tidewater_import::game_dir(data), saves, settings, shots)
    }

    /// The same storage with a wall clock that always reads `secs`.
    pub fn with_fixed_clock(mut self, secs: u64) -> Self {
        self.fixed_clock = Some(secs);
        self
    }
}

impl Storage for Desktop {
    fn install(&self, name: &str) -> io::Result<Option<Box<dyn ReadSeek>>> {
        self.install.open(name)
    }

    fn user_map(&self, path: &str) -> io::Result<Option<Vec<u8>>> {
        let p = Path::new(path);
        if p.is_file() {
            std::fs::read(p).map(Some)
        } else {
            Ok(None)
        }
    }

    fn maps(&self) -> Vec<String> {
        let root = self.install.root();
        // The folder is found in any letter case, as every installation file is.
        let Some(dir) = std::fs::read_dir(root)
            .ok()
            .and_then(|d| {
                d.flatten()
                    .find(|e| e.path().is_dir() && e.file_name().to_string_lossy().eq_ignore_ascii_case("maps"))
            })
            .map(|e| e.path())
        else {
            return Vec::new();
        };
        let mut found = Vec::new();
        list_puds(&dir, crate::customsetup::MAPS_DIR, &mut found);
        found
    }

    fn saves(&self) -> Vec<String> {
        let Ok(dir) = std::fs::read_dir(&self.saves) else {
            return Vec::new();
        };
        dir.flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect()
    }

    fn read_save(&self, file: &str) -> io::Result<Vec<u8>> {
        std::fs::read(self.saves.join(file))
    }

    fn write_save(&self, file: &str, bytes: &[u8]) -> Result<(), WriteError> {
        std::fs::create_dir_all(&self.saves).map_err(|_| WriteError::CantCreate)?;
        let path = self.saves.join(file);
        let mut f = std::fs::File::create(&path).map_err(|_| WriteError::CantCreate)?;
        let written = io::Write::write_all(&mut f, bytes).and_then(|()| f.sync_all());
        if written.is_err() {
            drop(f);
            let _ = std::fs::remove_file(&path);
            return Err(WriteError::WriteFailed);
        }
        Ok(())
    }

    fn delete_save(&self, file: &str) -> io::Result<()> {
        std::fs::remove_file(self.saves.join(file))
    }

    fn read_settings(&self) -> Option<Vec<u8>> {
        std::fs::read(&self.settings).ok()
    }

    fn write_settings(&self, bytes: &[u8]) -> io::Result<()> {
        if let Some(dir) = self.settings.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.settings, bytes)
    }

    fn has_screenshot(&self, name: &str) -> bool {
        self.shots.join(name).exists()
    }

    fn write_screenshot(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        std::fs::create_dir_all(&self.shots)?;
        std::fs::write(self.shots.join(name), bytes)
    }

    fn write_replay(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        std::fs::create_dir_all(&self.replays)?;
        // Written beside the file and moved over it, so a crash leaves the last whole recording.
        let tmp = self.replays.join(format!("{name}.part"));
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, self.replays.join(name))
    }

    fn write_received_map(&self, name: &str, hash: u64, bytes: &[u8]) -> io::Result<String> {
        let dir = self.shots.join("received-maps");
        std::fs::create_dir_all(&dir)?;
        // The creator's name only suggests the file's: plain characters, no folders; the hash makes it unique.
        let file = name.rsplit(['\\', '/']).next().unwrap_or("");
        let stem: String = file
            .rsplit_once('.')
            .map_or(file, |(s, _)| s)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
            .take(40)
            .collect();
        let stem = if stem.trim().is_empty() { "map" } else { stem.trim() };
        let path = dir.join(format!("{stem}-{hash:016x}.pud"));
        let tmp = dir.join(format!("{stem}-{hash:016x}.part"));
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path.to_string_lossy().into_owned())
    }

    fn wall_clock_secs(&self) -> u64 {
        // Truncated to 32 bits, as the seed takes it.
        self.fixed_clock.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as u32 as u64)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn desktop(root: &Path) -> Desktop {
        Desktop::new(
            root.join("install"),
            root.join("Save"),
            root.join("user").join("settings.txt"),
            root.join("user"),
        )
    }

    #[test]
    fn a_received_map_is_kept_in_the_user_folder_under_a_safe_name() {
        let t = tempfile::tempdir().unwrap();
        let s = desktop(t.path());
        let path = s
            .write_received_map("Maps\\..\\Gold River!.pud", 0xabc, b"pud")
            .unwrap();
        assert!(
            Path::new(&path).starts_with(t.path().join("user").join("received-maps")),
            "{path}"
        );
        assert!(path.ends_with("Gold River-0000000000000abc.pud"), "{path}");
        assert_eq!(s.user_map(&path).unwrap().unwrap(), b"pud");
        let odd = s.write_received_map("\\\\??", 1, b"x").unwrap();
        assert!(odd.ends_with("map-0000000000000001.pud"), "{odd}");
    }

    #[test]
    fn saves_settings_and_screenshots_round_trip() {
        let t = tempfile::tempdir().unwrap();
        let s = desktop(t.path());
        assert!(s.saves().is_empty(), "no folder yet");
        assert_eq!(s.read_settings(), None);
        s.write_save("one.sav", b"abc").unwrap();
        assert_eq!(s.saves(), ["one.sav"]);
        assert_eq!(s.read_save("one.sav").unwrap(), b"abc");
        s.delete_save("one.sav").unwrap();
        assert!(s.saves().is_empty());
        s.write_settings(b"x=1").unwrap();
        assert_eq!(s.read_settings().as_deref(), Some(&b"x=1"[..]));
        assert!(!s.has_screenshot("screen000.pcx"));
        s.write_screenshot("screen000.pcx", b"p").unwrap();
        assert!(s.has_screenshot("screen000.pcx"));
        assert_eq!(
            std::fs::read(t.path().join("user").join("screen000.pcx")).unwrap(),
            b"p"
        );
        // Replays go into the user folder's `replays`, never beside the installation.
        s.write_replay("a.twrep", b"r1").unwrap();
        s.write_replay("a.twrep", b"r2").unwrap();
        assert_eq!(
            std::fs::read(t.path().join("user").join("replays").join("a.twrep")).unwrap(),
            b"r2"
        );
        assert!(!t.path().join("user").join("replays").join("a.twrep.part").exists());
        assert!(!t.path().join("install").exists());
    }

    #[test]
    fn the_scenarios_are_the_puds_in_the_maps_folder_and_below_it() {
        let t = tempfile::tempdir().unwrap();
        let s = desktop(t.path());
        assert!(s.maps().is_empty(), "no installation, no maps");
        let maps = t.path().join("install").join("MAPS");
        std::fs::create_dir_all(maps.join("Sub")).unwrap();
        for f in ["One.pud", "Two.PUD", "notes.txt", "Sub/Three.pud"] {
            std::fs::write(maps.join(f), b"x").unwrap();
        }
        let mut got = s.maps();
        got.sort();
        assert_eq!(got, ["Maps\\One.pud", "Maps\\Sub\\Three.pud", "Maps\\Two.PUD"]);
    }

    #[test]
    fn a_save_folder_that_cant_be_made_is_cant_create() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("Save"), b"a file where the folder should be").unwrap();
        assert_eq!(desktop(t.path()).write_save("a.sav", b"x"), Err(WriteError::CantCreate));
    }

    #[test]
    fn the_installation_is_read_in_any_letter_case() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("install").join("Rez")).unwrap();
        std::fs::write(t.path().join("install").join("Rez").join("UnitData.dat"), b"u").unwrap();
        let s = desktop(t.path());
        let mut f = s.install("rez\\unitdata.dat").unwrap().expect("found");
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        assert_eq!(b, b"u");
        assert!(s.install("rez\\none.dat").unwrap().is_none());
    }

    #[test]
    fn a_fixed_clock_stands_still() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(desktop(t.path()).with_fixed_clock(42).wall_clock_secs(), 42);
        assert!(desktop(t.path()).wall_clock_secs() > 1_000_000_000);
    }
}
