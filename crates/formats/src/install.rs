//! A user's installation: the data archives plus loose files, read through one interface.
//!
//! Lookup order (highest priority first): the enabled mods' files, last mod first ([`crate::mods`]), loose files under
//! the install directory, `War2Patch.mpq`, `War2Dat.mpq`, `Install.mpq`. Names use backslashes and are case-insensitive, as in the archives.
//!
//! The files come from a [`Files`]: a [`Folder`] on disk ([`Install::open`]), or whatever a platform gives
//! ([`Install::from_files`]).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::mods::ModSet;
use crate::mpq::{Archive, ArchiveSet, MpqError, ReadSeek};
use crate::pud::Era;
use crate::tileset::{ChangeTable, ChangeTables, Tileset, TilesetError};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{0}")]
    Mpq(#[from] MpqError),
    #[error("{0}")]
    Tileset(#[from] TilesetError),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("no data archives found in {0}")]
    NoArchives(PathBuf),
    #[error("no data archives found")]
    NoArchivesInFiles,
}

/// The installation's files, by name (components separated by `\` or `/`, matched in any letter case).
pub trait Files: Send + Sync {
    /// Opens the file `name`, or `None` when there is no such file.
    fn open(&self, name: &str) -> std::io::Result<Option<Box<dyn ReadSeek>>>;
}

/// The installation as a folder on disk.
pub struct Folder {
    root: PathBuf,
}

impl Folder {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Files for Folder {
    fn open(&self, name: &str) -> std::io::Result<Option<Box<dyn ReadSeek>>> {
        match find_case_insensitive(&self.root, name) {
            Some(p) => Ok(Some(Box::new(File::open(p)?))),
            None => Ok(None),
        }
    }
}

pub struct Install {
    files: Arc<dyn Files>,
    archives: ArchiveSet,
    mods: Arc<ModSet>,
}

/// Archive files in priority order, lowest first.
const ARCHIVES: [&str; 3] = ["Install.mpq", "War2Dat.mpq", "War2Patch.mpq"];

impl Install {
    /// The installation in the folder `root`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, InstallError> {
        let root = root.as_ref().to_path_buf();
        Self::from_files(Box::new(Folder::new(root.clone()))).map_err(|e| match e {
            InstallError::NoArchivesInFiles => InstallError::NoArchives(root),
            e => e,
        })
    }

    /// The installation whose files `files` gives.
    pub fn from_files(files: Box<dyn Files>) -> Result<Self, InstallError> {
        Self::from_shared(files.into())
    }

    /// A second handle on the same files, with archives of its own, for reading on another thread.
    pub fn reopen(&self) -> Result<Self, InstallError> {
        Ok(Self::from_shared(self.files.clone())?.with_mods(self.mods.clone()))
    }

    fn from_shared(files: Arc<dyn Files>) -> Result<Self, InstallError> {
        let mut list = Vec::new();
        for name in ARCHIVES {
            if let Some(f) = files.open(name)? {
                list.push(Archive::from_reader(f)?);
            }
        }
        if list.is_empty() {
            return Err(InstallError::NoArchivesInFiles);
        }
        Ok(Self {
            files,
            archives: ArchiveSet::new(list),
            mods: Arc::new(ModSet::none()),
        })
    }

    /// This installation with `mods` over it: their files are read before the installation's own, and the last mod's
    /// before the earlier ones'. The archives themselves are never replaced by a mod.
    pub fn with_mods(mut self, mods: Arc<ModSet>) -> Self {
        self.mods = mods;
        self
    }

    /// The enabled mods.
    pub fn mods(&self) -> &Arc<ModSet> {
        &self.mods
    }

    /// An installation with no files at all: every read fails. The client's first-run screen runs on it, before any
    /// game files have been imported.
    pub fn none() -> Self {
        struct Nothing;
        impl Files for Nothing {
            fn open(&self, _name: &str) -> std::io::Result<Option<Box<dyn ReadSeek>>> {
                Ok(None)
            }
        }
        Self {
            files: Arc::new(Nothing),
            archives: ArchiveSet::new(Vec::new()),
            mods: Arc::new(ModSet::none()),
        }
    }

    /// Reads a game file by its archive name, e.g. `rez\unitdata.dat`.
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, InstallError> {
        if let Some(path) = self.mods.find(name) {
            return Ok(std::fs::read(path)?);
        }
        if let Some(mut f) = self.files.open(name)? {
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes)?;
            return Ok(bytes);
        }
        Ok(self.archives.read(name)?)
    }

    /// Whether `name` is a file of a mod, a loose file or in the archives.
    pub fn contains(&self, name: &str) -> bool {
        self.mods.find(name).is_some() || matches!(self.files.open(name), Ok(Some(_))) || self.archives.contains(name)
    }

    /// The hash of the active mods' files, 0 with no mod (see [`crate::identity::DataIdentity`]).
    pub fn mods_hash(&self) -> u64 {
        self.mods.hash()
    }

    /// The active mods, in the order they apply.
    pub fn mod_tags(&self) -> Vec<crate::identity::ModTag> {
        self.mods.tags()
    }

    /// The tileset directory name used by an era.
    pub fn tileset_name(era: Era) -> &'static str {
        match era {
            Era::Forest => "Forest",
            Era::Winter => "Iceland",
            Era::Wasteland => "Swamp",
            Era::Swamp => "XSwamp",
        }
    }

    pub fn tileset(&mut self, era: Era) -> Result<Tileset, InstallError> {
        let n = Self::tileset_name(era);
        let lower = n.to_ascii_lowercase();
        let mut f = |ext: &str| self.read(&format!("Art\\bgs\\{n}\\{lower}.{ext}"));
        let (ppl, cv4, vx4, vr4) = (f("ppl")?, f("cv4")?, f("vx4")?, f("vr4")?);
        let mut ts = Tileset::parse(&ppl, &cv4, &vx4, &vr4)?;
        // The change tables are named after the tileset's first letter, except XSwamp's.
        let letter = if n == "XSwamp" {
            'x'
        } else {
            lower.as_bytes()[0] as char
        };
        let mut table = |kind: &str| -> Result<ChangeTable, InstallError> {
            Ok(ChangeTable::parse(
                &self.read(&format!("Art\\bgs\\{n}\\n{letter}_{kind}.bin"))?,
            )?)
        };
        ts.changes = Some(ChangeTables {
            wall: table("wall")?,
            forest: table("tree")?,
            rock: table("rock")?,
        });
        Ok(ts)
    }
}

/// Resolves a relative path (components separated by `/` or `\`) under `root`, matching each component
/// case-insensitively. A `.` or `..` component, or one with a `:`, finds nothing. On a case-sensitive file system two
/// folders can differ only by case (`Rez` and `rez`); each is searched, the exact spelling first.
pub(crate) fn find_case_insensitive(root: &Path, rel: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = rel.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    // A name never climbs out of the folder it is looked up in.
    if parts.iter().any(|p| *p == ".." || *p == "." || p.contains(':')) {
        return None;
    }
    find_parts(root, &parts)
}

fn find_parts(cur: &Path, parts: &[&str]) -> Option<PathBuf> {
    let Some((part, rest)) = parts.split_first() else {
        return cur.is_file().then(|| cur.to_path_buf());
    };
    let direct = cur.join(part);
    if direct.exists()
        && let Some(found) = find_parts(&direct, rest)
    {
        return Some(found);
    }
    std::fs::read_dir(cur)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_name() != std::ffi::OsStr::new(part))
        .filter(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))
        .find_map(|e| find_parts(&e.path(), rest))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// Files held in memory, as a platform without a file system would give them.
    struct Memory(Vec<(&'static str, &'static [u8])>);

    impl Files for Memory {
        fn open(&self, name: &str) -> std::io::Result<Option<Box<dyn ReadSeek>>> {
            Ok(self
                .0
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, b)| Box::new(Cursor::new(b.to_vec())) as Box<dyn ReadSeek>))
        }
    }

    #[test]
    fn files_without_archives_are_no_installation() {
        let files = Memory(vec![("rez\\unitdata.dat", b"x")]);
        assert!(matches!(
            Install::from_files(Box::new(files)),
            Err(InstallError::NoArchivesInFiles)
        ));
        let dir = std::env::temp_dir().join(format!("tidewater-install-{}", std::process::id()));
        assert!(matches!(Install::open(&dir), Err(InstallError::NoArchives(p)) if p == dir));
    }

    #[test]
    fn an_archive_without_a_header_is_refused() {
        let files = Memory(vec![("INSTALL.MPQ", b"not an archive")]);
        assert!(matches!(
            Install::from_files(Box::new(files)),
            Err(InstallError::Mpq(MpqError::NoHeader))
        ));
    }

    /// A folder install holding an archive of `files`, and an empty mods folder beside it.
    fn folder_install(tag: &str, files: &[(&str, &[u8])]) -> (PathBuf, Install) {
        let root = std::env::temp_dir().join(format!("tidewater-install-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("game")).unwrap();
        std::fs::create_dir_all(root.join("mods")).unwrap();
        std::fs::write(root.join("game").join("War2Dat.mpq"), crate::testmpq::mpq_bytes(files)).unwrap();
        let install = Install::open(root.join("game")).unwrap();
        (root, install)
    }

    fn put(path: PathBuf, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn contains_sees_loose_files_and_archives() {
        let (root, install) = folder_install("contains", &[("rez\\a.dat", b"archive")]);
        assert!(install.contains("REZ\\A.DAT"));
        assert!(!install.contains("rez\\loose.dat"));
        put(root.join("game").join("rez").join("loose.dat"), b"x");
        assert!(install.contains("rez/Loose.dat"), "a loose file counts");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mods_override_loose_files_and_archives_and_the_last_mod_wins() {
        let (root, install) = folder_install(
            "mods",
            &[
                ("rez\\a.dat", b"archive"),
                ("rez\\b.dat", b"archive"),
                ("rez\\c.dat", b"archive"),
            ],
        );
        put(root.join("game").join("rez").join("b.dat"), b"loose");
        put(root.join("mods").join("one").join("Rez").join("A.DAT"), b"one");
        put(root.join("mods").join("one").join("rez").join("b.dat"), b"one");
        put(root.join("mods").join("one").join("rez").join("only-one.dat"), b"one");
        put(root.join("mods").join("two").join("rez").join("b.dat"), b"two");
        let set = ModSet::resolve(&root.join("mods"), &["one".into(), "two".into()]).unwrap();
        let mut install = install.with_mods(Arc::new(set));
        assert_eq!(install.read("rez\\a.dat").unwrap(), b"one", "a mod beats the archive");
        assert_eq!(install.read("rez\\b.dat").unwrap(), b"two", "the last mod wins");
        assert_eq!(
            install.read("rez\\c.dat").unwrap(),
            b"archive",
            "untouched files still come from the game"
        );
        assert!(install.contains("rez\\only-one.dat"), "a file only a mod adds is found");
        assert_eq!(install.read("rez\\only-one.dat").unwrap(), b"one");
        // A second handle reads the same mods.
        let mut again = install.reopen().unwrap();
        assert_eq!(again.read("rez\\b.dat").unwrap(), b"two");
        assert_eq!(again.mods_hash(), install.mods_hash());
        assert_ne!(install.mods_hash(), 0);
        assert_eq!(
            again.mod_tags().iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["one", "two"]
        );
        // The mods are part of the data identity, so a changed mod changes it.
        let before = crate::identity::DataIdentity {
            data_hash: 0,
            mods_hash: install.mods_hash(),
            mods: install.mod_tags(),
        };
        put(root.join("mods").join("two").join("rez").join("b.dat"), b"changed");
        let set = ModSet::resolve(&root.join("mods"), &["one".into(), "two".into()]).unwrap();
        let changed = crate::identity::DataIdentity {
            data_hash: 0,
            mods_hash: set.hash(),
            mods: set.tags(),
        };
        assert!(matches!(
            before.mismatch(&changed),
            Some(crate::identity::DataMismatch::Mods { .. })
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_name_cannot_climb_out_of_the_install() {
        let (root, mut install) = folder_install("climb", &[("rez\\a.dat", b"archive")]);
        put(root.join("secret.txt"), b"private");
        assert!(install.read("..\\secret.txt").is_err());
        assert!(!install.contains("../secret.txt"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
