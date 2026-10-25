//! A user's installation: the data archives plus loose files, read through one interface.
//!
//! Lookup order (highest priority first): loose files under the install directory, `War2Patch.mpq`, `War2Dat.mpq`,
//! `Install.mpq`. Names use backslashes and are case-insensitive, as in the archives.
//!
//! The files come from a [`Files`]: a [`Folder`] on disk ([`Install::open`]), or whatever a platform gives
//! ([`Install::from_files`]).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
        Self::from_shared(self.files.clone())
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
        })
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
        }
    }

    /// Reads a game file by its archive name, e.g. `rez\unitdata.dat`.
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, InstallError> {
        if let Some(mut f) = self.files.open(name)? {
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes)?;
            return Ok(bytes);
        }
        Ok(self.archives.read(name)?)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.archives.contains(name)
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

/// Resolves a relative path (components separated by `/`) under `root`, matching each component case-insensitively.
fn find_case_insensitive(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut cur = root.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|p| !p.is_empty()) {
        let direct = cur.join(part);
        if direct.exists() {
            cur = direct;
            continue;
        }
        let entry = std::fs::read_dir(&cur)
            .ok()?
            .filter_map(Result::ok)
            .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        cur = entry.path();
    }
    cur.is_file().then_some(cur)
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
}
